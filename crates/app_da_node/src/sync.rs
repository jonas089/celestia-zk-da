//! Celestia sync functionality for verifying proof chain.

use anyhow::Result;
use blob_schema::TransitionBlobV1;
use celestia_adapter::{CelestiaClient, Namespace};
use merkle::Hash32;
use tracing::{debug, info, warn};
use zk_host_harness::{program_hash, TransitionVerifier};

/// Result of syncing from Celestia.
#[derive(Debug)]
pub struct SyncResult {
    /// Number of transitions verified.
    pub transitions_verified: u64,
    /// Starting root.
    pub first_root: Hash32,
    /// Final root after all transitions.
    pub latest_root: Hash32,
    /// First Celestia height scanned.
    pub first_height: u64,
    /// Last Celestia height scanned.
    pub last_height: u64,
    /// Any errors encountered (non-fatal).
    pub warnings: Vec<String>,
}

/// Sync and verify proof chain from Celestia.
pub struct CelestiaSyncer {
    client: CelestiaClient,
    verifier: TransitionVerifier,
    namespace: Namespace,
    expected_program_hash: Hash32,
}

impl CelestiaSyncer {
    /// Create a new syncer.
    pub fn new(celestia_rpc: &str, namespace: Namespace) -> Self {
        Self {
            client: CelestiaClient::with_url(celestia_rpc),
            verifier: TransitionVerifier::new(),
            namespace,
            expected_program_hash: program_hash(),
        }
    }

    /// Sync from a height range and verify all proofs.
    pub async fn sync_range(
        &self,
        from_height: u64,
        to_height: u64,
        expected_prev_root: Option<Hash32>,
    ) -> Result<SyncResult> {
        info!(
            "Syncing from height {} to {} for namespace",
            from_height, to_height
        );

        let blobs = self
            .client
            .get_blobs_range(&self.namespace, from_height, to_height)
            .await?;

        if blobs.is_empty() {
            return Err(anyhow::anyhow!("no blobs found in range"));
        }

        let mut transitions: Vec<(u64, TransitionBlobV1)> = Vec::new();
        let mut warnings = Vec::new();

        // Decode all blobs
        for (height, blob) in blobs {
            match TransitionBlobV1::decode(&blob.data) {
                Ok(transition) => {
                    transitions.push((height, transition));
                }
                Err(e) => {
                    warnings.push(format!("Failed to decode blob at height {}: {}", height, e));
                }
            }
        }

        if transitions.is_empty() {
            return Err(anyhow::anyhow!("no valid transition blobs found"));
        }

        // Sort by sequence number
        transitions.sort_by_key(|(_, t)| t.sequence);

        // Verify chain
        let first_transition = &transitions[0].1;
        let mut current_root = expected_prev_root.unwrap_or(first_transition.prev_root);
        let first_root = current_root;
        let first_height = transitions[0].0;
        let mut last_height = first_height;

        for (height, transition) in &transitions {
            debug!(
                "Verifying transition {} at height {}",
                transition.sequence, height
            );

            // Verify program hash
            if transition.program_hash != self.expected_program_hash {
                warnings.push(format!(
                    "Transition {} has unexpected program hash",
                    transition.sequence
                ));
                continue;
            }

            // Verify root continuity
            if transition.prev_root != current_root {
                return Err(anyhow::anyhow!(
                    "Root chain broken at transition {}: expected {}, got {}",
                    transition.sequence,
                    hex::encode(current_root),
                    hex::encode(transition.prev_root)
                ));
            }

            // Verify proof (if not empty)
            if !transition.proof.is_empty() {
                match self.verifier.verify(&transition.proof) {
                    Ok(output) => {
                        if output.prev_root != transition.prev_root {
                            return Err(anyhow::anyhow!(
                                "Proof prev_root mismatch at transition {}",
                                transition.sequence
                            ));
                        }
                        if output.new_root != transition.new_root {
                            return Err(anyhow::anyhow!(
                                "Proof new_root mismatch at transition {}",
                                transition.sequence
                            ));
                        }
                        debug!("Proof verified for transition {}", transition.sequence);
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!(
                            "Proof verification failed at transition {}: {}",
                            transition.sequence,
                            e
                        ));
                    }
                }
            } else {
                warnings.push(format!(
                    "Transition {} has no proof (skipping verification)",
                    transition.sequence
                ));
            }

            current_root = transition.new_root;
            last_height = *height;
        }

        info!(
            "Sync complete: {} transitions verified, root {} -> {}",
            transitions.len(),
            hex::encode(first_root),
            hex::encode(current_root)
        );

        Ok(SyncResult {
            transitions_verified: transitions.len() as u64,
            first_root,
            latest_root: current_root,
            first_height,
            last_height,
            warnings,
        })
    }

    /// Get the current head height from Celestia.
    pub async fn head_height(&self) -> Result<u64> {
        Ok(self.client.get_head_height().await?)
    }
}
