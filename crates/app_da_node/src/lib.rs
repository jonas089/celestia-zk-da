//! Application-specific DA node.
//!
//! This crate provides the core functionality for an app-specific DA node that:
//! - Stores application state with Merkle commitment
//! - Applies state transitions and generates ZK proofs
//! - Posts proofs to Celestia DA
//! - Serves state queries with Merkle proofs
//! - Verifies proofs from Celestia for syncing

pub mod api;
pub mod client;
pub mod node;
pub mod sync;

pub use client::{AppNodeClient, RootInfo, TransitionResult as ClientTransitionResult};
pub use node::{AppNode, AppNodeConfig};
