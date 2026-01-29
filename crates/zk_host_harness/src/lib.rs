//! SP1 host harness for proof generation and verification.
//!
//! This crate provides the host-side functionality for:
//! - Building transition inputs
//! - Generating proofs
//! - Verifying proofs
//! - Extracting outputs

use merkle::{Hash32, UpdateWitness};
use sha2::{Digest, Sha256};
use sp1_sdk::{include_elf, ProverClient, SP1ProofWithPublicValues, SP1Stdin, SP1VerifyingKey};
use thiserror::Error;
use tracing::info;
use transition_format::{TransitionInput, TransitionOutput};

/// The ELF binary for the state transition program.
pub const TRANSITION_ELF: &[u8] = include_elf!("zk_guest_transition");

/// Errors that can occur during proving.
#[derive(Error, Debug)]
pub enum ProverError {
    #[error("execution failed: {0}")]
    Execution(String),
    #[error("proof generation failed: {0}")]
    ProofGeneration(String),
    #[error("verification failed: {0}")]
    Verification(String),
    #[error("output decode failed: {0}")]
    OutputDecode(String),
}

/// Result of proving a transition.
pub struct ProofResult {
    /// The SP1 proof.
    pub proof: SP1ProofWithPublicValues,
    /// The transition output extracted from the proof.
    pub output: TransitionOutput,
    /// Proof bytes (serialized).
    pub proof_bytes: Vec<u8>,
}

/// Hash of the transition program ELF.
pub fn program_hash() -> Hash32 {
    let mut hasher = Sha256::new();
    hasher.update(TRANSITION_ELF);
    hasher.finalize().into()
}

/// State transition prover.
pub struct TransitionProver {
    // We don't store the client since methods need different trait bounds
}

impl Default for TransitionProver {
    fn default() -> Self {
        Self::new()
    }
}

impl TransitionProver {
    /// Create a new prover.
    pub fn new() -> Self {
        Self {}
    }

    /// Execute the transition without generating a proof (for testing).
    pub fn execute(&self, input: &TransitionInput) -> Result<TransitionOutput, ProverError> {
        let client = ProverClient::from_env();

        let mut stdin = SP1Stdin::new();
        stdin.write(input);

        info!("Executing transition...");

        let (public_values, _report) = client
            .execute(TRANSITION_ELF, &stdin)
            .run()
            .map_err(|e| ProverError::Execution(e.to_string()))?;

        let output: TransitionOutput = bincode::deserialize(public_values.as_slice())
            .map_err(|e| ProverError::OutputDecode(e.to_string()))?;

        info!(
            "Execution complete: prev_root={}, new_root={}",
            hex::encode(&output.prev_root),
            hex::encode(&output.new_root)
        );

        Ok(output)
    }

    /// Generate a proof for a transition.
    pub fn prove(&self, input: &TransitionInput) -> Result<ProofResult, ProverError> {
        let client = ProverClient::from_env();

        let mut stdin = SP1Stdin::new();
        stdin.write(input);

        info!("Setting up prover...");
        let (pk, _vk) = client.setup(TRANSITION_ELF);

        info!("Generating proof...");
        let proof = client
            .prove(&pk, &stdin)
            .run()
            .map_err(|e| ProverError::ProofGeneration(e.to_string()))?;

        // Extract output from proof
        let output: TransitionOutput = bincode::deserialize(proof.public_values.as_slice())
            .map_err(|e| ProverError::OutputDecode(e.to_string()))?;

        // Serialize proof
        let proof_bytes =
            bincode::serialize(&proof).map_err(|e| ProverError::ProofGeneration(e.to_string()))?;

        info!(
            "Proof generated: size={} bytes, prev_root={}, new_root={}",
            proof_bytes.len(),
            hex::encode(&output.prev_root),
            hex::encode(&output.new_root)
        );

        Ok(ProofResult {
            proof,
            output,
            proof_bytes,
        })
    }

    /// Get the verifying key for the transition program.
    pub fn verifying_key(&self) -> SP1VerifyingKey {
        let client = ProverClient::from_env();
        let (_pk, vk) = client.setup(TRANSITION_ELF);
        vk
    }
}

/// State transition verifier.
pub struct TransitionVerifier {
    vk: SP1VerifyingKey,
}

impl TransitionVerifier {
    /// Create a new verifier.
    pub fn new() -> Self {
        let client = ProverClient::from_env();
        let (_pk, vk) = client.setup(TRANSITION_ELF);
        Self { vk }
    }

    /// Create a verifier with a custom verifying key.
    pub fn with_vk(vk: SP1VerifyingKey) -> Self {
        Self { vk }
    }

    /// Verify a proof and extract the output.
    pub fn verify(&self, proof_bytes: &[u8]) -> Result<TransitionOutput, ProverError> {
        let proof: SP1ProofWithPublicValues = bincode::deserialize(proof_bytes)
            .map_err(|e| ProverError::Verification(e.to_string()))?;

        let client = ProverClient::from_env();
        client
            .verify(&proof, &self.vk)
            .map_err(|e| ProverError::Verification(e.to_string()))?;

        let output: TransitionOutput = bincode::deserialize(proof.public_values.as_slice())
            .map_err(|e| ProverError::OutputDecode(e.to_string()))?;

        Ok(output)
    }

    /// Verify a proof object directly.
    pub fn verify_proof(
        &self,
        proof: &SP1ProofWithPublicValues,
    ) -> Result<TransitionOutput, ProverError> {
        let client = ProverClient::from_env();
        client
            .verify(proof, &self.vk)
            .map_err(|e| ProverError::Verification(e.to_string()))?;

        let output: TransitionOutput = bincode::deserialize(proof.public_values.as_slice())
            .map_err(|e| ProverError::OutputDecode(e.to_string()))?;

        Ok(output)
    }
}

impl Default for TransitionVerifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a transition input from state operations.
pub fn build_transition_input(
    prev_root: Hash32,
    public_inputs: Vec<u8>,
    private_inputs: Vec<u8>,
    witnesses: Vec<UpdateWitness>,
) -> TransitionInput {
    TransitionInput::new(prev_root, public_inputs, private_inputs, witnesses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use merkle::SparseMerkleTree;

    #[test]
    fn test_program_hash() {
        let hash = program_hash();
        assert_ne!(hash, [0u8; 32]);
        // Hash should be deterministic
        assert_eq!(hash, program_hash());
    }

    #[test]
    #[ignore] // Requires SP1 toolchain
    fn test_execute_empty_transition() {
        let prover = TransitionProver::new();

        let input = TransitionInput::new(
            [0u8; 32], // Empty tree root (not actually correct, but for smoke test)
            vec![],
            vec![],
            vec![],
        );

        let output = prover.execute(&input).unwrap();
        assert_eq!(output.prev_root, input.prev_root);
    }

    #[test]
    #[ignore] // Requires SP1 toolchain
    fn test_execute_with_witnesses() {
        let prover = TransitionProver::new();

        // Create a real merkle tree and generate witnesses
        let mut tree = SparseMerkleTree::new();
        let prev_root = tree.root();

        let witness = tree.insert(b"key", b"value".to_vec());
        let new_root = tree.root();

        let input = TransitionInput::new(
            prev_root,
            b"public".to_vec(),
            b"private".to_vec(),
            vec![witness],
        );

        let output = prover.execute(&input).unwrap();
        assert_eq!(output.prev_root, prev_root);
        assert_eq!(output.new_root, new_root);
    }
}
