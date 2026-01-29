//! Celestia DA adapter for submitting and retrieving blobs.
//!
//! This crate provides a client for interacting with Celestia's blob submission
//! and retrieval APIs through the celestia-node JSON-RPC interface.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info};

/// Default Celestia node RPC endpoint (bridge node).
pub const DEFAULT_RPC_URL: &str = "http://localhost:26658";

/// Errors that can occur when interacting with Celestia.
#[derive(Error, Debug)]
pub enum CelestiaError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON-RPC error: code={code}, message={message}")]
    JsonRpc { code: i64, message: String },
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("blob not found")]
    BlobNotFound,
    #[error("namespace error: {0}")]
    Namespace(String),
}

/// A Celestia namespace (29 bytes: 1 byte version + 28 bytes ID).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    /// Version byte (0 for user namespaces).
    pub version: u8,
    /// 28-byte namespace ID.
    pub id: [u8; 28],
}

impl Namespace {
    /// Create a namespace from a human-readable string (padded/hashed to 28 bytes).
    pub fn from_string(name: &str) -> Self {
        let mut id = [0u8; 28];
        let bytes = name.as_bytes();
        let len = bytes.len().min(28);
        // Right-pad with zeros (Celestia convention for user namespaces)
        id[28 - len..].copy_from_slice(&bytes[..len]);
        Self { version: 0, id }
    }

    /// Create a namespace from raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CelestiaError> {
        if bytes.len() != 29 {
            return Err(CelestiaError::Namespace(format!(
                "expected 29 bytes, got {}",
                bytes.len()
            )));
        }
        let version = bytes[0];
        let mut id = [0u8; 28];
        id.copy_from_slice(&bytes[1..]);
        Ok(Self { version, id })
    }

    /// Convert to 29-byte representation.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![self.version];
        bytes.extend_from_slice(&self.id);
        bytes
    }

    /// Convert to base64 for API calls.
    pub fn to_base64(&self) -> String {
        BASE64.encode(self.to_bytes())
    }
}

/// A blob submission result.
#[derive(Debug, Clone)]
pub struct SubmitResult {
    /// Height at which the blob was included.
    pub height: u64,
    /// Commitment of the blob.
    pub commitment: Vec<u8>,
}

/// A retrieved blob.
#[derive(Debug, Clone)]
pub struct RetrievedBlob {
    /// The blob data.
    pub data: Vec<u8>,
    /// The namespace.
    pub namespace: Vec<u8>,
    /// Share commitment.
    pub commitment: Vec<u8>,
    /// Index in the block.
    pub index: u32,
}

/// JSON-RPC request structure.
#[derive(Serialize)]
struct JsonRpcRequest<T> {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: T,
}

/// JSON-RPC response structure.
#[derive(Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

/// Blob API response.
#[derive(Deserialize)]
struct BlobResponse {
    namespace: String,
    data: String,
    #[serde(rename = "share_version")]
    _share_version: u8,
    commitment: String,
    index: u32,
}

/// Celestia DA client.
#[derive(Clone)]
pub struct CelestiaClient {
    client: reqwest::Client,
    rpc_url: String,
    request_id: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl CelestiaClient {
    /// Create a new client with the default RPC URL.
    pub fn new() -> Self {
        Self::with_url(DEFAULT_RPC_URL)
    }

    /// Create a new client with a custom RPC URL.
    pub fn with_url(url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            rpc_url: url.to_string(),
            request_id: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    fn next_id(&self) -> u64 {
        self.request_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    async fn call<P: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: P,
    ) -> Result<R, CelestiaError> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: self.next_id(),
            method: method.to_string(),
            params,
        };

        debug!("Calling {} on {}", method, self.rpc_url);

        let response = self
            .client
            .post(&self.rpc_url)
            .json(&request)
            .send()
            .await?;

        let json: JsonRpcResponse<R> = response.json().await?;

        if let Some(error) = json.error {
            return Err(CelestiaError::JsonRpc {
                code: error.code,
                message: error.message,
            });
        }

        json.result
            .ok_or_else(|| CelestiaError::InvalidResponse("no result in response".to_string()))
    }

    /// Submit a blob to Celestia under the given namespace.
    pub async fn submit_blob(
        &self,
        namespace: &Namespace,
        data: &[u8],
    ) -> Result<SubmitResult, CelestiaError> {
        let ns_base64 = namespace.to_base64();
        let data_base64 = BASE64.encode(data);

        info!(
            "Submitting blob: namespace={}, data_len={}",
            ns_base64,
            data.len()
        );

        // Create blob object for submission
        let blob = serde_json::json!([{
            "namespace": ns_base64,
            "data": data_base64,
            "share_version": 0,
            "commitment": null,
            "index": null
        }]);

        // blob.Submit takes [blobs, TxConfig]
        // Pass empty object for default gas settings
        let params = serde_json::json!([blob, {}]);

        let height: u64 = self.call("blob.Submit", params).await?;

        info!("Blob submitted at height {}", height);

        // Note: We don't fetch the blob back immediately because there's a delay
        // between submission and availability. The commitment can be retrieved
        // later if needed via get_blobs().
        Ok(SubmitResult {
            height,
            commitment: vec![],
        })
    }

    /// Get all blobs for a namespace at a specific height.
    pub async fn get_blobs(
        &self,
        namespace: &Namespace,
        height: u64,
    ) -> Result<Vec<RetrievedBlob>, CelestiaError> {
        let ns_base64 = namespace.to_base64();

        debug!("Getting blobs at height {} for namespace", height);

        let params = serde_json::json!([height, [ns_base64]]);

        let responses: Vec<BlobResponse> = match self.call("blob.GetAll", params).await {
            Ok(r) => r,
            Err(CelestiaError::JsonRpc { code: _, message })
                if message.contains("blob: not found") =>
            {
                return Ok(vec![]);
            }
            Err(e) => return Err(e),
        };

        let blobs = responses
            .into_iter()
            .map(|r| {
                let data = BASE64.decode(&r.data).unwrap_or_default();
                let namespace = BASE64.decode(&r.namespace).unwrap_or_default();
                let commitment = BASE64.decode(&r.commitment).unwrap_or_default();
                RetrievedBlob {
                    data,
                    namespace,
                    commitment,
                    index: r.index,
                }
            })
            .collect();

        Ok(blobs)
    }

    /// Get blobs for a namespace across a range of heights.
    pub async fn get_blobs_range(
        &self,
        namespace: &Namespace,
        from_height: u64,
        to_height: u64,
    ) -> Result<Vec<(u64, RetrievedBlob)>, CelestiaError> {
        let mut all_blobs = Vec::new();

        for height in from_height..=to_height {
            let blobs = self.get_blobs(namespace, height).await?;
            for blob in blobs {
                all_blobs.push((height, blob));
            }
        }

        Ok(all_blobs)
    }

    /// Get the current chain head height.
    pub async fn get_head_height(&self) -> Result<u64, CelestiaError> {
        #[derive(Deserialize)]
        struct Header {
            header: HeaderInner,
        }

        #[derive(Deserialize)]
        struct HeaderInner {
            height: String,
        }

        let params: [u8; 0] = [];
        let header: Header = self.call("header.LocalHead", params).await?;

        header
            .header
            .height
            .parse()
            .map_err(|_| CelestiaError::InvalidResponse("invalid height".to_string()))
    }

    /// Check if the node is ready.
    pub async fn is_ready(&self) -> Result<bool, CelestiaError> {
        let params: [u8; 0] = [];
        let ready: bool = self.call("node.Ready", params).await?;
        Ok(ready)
    }
}

impl Default for CelestiaClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_namespace_from_string() {
        let ns = Namespace::from_string("myapp");
        assert_eq!(ns.version, 0);
        // "myapp" is 5 bytes, should be right-padded
        assert_eq!(&ns.id[23..], b"myapp");
        assert_eq!(&ns.id[..23], &[0u8; 23]);
    }

    #[test]
    fn test_namespace_roundtrip() {
        let ns = Namespace::from_string("test-namespace");
        let bytes = ns.to_bytes();
        let ns2 = Namespace::from_bytes(&bytes).unwrap();
        assert_eq!(ns, ns2);
    }
}
