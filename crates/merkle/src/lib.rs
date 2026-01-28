//! Sparse Merkle Tree (SMT) implementation for state commitment.
//!
//! This crate provides a sparse Merkle tree with:
//! - Efficient proofs of inclusion/exclusion
//! - Update witnesses for ZK circuits
//! - Serializable state for persistence

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use thiserror::Error;

/// Tree depth (using smaller depth for efficiency).
pub const TREE_DEPTH: usize = 160;

/// A 32-byte hash value.
pub type Hash32 = [u8; 32];

/// The empty/default hash for an empty subtree.
pub const EMPTY_HASH: Hash32 = [0u8; 32];

/// Errors that can occur during tree operations.
#[derive(Error, Debug)]
pub enum MerkleError {
    #[error("invalid proof")]
    InvalidProof,
    #[error("key not found")]
    KeyNotFound,
    #[error("serialization error: {0}")]
    Serialization(String),
}

/// Hash two child nodes to produce parent hash.
pub fn hash_nodes(left: &Hash32, right: &Hash32) -> Hash32 {
    let mut hasher = Sha256::new();
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

/// Hash a leaf value.
pub fn hash_leaf(key: &Hash32, value: &[u8]) -> Hash32 {
    let mut hasher = Sha256::new();
    hasher.update([0x00]); // Leaf prefix
    hasher.update(key);
    hasher.update(value);
    hasher.finalize().into()
}

/// Hash a key to get its path in the tree (truncated to TREE_DEPTH bits).
pub fn hash_key(key: &[u8]) -> Hash32 {
    let mut hasher = Sha256::new();
    hasher.update(key);
    hasher.finalize().into()
}

/// Get the bit at a specific position in a hash (for tree traversal).
/// Position 0 is the most significant bit.
fn get_bit(hash: &Hash32, position: usize) -> bool {
    let byte_idx = position / 8;
    let bit_idx = 7 - (position % 8);
    (hash[byte_idx] >> bit_idx) & 1 == 1
}

/// A Merkle proof for inclusion/exclusion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MerkleProof {
    /// The key being proven.
    pub key: Hash32,
    /// The value (empty if proving non-membership).
    pub value: Option<Vec<u8>>,
    /// Sibling hashes from leaf to root.
    pub siblings: Vec<Hash32>,
}

impl MerkleProof {
    /// Verify this proof against a root.
    pub fn verify(&self, root: &Hash32) -> bool {
        self.compute_root() == *root
    }

    /// Get the computed root from this proof.
    pub fn compute_root(&self) -> Hash32 {
        let leaf_hash = match &self.value {
            Some(v) => hash_leaf(&self.key, v),
            None => EMPTY_HASH,
        };

        let mut current = leaf_hash;
        for (i, sibling) in self.siblings.iter().enumerate() {
            let depth = TREE_DEPTH - 1 - i;
            if get_bit(&self.key, depth) {
                current = hash_nodes(sibling, &current);
            } else {
                current = hash_nodes(&current, sibling);
            }
        }

        current
    }
}

/// Witness for updating a key in the tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWitness {
    /// The key being updated.
    pub key: Hash32,
    /// Old value (None if key didn't exist).
    pub old_value: Option<Vec<u8>>,
    /// New value (None if deleting).
    pub new_value: Option<Vec<u8>>,
    /// Sibling hashes from leaf to root.
    pub siblings: Vec<Hash32>,
}

impl UpdateWitness {
    /// Compute the old root from this witness.
    pub fn compute_old_root(&self) -> Hash32 {
        let leaf_hash = match &self.old_value {
            Some(v) => hash_leaf(&self.key, v),
            None => EMPTY_HASH,
        };

        let mut current = leaf_hash;
        for (i, sibling) in self.siblings.iter().enumerate() {
            let depth = TREE_DEPTH - 1 - i;
            if get_bit(&self.key, depth) {
                current = hash_nodes(sibling, &current);
            } else {
                current = hash_nodes(&current, sibling);
            }
        }

        current
    }

    /// Compute the new root from this witness.
    pub fn compute_new_root(&self) -> Hash32 {
        let leaf_hash = match &self.new_value {
            Some(v) => hash_leaf(&self.key, v),
            None => EMPTY_HASH,
        };

        let mut current = leaf_hash;
        for (i, sibling) in self.siblings.iter().enumerate() {
            let depth = TREE_DEPTH - 1 - i;
            if get_bit(&self.key, depth) {
                current = hash_nodes(sibling, &current);
            } else {
                current = hash_nodes(&current, sibling);
            }
        }

        current
    }
}

/// Sparse Merkle Tree using a simple key-value store approach.
///
/// This implementation stores leaf values directly and computes
/// internal nodes on-the-fly for proofs.
#[derive(Debug, Clone)]
pub struct SparseMerkleTree {
    /// Leaf values indexed by key hash.
    leaves: HashMap<Hash32, Vec<u8>>,
    /// Cached root (recomputed on modification).
    cached_root: Option<Hash32>,
    /// Precomputed empty subtree hashes for each depth.
    empty_hashes: Vec<Hash32>,
}

impl Default for SparseMerkleTree {
    fn default() -> Self {
        Self::new()
    }
}

impl SparseMerkleTree {
    /// Create a new empty tree.
    pub fn new() -> Self {
        // Precompute empty hashes for each level
        let mut empty_hashes = vec![EMPTY_HASH; TREE_DEPTH + 1];
        for i in (0..TREE_DEPTH).rev() {
            empty_hashes[i] = hash_nodes(&empty_hashes[i + 1], &empty_hashes[i + 1]);
        }

        Self {
            leaves: HashMap::new(),
            cached_root: Some(empty_hashes[0]),
            empty_hashes,
        }
    }

    /// Get the current root hash.
    pub fn root(&self) -> Hash32 {
        self.cached_root.unwrap_or_else(|| self.compute_root())
    }

    /// Compute the root from all leaves.
    fn compute_root(&self) -> Hash32 {
        if self.leaves.is_empty() {
            return self.empty_hashes[0];
        }

        // Build subtree hashes level by level
        let mut level: HashMap<Vec<bool>, Hash32> = self
            .leaves
            .iter()
            .map(|(key, value)| {
                let path: Vec<bool> = (0..TREE_DEPTH).map(|i| get_bit(key, i)).collect();
                let leaf_hash = hash_leaf(key, value);
                (path, leaf_hash)
            })
            .collect();

        // Work up from leaves to root
        for depth in (0..TREE_DEPTH).rev() {
            let mut next_level: HashMap<Vec<bool>, Hash32> = HashMap::new();

            // Group by parent path
            let mut parents: HashMap<Vec<bool>, (Option<Hash32>, Option<Hash32>)> = HashMap::new();

            for (path, hash) in level {
                let parent_path: Vec<bool> = path[..depth].to_vec();
                let is_right = path[depth];

                let entry = parents.entry(parent_path).or_insert((None, None));
                if is_right {
                    entry.1 = Some(hash);
                } else {
                    entry.0 = Some(hash);
                }
            }

            for (parent_path, (left, right)) in parents {
                let left_hash = left.unwrap_or(self.empty_hashes[depth + 1]);
                let right_hash = right.unwrap_or(self.empty_hashes[depth + 1]);
                let parent_hash = hash_nodes(&left_hash, &right_hash);
                next_level.insert(parent_path, parent_hash);
            }

            level = next_level;
        }

        level.into_values().next().unwrap_or(self.empty_hashes[0])
    }

    /// Get a value by key.
    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let key_hash = hash_key(key);
        self.leaves.get(&key_hash).cloned()
    }

    /// Get a value by key hash.
    pub fn get_by_hash(&self, key_hash: &Hash32) -> Option<Vec<u8>> {
        self.leaves.get(key_hash).cloned()
    }

    /// Insert or update a key-value pair.
    pub fn insert(&mut self, key: &[u8], value: Vec<u8>) -> UpdateWitness {
        let key_hash = hash_key(key);
        self.insert_by_hash(key_hash, value)
    }

    /// Insert or update by key hash.
    pub fn insert_by_hash(&mut self, key_hash: Hash32, value: Vec<u8>) -> UpdateWitness {
        let old_value = self.leaves.get(&key_hash).cloned();
        let siblings = self.compute_siblings(&key_hash);

        let witness = UpdateWitness {
            key: key_hash,
            old_value,
            new_value: Some(value.clone()),
            siblings,
        };

        self.leaves.insert(key_hash, value);
        self.cached_root = None; // Invalidate cache

        witness
    }

    /// Delete a key.
    pub fn delete(&mut self, key: &[u8]) -> UpdateWitness {
        let key_hash = hash_key(key);
        self.delete_by_hash(key_hash)
    }

    /// Delete by key hash.
    pub fn delete_by_hash(&mut self, key_hash: Hash32) -> UpdateWitness {
        let old_value = self.leaves.get(&key_hash).cloned();
        let siblings = self.compute_siblings(&key_hash);

        let witness = UpdateWitness {
            key: key_hash,
            old_value,
            new_value: None,
            siblings,
        };

        self.leaves.remove(&key_hash);
        self.cached_root = None;

        witness
    }

    /// Get a Merkle proof for a key.
    pub fn get_proof(&self, key: &[u8]) -> MerkleProof {
        let key_hash = hash_key(key);
        self.get_proof_by_hash(&key_hash)
    }

    /// Get a Merkle proof by key hash.
    pub fn get_proof_by_hash(&self, key_hash: &Hash32) -> MerkleProof {
        let value = self.leaves.get(key_hash).cloned();
        let siblings = self.compute_siblings(key_hash);

        MerkleProof {
            key: *key_hash,
            value,
            siblings,
        }
    }

    /// Compute siblings for a key's path.
    fn compute_siblings(&self, key_hash: &Hash32) -> Vec<Hash32> {
        let target_path: Vec<bool> = (0..TREE_DEPTH).map(|i| get_bit(key_hash, i)).collect();

        // Build all leaf hashes with their paths
        let leaf_entries: Vec<(Vec<bool>, Hash32)> = self
            .leaves
            .iter()
            .filter(|(k, _)| *k != key_hash) // Exclude the target key
            .map(|(k, v)| {
                let path: Vec<bool> = (0..TREE_DEPTH).map(|i| get_bit(k, i)).collect();
                let hash = hash_leaf(k, v);
                (path, hash)
            })
            .collect();

        let mut siblings = Vec::with_capacity(TREE_DEPTH);

        // For each depth from bottom to top, compute the sibling hash
        for depth in (0..TREE_DEPTH).rev() {
            let target_prefix = &target_path[..depth];
            let target_bit = target_path[depth];

            // Find all leaves that share the prefix but differ at this bit
            let sibling_hash = self.compute_subtree_hash(
                &leaf_entries,
                target_prefix,
                !target_bit,
                depth,
            );

            siblings.push(sibling_hash);
        }

        siblings
    }

    /// Compute the hash of a subtree rooted at a given path prefix and direction.
    fn compute_subtree_hash(
        &self,
        leaves: &[(Vec<bool>, Hash32)],
        prefix: &[bool],
        direction: bool,
        depth: usize,
    ) -> Hash32 {
        // Filter leaves that start with prefix + direction
        let matching: Vec<&(Vec<bool>, Hash32)> = leaves
            .iter()
            .filter(|(path, _)| {
                path.len() > depth
                    && path[..prefix.len()] == *prefix
                    && path[prefix.len()] == direction
            })
            .collect();

        if matching.is_empty() {
            return self.empty_hashes[depth + 1];
        }

        // Recursively compute subtree hash
        self.compute_subtree_from_leaves(&matching, depth + 1)
    }

    /// Recursively compute subtree hash from filtered leaves.
    fn compute_subtree_from_leaves(
        &self,
        leaves: &[&(Vec<bool>, Hash32)],
        depth: usize,
    ) -> Hash32 {
        if depth == TREE_DEPTH {
            // At leaf level
            return leaves.first().map(|(_, h)| *h).unwrap_or(EMPTY_HASH);
        }

        if leaves.is_empty() {
            return self.empty_hashes[depth];
        }

        // Split into left and right subtrees
        let (left_leaves, right_leaves): (Vec<_>, Vec<_>) = leaves
            .iter()
            .partition(|(path, _)| !path[depth]);

        let left_hash = if left_leaves.is_empty() {
            self.empty_hashes[depth + 1]
        } else {
            self.compute_subtree_from_leaves(&left_leaves, depth + 1)
        };

        let right_hash = if right_leaves.is_empty() {
            self.empty_hashes[depth + 1]
        } else {
            self.compute_subtree_from_leaves(&right_leaves, depth + 1)
        };

        hash_nodes(&left_hash, &right_hash)
    }

    /// Serialize the tree for persistence.
    pub fn serialize(&self) -> Result<Vec<u8>, MerkleError> {
        #[derive(Serialize)]
        struct SerializedTree {
            leaves: Vec<(Hash32, Vec<u8>)>,
        }

        let tree = SerializedTree {
            leaves: self.leaves.iter().map(|(k, v)| (*k, v.clone())).collect(),
        };

        bincode::serialize(&tree).map_err(|e| MerkleError::Serialization(e.to_string()))
    }

    /// Deserialize a tree.
    pub fn deserialize(data: &[u8]) -> Result<Self, MerkleError> {
        #[derive(Deserialize)]
        struct SerializedTree {
            leaves: Vec<(Hash32, Vec<u8>)>,
        }

        let tree: SerializedTree =
            bincode::deserialize(data).map_err(|e| MerkleError::Serialization(e.to_string()))?;

        let mut smt = Self::new();
        smt.leaves = tree.leaves.into_iter().collect();
        smt.cached_root = None;
        Ok(smt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tree() {
        let tree = SparseMerkleTree::new();
        assert_eq!(tree.get(b"key"), None);
    }

    #[test]
    fn test_insert_and_get() {
        let mut tree = SparseMerkleTree::new();
        tree.insert(b"key1", b"value1".to_vec());
        tree.insert(b"key2", b"value2".to_vec());

        assert_eq!(tree.get(b"key1"), Some(b"value1".to_vec()));
        assert_eq!(tree.get(b"key2"), Some(b"value2".to_vec()));
        assert_eq!(tree.get(b"key3"), None);
    }

    #[test]
    fn test_update() {
        let mut tree = SparseMerkleTree::new();
        tree.insert(b"key", b"value1".to_vec());
        let root1 = tree.root();

        tree.insert(b"key", b"value2".to_vec());
        let root2 = tree.root();

        assert_ne!(root1, root2);
        assert_eq!(tree.get(b"key"), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_delete() {
        let mut tree = SparseMerkleTree::new();
        let empty_root = tree.root();

        tree.insert(b"key", b"value".to_vec());
        assert_eq!(tree.get(b"key"), Some(b"value".to_vec()));

        tree.delete(b"key");
        assert_eq!(tree.get(b"key"), None);
        assert_eq!(tree.root(), empty_root);
    }

    #[test]
    fn test_proof_verification() {
        let mut tree = SparseMerkleTree::new();
        tree.insert(b"key1", b"value1".to_vec());
        tree.insert(b"key2", b"value2".to_vec());

        let root = tree.root();

        let proof1 = tree.get_proof(b"key1");
        assert!(proof1.verify(&root));
        assert_eq!(proof1.value, Some(b"value1".to_vec()));

        let proof2 = tree.get_proof(b"key2");
        assert!(proof2.verify(&root));

        // Non-membership proof
        let proof3 = tree.get_proof(b"key3");
        assert!(proof3.verify(&root));
        assert_eq!(proof3.value, None);
    }

    #[test]
    fn test_update_witness() {
        let mut tree = SparseMerkleTree::new();
        let root0 = tree.root();

        let witness = tree.insert(b"key", b"value".to_vec());
        let root1 = tree.root();

        assert_eq!(witness.compute_old_root(), root0);
        assert_eq!(witness.compute_new_root(), root1);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut tree = SparseMerkleTree::new();
        tree.insert(b"key1", b"value1".to_vec());
        tree.insert(b"key2", b"value2".to_vec());

        let data = tree.serialize().unwrap();
        let tree2 = SparseMerkleTree::deserialize(&data).unwrap();

        assert_eq!(tree.root(), tree2.root());
        assert_eq!(tree2.get(b"key1"), Some(b"value1".to_vec()));
        assert_eq!(tree2.get(b"key2"), Some(b"value2".to_vec()));
    }
}
