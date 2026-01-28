//! Standalone verifier library for proof chains.
//!
//! This crate provides functionality to verify the entire proof chain
//! from Celestia DA, ensuring:
//! - Each proof is valid
//! - Root continuity is maintained
//! - Program hash matches expected

use anyhow::Result;
use blob_schema::TransitionBlobV1;
use celestia_adapter::{CelestiaClient, Namespace};
use merkle::Hash32;
use thiserror::Error;
use tracing::{debug, info};
use zk_host_harness::{program_hash, TransitionVerifier};

/// Verification errors.
#[derive(Error, Debug)]
pub enum VerifyError {
    #[error("celestia error: {0}")]
    Celestia(#[from] celestia_adapter::CelestiaError),
    #[error("blob decode error: {0}")]
    BlobDecode(#[from] blob_schema::BlobError),
    #[error("proof verification failed at sequence {sequence}: {message}")]
    ProofInvalid { sequence: u64, message: String },
    #[error("root chain broken at sequence {sequence}: expected {expected}, got {actual}")]
    RootChainBroken {
        sequence: u64,
        expected: String,
        actual: String,
    },
    #[error("program hash mismatch at sequence {sequence}")]
    ProgramHashMismatch { sequence: u64 },
    #[error("no blobs found")]
    NoBlobsFound,
}

/// Result of verification.
#[derive(Debug, Clone)]
pub struct VerificationResult {
    /// Total transitions verified.
    pub total_transitions: u64,
    /// First root in the chain.
    pub first_root: Hash32,
    /// Latest root after all transitions.
    pub latest_root: Hash32,
    /// First sequence number.
    pub first_sequence: u64,
    /// Last sequence number.
    pub last_sequence: u64,
    /// Celestia heights covered.
    pub height_range: (u64, u64),
    /// Transitions with empty proofs (not verified).
    pub unverified_transitions: Vec<u64>,
}

/// Configuration for verification.
#[derive(Debug, Clone)]
pub struct VerifyConfig {
    /// Celestia RPC URL.
    pub celestia_rpc: String,
    /// Namespace to verify.
    pub namespace: Namespace,
    /// Expected program hash (uses default if None).
    pub expected_program_hash: Option<Hash32>,
    /// Skip proof verification (only check root chain).
    pub skip_proof_verification: bool,
    /// Expected first root (optional).
    pub expected_first_root: Option<Hash32>,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            celestia_rpc: celestia_adapter::DEFAULT_RPC_URL.to_string(),
            namespace: Namespace::from_string("zkapp"),
            expected_program_hash: None,
            skip_proof_verification: false,
            expected_first_root: None,
        }
    }
}

/// Proof chain verifier.
pub struct ChainVerifier {
    client: CelestiaClient,
    verifier: TransitionVerifier,
    config: VerifyConfig,
}

impl ChainVerifier {
    /// Create a new verifier.
    pub fn new(config: VerifyConfig) -> Self {
        Self {
            client: CelestiaClient::with_url(&config.celestia_rpc),
            verifier: TransitionVerifier::new(),
            config,
        }
    }

    /// Verify all transitions in a height range.
    pub async fn verify_range(
        &self,
        from_height: u64,
        to_height: u64,
    ) -> Result<VerificationResult, VerifyError> {
        info!(
            "Verifying transitions from height {} to {}",
            from_height, to_height
        );

        // Fetch all blobs
        let blobs = self
            .client
            .get_blobs_range(&self.config.namespace, from_height, to_height)
            .await?;

        if blobs.is_empty() {
            return Err(VerifyError::NoBlobsFound);
        }

        // Decode and sort by sequence
        let mut transitions: Vec<(u64, TransitionBlobV1)> = Vec::new();
        for (height, blob) in blobs {
            let transition = TransitionBlobV1::decode(&blob.data)?;
            transitions.push((height, transition));
        }
        transitions.sort_by_key(|(_, t)| t.sequence);

        let expected_program_hash = self
            .config
            .expected_program_hash
            .unwrap_or_else(program_hash);

        // Verify each transition
        let first = &transitions[0];
        let mut current_root = self.config.expected_first_root.unwrap_or(first.1.prev_root);
        let first_root = current_root;
        let first_sequence = first.1.sequence;
        let first_height = first.0;
        let mut last_sequence = first_sequence;
        let mut last_height = first_height;
        let mut unverified = Vec::new();

        for (height, transition) in &transitions {
            debug!(
                "Verifying transition {} at height {}",
                transition.sequence, height
            );

            // Check program hash
            if transition.program_hash != expected_program_hash {
                return Err(VerifyError::ProgramHashMismatch {
                    sequence: transition.sequence,
                });
            }

            // Check root continuity
            if transition.prev_root != current_root {
                return Err(VerifyError::RootChainBroken {
                    sequence: transition.sequence,
                    expected: hex::encode(current_root),
                    actual: hex::encode(transition.prev_root),
                });
            }

            // Verify proof
            if !self.config.skip_proof_verification && !transition.proof.is_empty() {
                match self.verifier.verify(&transition.proof) {
                    Ok(output) => {
                        if output.prev_root != transition.prev_root
                            || output.new_root != transition.new_root
                        {
                            return Err(VerifyError::ProofInvalid {
                                sequence: transition.sequence,
                                message: "proof output mismatch".to_string(),
                            });
                        }
                    }
                    Err(e) => {
                        return Err(VerifyError::ProofInvalid {
                            sequence: transition.sequence,
                            message: e.to_string(),
                        });
                    }
                }
            } else if transition.proof.is_empty() {
                unverified.push(transition.sequence);
            }

            current_root = transition.new_root;
            last_sequence = transition.sequence;
            last_height = *height;
        }

        let result = VerificationResult {
            total_transitions: transitions.len() as u64,
            first_root,
            latest_root: current_root,
            first_sequence,
            last_sequence,
            height_range: (first_height, last_height),
            unverified_transitions: unverified,
        };

        info!(
            "Verification complete: {} transitions, root {} -> {}",
            result.total_transitions,
            hex::encode(result.first_root),
            hex::encode(result.latest_root)
        );

        Ok(result)
    }

    /// Get the current head height.
    pub async fn head_height(&self) -> Result<u64, VerifyError> {
        Ok(self.client.get_head_height().await?)
    }

    /// Check if Celestia node is ready.
    pub async fn is_ready(&self) -> bool {
        self.client.is_ready().await.unwrap_or(false)
    }
}

/// Verify a single blob's proof.
pub fn verify_blob(blob: &TransitionBlobV1) -> Result<(), VerifyError> {
    if blob.proof.is_empty() {
        return Ok(());
    }

    let verifier = TransitionVerifier::new();
    let output = verifier
        .verify(&blob.proof)
        .map_err(|e| VerifyError::ProofInvalid {
            sequence: blob.sequence,
            message: e.to_string(),
        })?;

    if output.prev_root != blob.prev_root || output.new_root != blob.new_root {
        return Err(VerifyError::ProofInvalid {
            sequence: blob.sequence,
            message: "output mismatch".to_string(),
        });
    }

    Ok(())
}
