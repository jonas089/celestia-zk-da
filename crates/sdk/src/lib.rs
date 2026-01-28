//! SDK for building ZK applications with Celestia DA.
//!
//! This SDK provides a high-level interface for developers to build
//! ZK applications where they only need to implement business logic.
//!
//! # Example
//!
//! ```ignore
//! use sdk::{Application, Context, Result};
//!
//! struct MyApp;
//!
//! impl Application for MyApp {
//!     type PublicInput = TransferRequest;
//!     type PrivateInput = TransferAuth;
//!     type Output = TransferReceipt;
//!
//!     fn apply(
//!         &self,
//!         ctx: &mut Context,
//!         public: Self::PublicInput,
//!         private: Self::PrivateInput,
//!     ) -> Result<Self::Output> {
//!         // Your business logic here
//!     }
//! }
//! ```

use merkle::{Hash32, MerkleProof, UpdateWitness};
use serde::{de::DeserializeOwned, Serialize};
use state::{StateOp, StateStore};
use std::collections::HashMap;
use thiserror::Error;

pub use merkle;
pub use state;
pub use transition_format;

/// SDK errors.
#[derive(Error, Debug)]
pub enum SdkError {
    #[error("state error: {0}")]
    State(#[from] state::StateError),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("application error: {0}")]
    Application(String),
    #[error("key not found: {0}")]
    KeyNotFound(String),
}

/// Result type for SDK operations.
pub type Result<T> = std::result::Result<T, SdkError>;

/// Context for application execution.
///
/// Provides state read/write operations that automatically track
/// Merkle witnesses for ZK proving.
pub struct Context {
    /// Underlying state store.
    store: StateStore,
    /// Operations performed in this context.
    operations: Vec<StateOp>,
    /// Witnesses collected for touched keys.
    witnesses: Vec<UpdateWitness>,
    /// Cache of read values.
    read_cache: HashMap<Vec<u8>, Option<Vec<u8>>>,
}

impl Context {
    /// Create a new context from a state store.
    pub fn new(store: StateStore) -> Self {
        Self {
            store,
            operations: Vec::new(),
            witnesses: Vec::new(),
            read_cache: HashMap::new(),
        }
    }

    /// Get the current state root.
    pub fn root(&self) -> Hash32 {
        self.store.root()
    }

    /// Read a raw value.
    pub fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self.store.get_raw(key)?)
    }

    /// Read a typed value.
    pub fn get<V: DeserializeOwned>(&self, key: &[u8]) -> Result<Option<V>> {
        Ok(self.store.get(key)?)
    }

    /// Read a typed value, returning error if not found.
    pub fn get_required<V: DeserializeOwned>(&self, key: &[u8]) -> Result<V> {
        self.get(key)?
            .ok_or_else(|| SdkError::KeyNotFound(String::from_utf8_lossy(key).to_string()))
    }

    /// Write a raw value.
    pub fn set_raw(&mut self, key: &[u8], value: Vec<u8>) -> Result<()> {
        let witness = self.store.insert_raw(key, value.clone())?;
        self.operations.push(StateOp::Insert {
            key: key.to_vec(),
            value,
        });
        self.witnesses.push(witness);
        Ok(())
    }

    /// Write a typed value.
    pub fn set<V: Serialize>(&mut self, key: &[u8], value: &V) -> Result<()> {
        let data = bincode::serialize(value).map_err(|e| SdkError::Serialization(e.to_string()))?;
        self.set_raw(key, data)
    }

    /// Delete a key.
    pub fn delete(&mut self, key: &[u8]) -> Result<()> {
        let witness = self.store.delete(key)?;
        self.operations.push(StateOp::Delete {
            key: key.to_vec(),
        });
        self.witnesses.push(witness);
        Ok(())
    }

    /// Check if a key exists.
    pub fn exists(&self, key: &[u8]) -> Result<bool> {
        Ok(self.get_raw(key)?.is_some())
    }

    /// Get all operations performed.
    pub fn operations(&self) -> &[StateOp] {
        &self.operations
    }

    /// Get all witnesses collected.
    pub fn witnesses(&self) -> &[UpdateWitness] {
        &self.witnesses
    }

    /// Take the witnesses (consumes them).
    pub fn take_witnesses(&mut self) -> Vec<UpdateWitness> {
        std::mem::take(&mut self.witnesses)
    }

    /// Commit the state changes.
    pub fn commit(&mut self) -> Result<Hash32> {
        Ok(self.store.commit()?)
    }

    /// Get a Merkle proof for a key.
    pub fn get_proof(&self, key: &[u8]) -> MerkleProof {
        self.store.get_proof(key)
    }

    /// Into inner store.
    pub fn into_store(self) -> StateStore {
        self.store
    }
}

/// Trait for ZK applications.
///
/// Implement this trait to define your application's business logic.
/// The SDK handles all ZK-related complexity (witnesses, proofs, Merkle trees).
pub trait Application {
    /// Public input type (visible to verifiers).
    type PublicInput: Serialize + DeserializeOwned;
    /// Private input type (hidden from verifiers).
    type PrivateInput: Serialize + DeserializeOwned;
    /// Output type (returned after execution).
    type Output: Serialize + DeserializeOwned;

    /// Apply a state transition.
    ///
    /// This is where you implement your business logic.
    /// Use `ctx` to read/write state.
    fn apply(
        &self,
        ctx: &mut Context,
        public: Self::PublicInput,
        private: Self::PrivateInput,
    ) -> Result<Self::Output>;
}

/// Helper for building typed keys.
pub struct KeyBuilder {
    prefix: Vec<u8>,
}

impl KeyBuilder {
    /// Create a new key builder with a prefix.
    pub fn new(prefix: impl AsRef<[u8]>) -> Self {
        Self {
            prefix: prefix.as_ref().to_vec(),
        }
    }

    /// Build a key with a suffix.
    pub fn key(&self, suffix: impl AsRef<[u8]>) -> Vec<u8> {
        let mut key = self.prefix.clone();
        key.push(b':');
        key.extend_from_slice(suffix.as_ref());
        key
    }
}

/// Account-based state helper.
pub mod accounts {
    use super::*;

    /// Account balance key builder.
    pub fn balance_key(account: &str) -> Vec<u8> {
        KeyBuilder::new("balance").key(account)
    }

    /// Account nonce key builder.
    pub fn nonce_key(account: &str) -> Vec<u8> {
        KeyBuilder::new("nonce").key(account)
    }

    /// Get balance from context.
    pub fn get_balance(ctx: &Context, account: &str) -> Result<u64> {
        ctx.get(&balance_key(account)).map(|v| v.unwrap_or(0))
    }

    /// Set balance in context.
    pub fn set_balance(ctx: &mut Context, account: &str, balance: u64) -> Result<()> {
        ctx.set(&balance_key(account), &balance)
    }

    /// Get nonce from context.
    pub fn get_nonce(ctx: &Context, account: &str) -> Result<u64> {
        ctx.get(&nonce_key(account)).map(|v| v.unwrap_or(0))
    }

    /// Increment and return nonce.
    pub fn increment_nonce(ctx: &mut Context, account: &str) -> Result<u64> {
        let nonce = get_nonce(ctx, account)?;
        let new_nonce = nonce + 1;
        ctx.set(&nonce_key(account), &new_nonce)?;
        Ok(new_nonce)
    }

    /// Transfer between accounts.
    pub fn transfer(
        ctx: &mut Context,
        from: &str,
        to: &str,
        amount: u64,
    ) -> Result<()> {
        let from_balance = get_balance(ctx, from)?;
        if from_balance < amount {
            return Err(SdkError::Application(format!(
                "insufficient balance: {} < {}",
                from_balance, amount
            )));
        }

        let to_balance = get_balance(ctx, to)?;

        set_balance(ctx, from, from_balance - amount)?;
        set_balance(ctx, to, to_balance + amount)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_basic_operations() {
        let store = StateStore::in_memory().unwrap();
        let mut ctx = Context::new(store);

        ctx.set_raw(b"key1", b"value1".to_vec()).unwrap();
        assert_eq!(ctx.get_raw(b"key1").unwrap(), Some(b"value1".to_vec()));

        ctx.delete(b"key1").unwrap();
        assert_eq!(ctx.get_raw(b"key1").unwrap(), None);
    }

    #[test]
    fn test_context_typed_values() {
        let store = StateStore::in_memory().unwrap();
        let mut ctx = Context::new(store);

        ctx.set(b"number", &42u64).unwrap();
        let loaded: u64 = ctx.get_required(b"number").unwrap();
        assert_eq!(loaded, 42);
    }

    #[test]
    fn test_accounts_transfer() {
        let store = StateStore::in_memory().unwrap();
        let mut ctx = Context::new(store);

        accounts::set_balance(&mut ctx, "alice", 100).unwrap();
        accounts::set_balance(&mut ctx, "bob", 50).unwrap();

        accounts::transfer(&mut ctx, "alice", "bob", 30).unwrap();

        assert_eq!(accounts::get_balance(&ctx, "alice").unwrap(), 70);
        assert_eq!(accounts::get_balance(&ctx, "bob").unwrap(), 80);
    }

    #[test]
    fn test_accounts_insufficient_balance() {
        let store = StateStore::in_memory().unwrap();
        let mut ctx = Context::new(store);

        accounts::set_balance(&mut ctx, "alice", 10).unwrap();

        let result = accounts::transfer(&mut ctx, "alice", "bob", 20);
        assert!(result.is_err());
    }

    #[test]
    fn test_witnesses_collected() {
        let store = StateStore::in_memory().unwrap();
        let mut ctx = Context::new(store);

        ctx.set_raw(b"key1", b"value1".to_vec()).unwrap();
        ctx.set_raw(b"key2", b"value2".to_vec()).unwrap();

        assert_eq!(ctx.witnesses().len(), 2);
    }
}
