//! State transition format for ZK proving.
//!
//! This crate defines the input/output formats for state transitions
//! that can be verified in a ZK circuit (SP1).
//!
//! The key insight is that we need to verify both:
//! 1. Merkle tree correctness (witnesses produce valid roots)
//! 2. Business logic correctness (operations are valid according to app rules)

use merkle::{Hash32, UpdateWitness};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Input to the SP1 state transition program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionInput {
    /// Previous state root (public).
    pub prev_root: Hash32,
    /// Public inputs (application-specific).
    pub public_inputs: Vec<u8>,
    /// Private inputs (application-specific).
    pub private_inputs: Vec<u8>,
    /// Update witnesses for all touched keys.
    pub witnesses: Vec<UpdateWitness>,
    /// Operations with their verification data.
    pub operations: Vec<VerifiableOperation>,
}

impl TransitionInput {
    /// Create a new transition input.
    pub fn new(
        prev_root: Hash32,
        public_inputs: Vec<u8>,
        private_inputs: Vec<u8>,
        witnesses: Vec<UpdateWitness>,
    ) -> Self {
        Self {
            prev_root,
            public_inputs,
            private_inputs,
            witnesses,
            operations: Vec::new(),
        }
    }

    /// Add operations for verification.
    pub fn with_operations(mut self, ops: Vec<VerifiableOperation>) -> Self {
        self.operations = ops;
        self
    }

    /// Encode to bytes for SP1 input.
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("serialization should not fail")
    }

    /// Decode from bytes.
    pub fn decode(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }

    /// Hash the public inputs.
    pub fn public_inputs_hash(&self) -> Hash32 {
        let mut hasher = Sha256::new();
        hasher.update(&self.prev_root);
        hasher.update(&self.public_inputs);
        hasher.finalize().into()
    }
}

/// Output from the SP1 state transition program.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransitionOutput {
    /// Previous state root (for verification).
    pub prev_root: Hash32,
    /// New state root after transition.
    pub new_root: Hash32,
    /// Hash of public inputs (for binding).
    pub public_inputs_hash: Hash32,
    /// Public outputs (application-specific).
    pub public_outputs: Vec<u8>,
}

impl TransitionOutput {
    /// Create a new transition output.
    pub fn new(
        prev_root: Hash32,
        new_root: Hash32,
        public_inputs_hash: Hash32,
        public_outputs: Vec<u8>,
    ) -> Self {
        Self {
            prev_root,
            new_root,
            public_inputs_hash,
            public_outputs,
        }
    }

    /// Encode to bytes.
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("serialization should not fail")
    }

    /// Decode from bytes.
    pub fn decode(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}

/// A verifiable operation that includes all data needed for circuit verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiableOperation {
    /// Operation type tag.
    pub op_type: OperationType,
    /// The key being operated on.
    pub key: Vec<u8>,
    /// Old value (from witness).
    pub old_value: Option<Vec<u8>>,
    /// New value (to be written).
    pub new_value: Option<Vec<u8>>,
    /// Witness index (links to TransitionInput.witnesses).
    pub witness_index: usize,
}

/// Types of operations that can be verified.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OperationType {
    /// Set a value (no constraints).
    Set,
    /// Create an account with initial balance.
    CreateAccount { initial_balance: u64 },
    /// Transfer funds between accounts.
    Transfer {
        from: Vec<u8>,
        to: Vec<u8>,
        amount: u64,
    },
    /// Mint new tokens (requires authority).
    Mint { amount: u64 },
    /// Burn tokens.
    Burn { amount: u64 },
}

/// Finance-specific operations for the example app.
pub mod finance {
    use super::*;

    /// A balance transfer operation.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Transfer {
        pub from: String,
        pub to: String,
        pub amount: u64,
        pub nonce: u64,
    }

    /// Account state.
    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    pub struct Account {
        pub balance: u64,
        pub nonce: u64,
    }

    impl Account {
        pub fn encode(&self) -> Vec<u8> {
            bincode::serialize(self).expect("encoding should not fail")
        }

        pub fn decode(data: &[u8]) -> Option<Self> {
            bincode::deserialize(data).ok()
        }
    }

    /// Verify a transfer operation inside the circuit.
    /// Returns true if the transfer is valid.
    pub fn verify_transfer(
        from_old: &Account,
        from_new: &Account,
        to_old: &Account,
        to_new: &Account,
        amount: u64,
        expected_nonce: u64,
    ) -> bool {
        // Check sender has sufficient balance
        if from_old.balance < amount {
            return false;
        }

        // Check nonce is correct
        if from_old.nonce != expected_nonce {
            return false;
        }

        // Check balances are updated correctly
        if from_new.balance != from_old.balance - amount {
            return false;
        }

        if to_new.balance != to_old.balance + amount {
            return false;
        }

        // Check sender nonce is incremented
        if from_new.nonce != from_old.nonce + 1 {
            return false;
        }

        // Check receiver nonce is unchanged
        if to_new.nonce != to_old.nonce {
            return false;
        }

        true
    }
}

/// A single state operation to be applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    /// Set a key to a value.
    Set { key: Vec<u8>, value: Vec<u8> },
    /// Delete a key.
    Delete { key: Vec<u8> },
}

impl Operation {
    /// Create a set operation.
    pub fn set(key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Operation::Set {
            key: key.into(),
            value: value.into(),
        }
    }

    /// Create a delete operation.
    pub fn delete(key: impl Into<Vec<u8>>) -> Self {
        Operation::Delete { key: key.into() }
    }

    /// Get the key this operation affects.
    pub fn key(&self) -> &[u8] {
        match self {
            Operation::Set { key, .. } => key,
            Operation::Delete { key } => key,
        }
    }
}

/// A batch of operations forming a transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationBatch {
    /// Operations to apply.
    pub operations: Vec<Operation>,
    /// Application-specific metadata.
    pub metadata: Vec<u8>,
}

impl OperationBatch {
    /// Create a new operation batch.
    pub fn new(operations: Vec<Operation>) -> Self {
        Self {
            operations,
            metadata: Vec::new(),
        }
    }

    /// Add metadata.
    pub fn with_metadata(mut self, metadata: Vec<u8>) -> Self {
        self.metadata = metadata;
        self
    }

    /// Encode to bytes.
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("serialization should not fail")
    }

    /// Decode from bytes.
    pub fn decode(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}

/// Verifies that a sequence of witnesses produces the expected root transition.
pub fn verify_witnesses(
    prev_root: Hash32,
    witnesses: &[UpdateWitness],
) -> Result<Hash32, &'static str> {
    let mut current_root = prev_root;

    for witness in witnesses {
        // Verify the witness starts from our current root
        let computed_old_root = witness.compute_old_root();
        if computed_old_root != current_root {
            return Err("witness old root mismatch");
        }

        // Compute new root
        current_root = witness.compute_new_root();
    }

    Ok(current_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use merkle::SparseMerkleTree;

    #[test]
    fn test_transition_input_roundtrip() {
        let input = TransitionInput::new(
            [1u8; 32],
            b"public".to_vec(),
            b"private".to_vec(),
            vec![],
        );

        let encoded = input.encode();
        let decoded = TransitionInput::decode(&encoded).unwrap();

        assert_eq!(input.prev_root, decoded.prev_root);
        assert_eq!(input.public_inputs, decoded.public_inputs);
        assert_eq!(input.private_inputs, decoded.private_inputs);
    }

    #[test]
    fn test_transition_output_roundtrip() {
        let output = TransitionOutput::new(
            [1u8; 32],
            [2u8; 32],
            [3u8; 32],
            b"outputs".to_vec(),
        );

        let encoded = output.encode();
        let decoded = TransitionOutput::decode(&encoded).unwrap();

        assert_eq!(output, decoded);
    }

    #[test]
    fn test_verify_witnesses() {
        let mut tree = SparseMerkleTree::new();
        let root0 = tree.root();

        let w1 = tree.insert(b"key1", b"value1".to_vec());
        let w2 = tree.insert(b"key2", b"value2".to_vec());
        let root2 = tree.root();

        let result = verify_witnesses(root0, &[w1, w2]).unwrap();
        assert_eq!(result, root2);
    }

    #[test]
    fn test_verify_witnesses_mismatch() {
        let mut tree = SparseMerkleTree::new();
        let _w1 = tree.insert(b"key1", b"value1".to_vec());
        let w2 = tree.insert(b"key2", b"value2".to_vec());

        // Try to verify with wrong starting root
        let wrong_root = [99u8; 32];
        let result = verify_witnesses(wrong_root, &[w2]);
        assert!(result.is_err());
    }

    #[test]
    fn test_finance_transfer_verification() {
        use finance::*;

        let from_old = Account { balance: 100, nonce: 0 };
        let from_new = Account { balance: 70, nonce: 1 };
        let to_old = Account { balance: 50, nonce: 5 };
        let to_new = Account { balance: 80, nonce: 5 };

        assert!(verify_transfer(&from_old, &from_new, &to_old, &to_new, 30, 0));

        // Invalid: insufficient balance
        let from_old_poor = Account { balance: 10, nonce: 0 };
        assert!(!verify_transfer(&from_old_poor, &from_new, &to_old, &to_new, 30, 0));

        // Invalid: wrong nonce
        assert!(!verify_transfer(&from_old, &from_new, &to_old, &to_new, 30, 1));
    }
}
