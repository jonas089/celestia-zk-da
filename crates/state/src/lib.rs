//! State management with Merkle tree commitment.
//!
//! This crate provides a persistent key-value store backed by sled,
//! with Merkle tree commitment for state roots and proofs.

use merkle::{Hash32, MerkleProof, SparseMerkleTree, UpdateWitness};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during state operations.
#[derive(Error, Debug)]
pub enum StateError {
    #[error("database error: {0}")]
    Database(#[from] sled::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("merkle error: {0}")]
    Merkle(#[from] merkle::MerkleError),
    #[error("key not found: {0}")]
    KeyNotFound(String),
}

/// Key prefix for typed keys.
pub trait KeyPrefix {
    /// The prefix for this key type.
    fn prefix() -> &'static [u8];
}

/// A typed key with a prefix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedKey<T: KeyPrefix> {
    pub id: Vec<u8>,
    _marker: std::marker::PhantomData<T>,
}

impl<T: KeyPrefix> TypedKey<T> {
    /// Create a new typed key.
    pub fn new(id: impl AsRef<[u8]>) -> Self {
        Self {
            id: id.as_ref().to_vec(),
            _marker: std::marker::PhantomData,
        }
    }

    /// Get the full key bytes including prefix.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = T::prefix().to_vec();
        bytes.push(b':');
        bytes.extend_from_slice(&self.id);
        bytes
    }
}

/// State store with Merkle commitment.
pub struct StateStore {
    /// Underlying key-value database.
    db: sled::Db,
    /// Merkle tree for state commitment.
    tree: SparseMerkleTree,
    /// Current transition index.
    transition_index: u64,
}

impl StateStore {
    /// Open or create a state store at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StateError> {
        let db = sled::open(path)?;

        // Load Merkle tree if exists
        let tree = if let Some(data) = db.get(b"__merkle_tree__")? {
            SparseMerkleTree::deserialize(&data)?
        } else {
            SparseMerkleTree::new()
        };

        // Load transition index
        let transition_index = if let Some(data) = db.get(b"__transition_index__")? {
            u64::from_le_bytes(data.as_ref().try_into().unwrap_or([0; 8]))
        } else {
            0
        };

        Ok(Self {
            db,
            tree,
            transition_index,
        })
    }

    /// Create an in-memory state store (for testing).
    pub fn in_memory() -> Result<Self, StateError> {
        let db = sled::Config::new().temporary(true).open()?;
        Ok(Self {
            db,
            tree: SparseMerkleTree::new(),
            transition_index: 0,
        })
    }

    /// Get the current state root.
    pub fn root(&self) -> Hash32 {
        self.tree.root()
    }

    /// Get the current transition index.
    pub fn transition_index(&self) -> u64 {
        self.transition_index
    }

    /// Get a raw value by key.
    pub fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StateError> {
        Ok(self.db.get(key)?.map(|v| v.to_vec()))
    }

    /// Get a typed value by key.
    pub fn get<V: DeserializeOwned>(&self, key: &[u8]) -> Result<Option<V>, StateError> {
        match self.db.get(key)? {
            Some(data) => {
                let value: V = bincode::deserialize(&data)
                    .map_err(|e| StateError::Serialization(e.to_string()))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Get a value with its Merkle proof.
    pub fn get_with_proof(&self, key: &[u8]) -> Result<(Option<Vec<u8>>, MerkleProof), StateError> {
        let value = self.tree.get(key);
        let proof = self.tree.get_proof(key);
        Ok((value, proof))
    }

    /// Insert a raw value.
    pub fn insert_raw(&mut self, key: &[u8], value: Vec<u8>) -> Result<UpdateWitness, StateError> {
        self.db.insert(key, value.as_slice())?;
        let witness = self.tree.insert(key, value);
        Ok(witness)
    }

    /// Insert a typed value.
    pub fn insert<V: Serialize>(&mut self, key: &[u8], value: &V) -> Result<UpdateWitness, StateError> {
        let data = bincode::serialize(value)
            .map_err(|e| StateError::Serialization(e.to_string()))?;
        self.insert_raw(key, data)
    }

    /// Delete a key.
    pub fn delete(&mut self, key: &[u8]) -> Result<UpdateWitness, StateError> {
        self.db.remove(key)?;
        let witness = self.tree.delete(key);
        Ok(witness)
    }

    /// Apply a batch of operations and return all witnesses.
    pub fn apply_batch(&mut self, ops: Vec<StateOp>) -> Result<Vec<UpdateWitness>, StateError> {
        let mut witnesses = Vec::with_capacity(ops.len());

        for op in ops {
            let witness = match op {
                StateOp::Insert { key, value } => self.insert_raw(&key, value)?,
                StateOp::Delete { key } => self.delete(&key)?,
            };
            witnesses.push(witness);
        }

        Ok(witnesses)
    }

    /// Commit the current state (persist Merkle tree and increment transition index).
    pub fn commit(&mut self) -> Result<Hash32, StateError> {
        self.transition_index += 1;

        // Persist Merkle tree
        let tree_data = self.tree.serialize()?;
        self.db.insert(b"__merkle_tree__", tree_data)?;

        // Persist transition index
        self.db
            .insert(b"__transition_index__", &self.transition_index.to_le_bytes())?;

        self.db.flush()?;

        Ok(self.root())
    }

    /// Get a Merkle proof for a key.
    pub fn get_proof(&self, key: &[u8]) -> MerkleProof {
        self.tree.get_proof(key)
    }

    /// Iterate over all keys with a given prefix.
    pub fn scan_prefix(&self, prefix: &[u8]) -> impl Iterator<Item = (Vec<u8>, Vec<u8>)> + '_ {
        self.db.scan_prefix(prefix).filter_map(|r| {
            r.ok().map(|(k, v)| (k.to_vec(), v.to_vec()))
        })
    }
}

/// A state operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StateOp {
    /// Insert or update a key-value pair.
    Insert { key: Vec<u8>, value: Vec<u8> },
    /// Delete a key.
    Delete { key: Vec<u8> },
}

impl StateOp {
    /// Create an insert operation.
    pub fn insert(key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        StateOp::Insert {
            key: key.into(),
            value: value.into(),
        }
    }

    /// Create a delete operation.
    pub fn delete(key: impl Into<Vec<u8>>) -> Self {
        StateOp::Delete { key: key.into() }
    }
}

/// Builder for state transitions.
pub struct TransitionBuilder {
    ops: Vec<StateOp>,
}

impl Default for TransitionBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TransitionBuilder {
    /// Create a new transition builder.
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    /// Add an insert operation.
    pub fn insert(mut self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        self.ops.push(StateOp::insert(key, value));
        self
    }

    /// Add a delete operation.
    pub fn delete(mut self, key: impl Into<Vec<u8>>) -> Self {
        self.ops.push(StateOp::delete(key));
        self
    }

    /// Build the operations list.
    pub fn build(self) -> Vec<StateOp> {
        self.ops
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_store_basic() {
        let mut store = StateStore::in_memory().unwrap();

        store.insert_raw(b"key1", b"value1".to_vec()).unwrap();
        store.insert_raw(b"key2", b"value2".to_vec()).unwrap();

        assert_eq!(store.get_raw(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(store.get_raw(b"key2").unwrap(), Some(b"value2".to_vec()));
        assert_eq!(store.get_raw(b"key3").unwrap(), None);
    }

    #[test]
    fn test_state_store_with_proof() {
        let mut store = StateStore::in_memory().unwrap();

        store.insert_raw(b"key", b"value".to_vec()).unwrap();
        let root = store.root();

        let (value, proof) = store.get_with_proof(b"key").unwrap();
        assert_eq!(value, Some(b"value".to_vec()));
        assert!(proof.verify(&root));
    }

    #[test]
    fn test_state_store_batch() {
        let mut store = StateStore::in_memory().unwrap();

        let ops = TransitionBuilder::new()
            .insert(b"key1", b"value1")
            .insert(b"key2", b"value2")
            .build();

        let witnesses = store.apply_batch(ops).unwrap();
        assert_eq!(witnesses.len(), 2);

        assert_eq!(store.get_raw(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(store.get_raw(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_typed_values() {
        let mut store = StateStore::in_memory().unwrap();

        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Account {
            balance: u64,
            nonce: u64,
        }

        let account = Account { balance: 100, nonce: 0 };
        store.insert(b"account:alice", &account).unwrap();

        let loaded: Account = store.get(b"account:alice").unwrap().unwrap();
        assert_eq!(loaded, account);
    }
}
