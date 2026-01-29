//! HTTP client for the App DA Node API.
//!
//! This module provides a client for interacting with the App DA Node
//! HTTP API without directly accessing the database.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use merkle::{Hash32, MerkleProof};
use serde::{Deserialize, Serialize};
use state::StateOp;
use transition_format::{OperationType, VerifiableOperation};

/// HTTP client for the App DA Node API.
#[derive(Clone)]
pub struct AppNodeClient {
    base_url: String,
    client: reqwest::Client,
}

impl AppNodeClient {
    /// Create a new client with the given base URL.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Check if the server is healthy.
    pub async fn health(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        let response = self.client.get(&url).send().await?;
        Ok(response.status().is_success())
    }

    /// Get the latest state root.
    pub async fn get_latest_root(&self) -> Result<RootInfo> {
        let url = format!("{}/root/latest", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("failed to get latest root")?;

        if !response.status().is_success() {
            anyhow::bail!("API error: {}", response.status());
        }

        let data: RootResponse = response.json().await?;
        Ok(RootInfo {
            root: hex::decode(&data.root)
                .context("invalid root hex")?
                .try_into()
                .unwrap(),
            transition_index: data.transition_index,
            celestia_height: data.celestia_height,
        })
    }

    /// Get a value by key.
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let key_str = String::from_utf8_lossy(key);
        let url = format!("{}/value?key={}", self.base_url, key_str);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            anyhow::bail!("API error: {}", response.status());
        }

        let data: ValueResponse = response.json().await?;
        if let Some(value_b64) = data.value {
            let value = BASE64.decode(&value_b64)?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    /// Get a value with its Merkle proof.
    pub async fn get_with_proof(&self, key: &[u8]) -> Result<(Option<Vec<u8>>, MerkleProof)> {
        let key_str = String::from_utf8_lossy(key);
        let url = format!("{}/value?key={}", self.base_url, key_str);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            anyhow::bail!("API error: {}", response.status());
        }

        let data: ValueResponse = response.json().await?;

        let value = if let Some(value_b64) = data.value {
            Some(BASE64.decode(&value_b64)?)
        } else {
            None
        };

        let proof = parse_merkle_proof(&data.proof)?;

        Ok((value, proof))
    }

    /// Get the current root.
    pub async fn root(&self) -> Result<Hash32> {
        let info = self.get_latest_root().await?;
        Ok(info.root)
    }

    /// Get the current transition index.
    pub async fn transition_index(&self) -> Result<u64> {
        let info = self.get_latest_root().await?;
        Ok(info.transition_index)
    }

    /// Get the root history.
    pub async fn root_history(&self) -> Result<Vec<(u64, Hash32, Option<u64>)>> {
        let url = format!("{}/history", self.base_url);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            anyhow::bail!("API error: {}", response.status());
        }

        let data: HistoryResponse = response.json().await?;
        let mut result = Vec::new();
        for entry in data.entries {
            let root = hex::decode(&entry.root)?
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid root length"))?;
            result.push((entry.sequence, root, entry.celestia_height));
        }
        Ok(result)
    }

    /// Apply a state transition.
    pub async fn apply_transition(
        &self,
        ops: Vec<StateOp>,
        public_inputs: Vec<u8>,
        private_inputs: Vec<u8>,
        verifiable_ops: Vec<VerifiableOperation>,
    ) -> Result<TransitionResult> {
        let url = format!("{}/transition", self.base_url);

        // Convert operations
        let operations: Vec<OperationRequest> = ops
            .into_iter()
            .map(|op| match op {
                StateOp::Insert { key, value } => OperationRequest {
                    op_type: "insert".to_string(),
                    key: String::from_utf8_lossy(&key).to_string(),
                    value: Some(BASE64.encode(&value)),
                    encoding: None,
                },
                StateOp::Delete { key } => OperationRequest {
                    op_type: "delete".to_string(),
                    key: String::from_utf8_lossy(&key).to_string(),
                    value: None,
                    encoding: None,
                },
            })
            .collect();

        // Convert verifiable operations
        let verifiable_operations: Vec<VerifiableOperationRequest> = verifiable_ops
            .into_iter()
            .map(|vop| {
                let op_type_json = match vop.op_type {
                    OperationType::Set => serde_json::json!("Set"),
                    OperationType::CreateAccount { initial_balance } => {
                        serde_json::json!({
                            "CreateAccount": {
                                "initial_balance": initial_balance
                            }
                        })
                    }
                    OperationType::Transfer { from, to, amount } => {
                        serde_json::json!({
                            "Transfer": {
                                "from": String::from_utf8_lossy(&from),
                                "to": String::from_utf8_lossy(&to),
                                "amount": amount
                            }
                        })
                    }
                    OperationType::Mint { amount } => {
                        serde_json::json!({
                            "Mint": {
                                "amount": amount
                            }
                        })
                    }
                    OperationType::Burn { amount } => {
                        serde_json::json!({
                            "Burn": {
                                "amount": amount
                            }
                        })
                    }
                };

                VerifiableOperationRequest {
                    op_type: op_type_json,
                    key: String::from_utf8_lossy(&vop.key).to_string(),
                    old_value: vop.old_value.map(|v| BASE64.encode(&v)),
                    new_value: vop.new_value.map(|v| BASE64.encode(&v)),
                    witness_index: vop.witness_index,
                }
            })
            .collect();

        let request = ApplyTransitionRequest {
            operations,
            public_inputs: Some(BASE64.encode(&public_inputs)),
            private_inputs: Some(BASE64.encode(&private_inputs)),
            verifiable_operations,
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("failed to send transition request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("API error {}: {}", status, error_text);
        }

        let data: ApplyTransitionResponse = response.json().await?;

        Ok(TransitionResult {
            sequence: data.sequence,
            prev_root: hex::decode(&data.prev_root)?
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid prev_root length"))?,
            new_root: hex::decode(&data.new_root)?
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid new_root length"))?,
            celestia_height: data.celestia_height,
        })
    }
}

// Request/Response types matching the API

#[derive(Serialize)]
struct ApplyTransitionRequest {
    operations: Vec<OperationRequest>,
    public_inputs: Option<String>,
    private_inputs: Option<String>,
    verifiable_operations: Vec<VerifiableOperationRequest>,
}

#[derive(Serialize)]
struct OperationRequest {
    #[serde(rename = "type")]
    op_type: String,
    key: String,
    value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding: Option<String>,
}

#[derive(Serialize)]
struct VerifiableOperationRequest {
    op_type: serde_json::Value,
    key: String,
    old_value: Option<String>,
    new_value: Option<String>,
    witness_index: usize,
}

#[derive(Deserialize)]
struct RootResponse {
    root: String,
    transition_index: u64,
    celestia_height: Option<u64>,
}

#[derive(Deserialize)]
struct ValueResponse {
    value: Option<String>,
    proof: MerkleProofResponse,
}

#[derive(Deserialize)]
struct MerkleProofResponse {
    key_hash: String,
    value: Option<String>,
    siblings: Vec<String>,
}

#[derive(Deserialize)]
struct ApplyTransitionResponse {
    sequence: u64,
    prev_root: String,
    new_root: String,
    celestia_height: Option<u64>,
}

#[derive(Deserialize)]
struct HistoryResponse {
    entries: Vec<HistoryEntry>,
}

#[derive(Deserialize)]
struct HistoryEntry {
    sequence: u64,
    root: String,
    celestia_height: Option<u64>,
}

// Public types

/// Information about the latest root.
#[derive(Debug, Clone)]
pub struct RootInfo {
    pub root: Hash32,
    pub transition_index: u64,
    pub celestia_height: Option<u64>,
}

/// Result of applying a transition.
#[derive(Debug)]
pub struct TransitionResult {
    pub sequence: u64,
    pub prev_root: Hash32,
    pub new_root: Hash32,
    pub celestia_height: Option<u64>,
}

// Helper functions

fn parse_merkle_proof(response: &MerkleProofResponse) -> Result<MerkleProof> {
    let key: Hash32 = hex::decode(&response.key_hash)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid key hash length"))?;

    let value = if let Some(v) = &response.value {
        Some(BASE64.decode(v)?)
    } else {
        None
    };

    let siblings: Result<Vec<Hash32>> = response
        .siblings
        .iter()
        .map(|s| {
            hex::decode(s)?
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid sibling hash length"))
        })
        .collect();

    Ok(MerkleProof {
        key,
        value,
        siblings: siblings?,
    })
}
