//! Blob schema for transition data posted to Celestia DA.
//!
//! This crate defines the canonical encoding format for state transition blobs.
//! All blobs are deterministically encoded using bincode for verification.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Current schema version for transition blobs.
pub const SCHEMA_VERSION: u8 = 1;

/// Errors that can occur during blob encoding/decoding.
#[derive(Error, Debug)]
pub enum BlobError {
    #[error("encoding error: {0}")]
    Encoding(#[from] bincode::Error),
    #[error("invalid schema version: expected {expected}, got {got}")]
    InvalidVersion { expected: u8, got: u8 },
    #[error("invalid blob hash")]
    InvalidHash,
}

/// A 32-byte hash/root value.
pub type Hash32 = [u8; 32];

/// Transition blob version 1.
///
/// Contains all data needed for independent verification of a state transition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransitionBlobV1 {
    /// Schema version (must be 1).
    pub version: u8,
    /// Application identifier (namespace-derived or app-specific).
    pub app_id: Vec<u8>,
    /// Sequence number / transition index (monotonically increasing).
    pub sequence: u64,
    /// Previous state root.
    pub prev_root: Hash32,
    /// New state root after transition.
    pub new_root: Hash32,
    /// Public inputs (typed encoding by application).
    pub public_inputs: Vec<u8>,
    /// Public outputs (optional: events, receipts, etc.).
    pub public_outputs: Vec<u8>,
    /// SP1 proof bytes.
    pub proof: Vec<u8>,
    /// Hash of the ZK program ELF (binds proof to specific program).
    pub program_hash: Hash32,
    /// Unix timestamp (optional).
    pub timestamp: Option<u64>,
    /// Sequencer signature over the transition (optional).
    pub sequencer_signature: Option<Vec<u8>>,
}

impl TransitionBlobV1 {
    /// Create a new transition blob.
    pub fn new(
        app_id: Vec<u8>,
        sequence: u64,
        prev_root: Hash32,
        new_root: Hash32,
        public_inputs: Vec<u8>,
        proof: Vec<u8>,
        program_hash: Hash32,
    ) -> Self {
        Self {
            version: SCHEMA_VERSION,
            app_id,
            sequence,
            prev_root,
            new_root,
            public_inputs,
            public_outputs: Vec::new(),
            proof,
            program_hash,
            timestamp: None,
            sequencer_signature: None,
        }
    }

    /// Set public outputs.
    pub fn with_public_outputs(mut self, outputs: Vec<u8>) -> Self {
        self.public_outputs = outputs;
        self
    }

    /// Set timestamp.
    pub fn with_timestamp(mut self, ts: u64) -> Self {
        self.timestamp = Some(ts);
        self
    }

    /// Set sequencer signature.
    pub fn with_signature(mut self, sig: Vec<u8>) -> Self {
        self.sequencer_signature = Some(sig);
        self
    }

    /// Encode the blob to bytes using bincode (deterministic).
    pub fn encode(&self) -> Result<Vec<u8>, BlobError> {
        bincode::serialize(self).map_err(BlobError::from)
    }

    /// Decode a blob from bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, BlobError> {
        let blob: Self = bincode::deserialize(bytes)?;
        if blob.version != SCHEMA_VERSION {
            return Err(BlobError::InvalidVersion {
                expected: SCHEMA_VERSION,
                got: blob.version,
            });
        }
        Ok(blob)
    }

    /// Compute the hash of this blob (for indexing/signing).
    pub fn hash(&self) -> Hash32 {
        let encoded = self.encode().expect("encoding should not fail for valid blob");
        let mut hasher = Sha256::new();
        hasher.update(&encoded);
        hasher.finalize().into()
    }

    /// Compute the signing message for sequencer signature.
    /// Format: (sequence, prev_root, new_root, program_hash, blob_hash)
    pub fn signing_message(&self) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&self.sequence.to_le_bytes());
        msg.extend_from_slice(&self.prev_root);
        msg.extend_from_slice(&self.new_root);
        msg.extend_from_slice(&self.program_hash);
        msg.extend_from_slice(&self.hash());
        msg
    }
}

/// Helper to compute hash of arbitrary bytes.
pub fn hash_bytes(data: &[u8]) -> Hash32 {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Convert a hash to hex string.
pub fn hash_to_hex(hash: &Hash32) -> String {
    hex::encode(hash)
}

/// Parse a hex string to hash.
pub fn hex_to_hash(s: &str) -> Result<Hash32, hex::FromHexError> {
    let bytes = hex::decode(s)?;
    if bytes.len() != 32 {
        return Err(hex::FromHexError::InvalidStringLength);
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blob_roundtrip() {
        let blob = TransitionBlobV1::new(
            b"test-app".to_vec(),
            1,
            [0u8; 32],
            [1u8; 32],
            b"public".to_vec(),
            b"proof".to_vec(),
            [2u8; 32],
        )
        .with_timestamp(12345)
        .with_public_outputs(b"outputs".to_vec());

        let encoded = blob.encode().unwrap();
        let decoded = TransitionBlobV1::decode(&encoded).unwrap();
        assert_eq!(blob, decoded);
    }

    #[test]
    fn test_blob_hash_deterministic() {
        let blob = TransitionBlobV1::new(
            b"test".to_vec(),
            1,
            [0u8; 32],
            [1u8; 32],
            vec![],
            vec![],
            [0u8; 32],
        );

        let hash1 = blob.hash();
        let hash2 = blob.hash();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_invalid_version() {
        let mut blob = TransitionBlobV1::new(
            vec![],
            0,
            [0u8; 32],
            [0u8; 32],
            vec![],
            vec![],
            [0u8; 32],
        );
        blob.version = 99;

        let encoded = bincode::serialize(&blob).unwrap();
        let result = TransitionBlobV1::decode(&encoded);
        assert!(matches!(result, Err(BlobError::InvalidVersion { .. })));
    }
}
