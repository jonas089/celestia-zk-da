//! Core app node implementation.

use anyhow::Result;
use blob_schema::TransitionBlobV1;
use celestia_adapter::{CelestiaClient, Namespace};
use merkle::{Hash32, MerkleProof};
use state::{StateOp, StateStore};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};
use transition_format::{TransitionInput, VerifiableOperation};
use zk_host_harness::{program_hash, TransitionProver};

/// Configuration for the app node.
#[derive(Debug, Clone)]
pub struct AppNodeConfig {
    /// Path to the state database.
    pub data_dir: PathBuf,
    /// Application ID (used in blobs).
    pub app_id: Vec<u8>,
    /// Celestia namespace.
    pub namespace: Namespace,
    /// Celestia RPC URL.
    pub celestia_rpc: String,
    /// Whether to actually post to Celestia (disable for testing).
    pub celestia_enabled: bool,
    /// Whether to generate real proofs (disable for faster testing).
    pub proving_enabled: bool,
}

impl Default for AppNodeConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("./data"),
            app_id: b"default-app".to_vec(),
            namespace: Namespace::from_string("zkapp"),
            celestia_rpc: celestia_adapter::DEFAULT_RPC_URL.to_string(),
            celestia_enabled: true,
            proving_enabled: true,
        }
    }
}

/// Shared state for the app node.
pub struct AppNodeState {
    /// State store.
    pub store: StateStore,
    /// Celestia client.
    pub celestia: CelestiaClient,
    /// Prover instance.
    pub prover: TransitionProver,
    /// Configuration.
    pub config: AppNodeConfig,
    /// Historical roots (sequence -> (root, celestia_height)).
    pub root_history: Vec<(Hash32, Option<u64>)>,
}

/// The application DA node.
pub struct AppNode {
    /// Shared state.
    state: Arc<RwLock<AppNodeState>>,
}

impl AppNode {
    /// Create a new app node.
    pub async fn new(config: AppNodeConfig) -> Result<Self> {
        // Create data directory if needed
        std::fs::create_dir_all(&config.data_dir)?;

        // Open state store
        let store = StateStore::open(config.data_dir.join("state"))?;
        let initial_root = store.root();

        // Create Celestia client
        let celestia = CelestiaClient::with_url(&config.celestia_rpc);

        // Create prover
        let prover = TransitionProver::new();

        // Initialize root history with genesis
        let root_history = vec![(initial_root, None)];

        let state = AppNodeState {
            store,
            celestia,
            prover,
            config,
            root_history,
        };

        Ok(Self {
            state: Arc::new(RwLock::new(state)),
        })
    }

    /// Create an in-memory node for testing.
    pub async fn in_memory(config: AppNodeConfig) -> Result<Self> {
        let store = StateStore::in_memory()?;
        let initial_root = store.root();
        let celestia = CelestiaClient::with_url(&config.celestia_rpc);
        let prover = TransitionProver::new();
        let root_history = vec![(initial_root, None)];

        let state = AppNodeState {
            store,
            celestia,
            prover,
            config,
            root_history,
        };

        Ok(Self {
            state: Arc::new(RwLock::new(state)),
        })
    }

    /// Get the shared state.
    pub fn state(&self) -> Arc<RwLock<AppNodeState>> {
        Arc::clone(&self.state)
    }

    /// Get the current state root.
    pub async fn root(&self) -> Hash32 {
        let state = self.state.read().await;
        state.store.root()
    }

    /// Get the current transition index.
    pub async fn transition_index(&self) -> u64 {
        let state = self.state.read().await;
        state.store.transition_index()
    }

    /// Get a value from the state.
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let state = self.state.read().await;
        Ok(state.store.get_raw(key)?)
    }

    /// Get a value with its Merkle proof.
    pub async fn get_with_proof(&self, key: &[u8]) -> Result<(Option<Vec<u8>>, MerkleProof)> {
        let state = self.state.read().await;
        Ok(state.store.get_with_proof(key)?)
    }

    /// Apply a batch of operations and generate a proof.
    pub async fn apply_transition(
        &self,
        ops: Vec<StateOp>,
        public_inputs: Vec<u8>,
        private_inputs: Vec<u8>,
        verifiable_ops: Vec<VerifiableOperation>,
    ) -> Result<TransitionResult> {
        let mut state = self.state.write().await;

        let prev_root = state.store.root();
        let sequence = state.store.transition_index() + 1;

        info!("Applying transition {}: {} operations", sequence, ops.len());

        // Apply operations and collect witnesses
        let witnesses = state.store.apply_batch(ops)?;

        // Commit the state changes
        let new_root = state.store.commit()?;

        debug!(
            "State updated: {} -> {}",
            hex::encode(prev_root),
            hex::encode(new_root)
        );

        // Build transition input with operations for business logic verification
        let input =
            TransitionInput::new(prev_root, public_inputs.clone(), private_inputs, witnesses)
                .with_operations(verifiable_ops);

        // Generate proof (or just execute for testing)
        let (proof_bytes, output) = if state.config.proving_enabled {
            let result = state.prover.prove(&input)?;
            (result.proof_bytes, result.output)
        } else {
            let output = state.prover.execute(&input)?;
            (Vec::new(), output)
        };

        // Verify the output matches our computation
        assert_eq!(output.prev_root, prev_root);
        assert_eq!(output.new_root, new_root);

        // Create blob
        let blob = TransitionBlobV1::new(
            state.config.app_id.clone(),
            sequence,
            prev_root,
            new_root,
            public_inputs,
            proof_bytes.clone(),
            program_hash(),
        )
        .with_timestamp(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );

        // Post to Celestia if enabled
        let celestia_result = if state.config.celestia_enabled {
            let blob_bytes = blob.encode()?;
            info!("Posting blob to Celestia: {} bytes", blob_bytes.len());
            match state
                .celestia
                .submit_blob(&state.config.namespace, &blob_bytes)
                .await
            {
                Ok(result) => {
                    info!("Blob posted at height {}", result.height);
                    Some(result)
                }
                Err(e) => {
                    tracing::warn!("Failed to post to Celestia: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Update root history
        state
            .root_history
            .push((new_root, celestia_result.as_ref().map(|r| r.height)));

        Ok(TransitionResult {
            sequence,
            prev_root,
            new_root,
            proof_bytes,
            blob,
            celestia_height: celestia_result.as_ref().map(|r| r.height),
        })
    }

    /// Get root history.
    pub async fn root_history(&self) -> Vec<(u64, Hash32, Option<u64>)> {
        let state = self.state.read().await;
        state
            .root_history
            .iter()
            .enumerate()
            .map(|(i, (root, height))| (i as u64, *root, *height))
            .collect()
    }
}

/// Result of applying a transition.
#[derive(Debug)]
pub struct TransitionResult {
    /// Sequence number of this transition.
    pub sequence: u64,
    /// Previous state root.
    pub prev_root: Hash32,
    /// New state root.
    pub new_root: Hash32,
    /// Proof bytes (empty if proving disabled).
    pub proof_bytes: Vec<u8>,
    /// The blob that was/would be posted.
    pub blob: TransitionBlobV1,
    /// Celestia height where blob was posted (if posted).
    pub celestia_height: Option<u64>,
}
