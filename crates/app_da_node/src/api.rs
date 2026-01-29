//! HTTP API for the app node.

use crate::node::AppNodeState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use blob_schema::TransitionBlobV1;
use merkle::MerkleProof;
use serde::{Deserialize, Serialize};
use state::StateOp;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use transition_format::{OperationType, VerifiableOperation};

/// API state type.
type ApiState = Arc<RwLock<AppNodeState>>;

/// Create the API router.
pub fn create_router(state: Arc<RwLock<AppNodeState>>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health", get(health))
        .route("/root/latest", get(get_latest_root))
        .route("/value", get(get_value))
        .route("/proof/merkle", get(get_merkle_proof))
        .route("/sync/status", get(get_sync_status))
        .route("/history", get(get_history))
        .route("/celestia/transition", get(get_celestia_transition))
        .route("/celestia/transitions", get(get_celestia_transitions))
        .route("/transition", post(apply_transition))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

// Response types

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
struct RootResponse {
    root: String,
    transition_index: u64,
    celestia_height: Option<u64>,
}

#[derive(Serialize)]
struct ValueResponse {
    key: String,
    value: Option<String>,
    root: String,
    proof: MerkleProofResponse,
}

#[derive(Serialize)]
struct MerkleProofResponse {
    key_hash: String,
    value: Option<String>,
    siblings: Vec<String>,
}

impl From<MerkleProof> for MerkleProofResponse {
    fn from(proof: MerkleProof) -> Self {
        Self {
            key_hash: hex::encode(proof.key),
            value: proof.value.map(|v| BASE64.encode(&v)),
            siblings: proof.siblings.iter().map(hex::encode).collect(),
        }
    }
}

#[derive(Serialize)]
struct SyncStatusResponse {
    transition_index: u64,
    latest_root: String,
    celestia_enabled: bool,
    last_celestia_height: Option<u64>,
}

#[derive(Serialize)]
struct HistoryEntry {
    sequence: u64,
    root: String,
    celestia_height: Option<u64>,
}

#[derive(Serialize)]
struct HistoryResponse {
    entries: Vec<HistoryEntry>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct TransitionResponse {
    sequence: u64,
    prev_root: String,
    new_root: String,
    public_inputs: String,
    proof: String,
    proof_size_bytes: usize,
    program_hash: String,
    celestia_height: u64,
}

impl TransitionResponse {
    fn from_blob(blob: &TransitionBlobV1, height: u64) -> Self {
        Self {
            sequence: blob.sequence,
            prev_root: hex::encode(blob.prev_root),
            new_root: hex::encode(blob.new_root),
            public_inputs: BASE64.encode(&blob.public_inputs),
            proof: BASE64.encode(&blob.proof),
            proof_size_bytes: blob.proof.len(),
            program_hash: hex::encode(blob.program_hash),
            celestia_height: height,
        }
    }
}

#[derive(Serialize)]
struct TransitionsResponse {
    transitions: Vec<TransitionResponse>,
}

#[derive(Deserialize)]
struct ApplyTransitionRequest {
    operations: Vec<OperationRequest>,
    public_inputs: Option<String>, // Base64 encoded
    private_inputs: Option<String>, // Base64 encoded
    verifiable_operations: Vec<VerifiableOperationRequest>,
}

#[derive(Deserialize)]
struct OperationRequest {
    #[serde(rename = "type")]
    op_type: String, // "insert" or "delete"
    key: String,     // UTF-8 or hex based on encoding
    value: Option<String>, // Base64 encoded
    #[serde(default)]
    encoding: Option<String>, // "hex" or "utf8" (default)
}

#[derive(Deserialize)]
struct VerifiableOperationRequest {
    op_type: serde_json::Value,
    key: String,
    old_value: Option<String>, // Base64 encoded
    new_value: Option<String>, // Base64 encoded
    witness_index: usize,
}

#[derive(Serialize)]
struct ApplyTransitionResponse {
    sequence: u64,
    prev_root: String,
    new_root: String,
    celestia_height: Option<u64>,
    proof_size_bytes: usize,
}

// Query parameters

#[derive(Deserialize)]
struct ValueQuery {
    key: String,
    #[serde(default)]
    encoding: Option<String>, // "hex" or "utf8" (default)
}

#[derive(Deserialize)]
struct ProofQuery {
    key: String,
    #[serde(default)]
    encoding: Option<String>,
}

#[derive(Deserialize)]
struct CelestiaTransitionQuery {
    height: u64,
}

#[derive(Deserialize)]
struct CelestiaTransitionsQuery {
    from_height: u64,
    to_height: u64,
}

// Handlers

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn get_latest_root(State(state): State<ApiState>) -> Json<RootResponse> {
    let state = state.read().await;
    let root = state.store.root();
    let transition_index = state.store.transition_index();
    let celestia_height = state.root_history.last().and_then(|(_, h)| *h);

    Json(RootResponse {
        root: hex::encode(root),
        transition_index,
        celestia_height,
    })
}

async fn get_value(
    State(state): State<ApiState>,
    Query(query): Query<ValueQuery>,
) -> Result<Json<ValueResponse>, (StatusCode, Json<ErrorResponse>)> {
    let key = decode_key(&query.key, query.encoding.as_deref())?;

    let state = state.read().await;
    let (value, proof) = state.store.get_with_proof(&key).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

    let root = state.store.root();

    Ok(Json(ValueResponse {
        key: query.key,
        value: value.map(|v| BASE64.encode(&v)),
        root: hex::encode(root),
        proof: proof.into(),
    }))
}

async fn get_merkle_proof(
    State(state): State<ApiState>,
    Query(query): Query<ProofQuery>,
) -> Result<Json<MerkleProofResponse>, (StatusCode, Json<ErrorResponse>)> {
    let key = decode_key(&query.key, query.encoding.as_deref())?;

    let state = state.read().await;
    let proof = state.store.get_proof(&key);

    Ok(Json(proof.into()))
}

async fn get_sync_status(State(state): State<ApiState>) -> Json<SyncStatusResponse> {
    let state = state.read().await;

    Json(SyncStatusResponse {
        transition_index: state.store.transition_index(),
        latest_root: hex::encode(state.store.root()),
        celestia_enabled: state.config.celestia_enabled,
        last_celestia_height: state.root_history.last().and_then(|(_, h)| *h),
    })
}

async fn get_history(State(state): State<ApiState>) -> Json<HistoryResponse> {
    let state = state.read().await;

    let entries = state
        .root_history
        .iter()
        .enumerate()
        .map(|(i, (root, height))| HistoryEntry {
            sequence: i as u64,
            root: hex::encode(root),
            celestia_height: *height,
        })
        .collect();

    Json(HistoryResponse { entries })
}

async fn get_celestia_transition(
    State(state): State<ApiState>,
    Query(query): Query<CelestiaTransitionQuery>,
) -> Result<Json<TransitionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let state = state.read().await;

    let blobs = state
        .celestia
        .get_blobs(&state.config.namespace, query.height)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: format!("Celestia error: {}", e),
                }),
            )
        })?;

    let blob = blobs.first().ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("No transition found at height {}", query.height),
            }),
        )
    })?;

    let transition = TransitionBlobV1::decode(&blob.data).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to decode transition: {}", e),
            }),
        )
    })?;

    Ok(Json(TransitionResponse::from_blob(&transition, query.height)))
}

async fn get_celestia_transitions(
    State(state): State<ApiState>,
    Query(query): Query<CelestiaTransitionsQuery>,
) -> Result<Json<TransitionsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let state = state.read().await;

    let blobs = state
        .celestia
        .get_blobs_range(&state.config.namespace, query.from_height, query.to_height)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: format!("Celestia error: {}", e),
                }),
            )
        })?;

    let mut transitions = Vec::new();
    for (height, blob) in blobs {
        if let Ok(transition) = TransitionBlobV1::decode(&blob.data) {
            transitions.push(TransitionResponse::from_blob(&transition, height));
        }
    }

    Ok(Json(TransitionsResponse { transitions }))
}

async fn apply_transition(
    State(state): State<ApiState>,
    Json(request): Json<ApplyTransitionRequest>,
) -> Result<Json<ApplyTransitionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut state = state.write().await;

    // Convert operations
    let mut ops = Vec::new();
    for op in request.operations {
        let key = decode_key(&op.key, op.encoding.as_deref())?;
        let state_op = match op.op_type.as_str() {
            "delete" => StateOp::Delete { key },
            "insert" | _ => {
                let value = if let Some(v) = op.value {
                    BASE64.decode(&v).map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(ErrorResponse {
                                error: format!("invalid base64 value: {}", e),
                            }),
                        )
                    })?
                } else {
                    Vec::new()
                };
                StateOp::Insert { key, value }
            }
        };
        ops.push(state_op);
    }

    // Decode inputs
    let public_inputs = if let Some(pi) = request.public_inputs {
        BASE64.decode(&pi).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("invalid base64 public_inputs: {}", e),
                }),
            )
        })?
    } else {
        Vec::new()
    };

    let private_inputs = if let Some(pi) = request.private_inputs {
        BASE64.decode(&pi).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("invalid base64 private_inputs: {}", e),
                }),
            )
        })?
    } else {
        Vec::new()
    };

    // Convert verifiable operations
    let mut verifiable_ops = Vec::new();
    for vop in request.verifiable_operations {
        let key = decode_key(&vop.key, None)?;
        let old_value = vop
            .old_value
            .map(|v| BASE64.decode(&v))
            .transpose()
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: format!("invalid base64 old_value: {}", e),
                    }),
                )
            })?;
        let new_value = vop
            .new_value
            .map(|v| BASE64.decode(&v))
            .transpose()
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: format!("invalid base64 new_value: {}", e),
                    }),
                )
            })?;

        // Parse operation type from JSON
        let op_type = parse_operation_type(&vop.op_type).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("invalid operation type: {}", e),
                }),
            )
        })?;

        verifiable_ops.push(VerifiableOperation {
            op_type,
            key,
            old_value,
            new_value,
            witness_index: vop.witness_index,
        });
    }

    // Apply transition using the app node logic
    let prev_root = state.store.root();
    let sequence = state.store.transition_index() + 1;

    // Apply operations and collect witnesses
    let witnesses = state.store.apply_batch(ops).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("failed to apply operations: {}", e),
            }),
        )
    })?;

    // Commit the state changes
    let new_root = state.store.commit().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("failed to commit state: {}", e),
            }),
        )
    })?;

    // Build transition input with operations for business logic verification
    let input = transition_format::TransitionInput::new(
        prev_root,
        public_inputs.clone(),
        private_inputs,
        witnesses,
    )
    .with_operations(verifiable_ops);

    // Generate proof (or just execute for testing)
    let (proof_bytes, output) = if state.config.proving_enabled {
        let result = state.prover.prove(&input).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("proof generation failed: {}", e),
                }),
            )
        })?;
        (result.proof_bytes, result.output)
    } else {
        let output = state.prover.execute(&input).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("execution failed: {}", e),
                }),
            )
        })?;
        (Vec::new(), output)
    };

    // Verify the output matches our computation
    if output.prev_root != prev_root || output.new_root != new_root {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "proof output mismatch".to_string(),
            }),
        ));
    }

    // Create blob
    let blob = blob_schema::TransitionBlobV1::new(
        state.config.app_id.clone(),
        sequence,
        prev_root,
        new_root,
        public_inputs,
        proof_bytes.clone(),
        zk_host_harness::program_hash(),
    )
    .with_timestamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    );

    // Post to Celestia if enabled
    let celestia_result = if state.config.celestia_enabled {
        let blob_bytes = blob.encode().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("blob encoding failed: {}", e),
                }),
            )
        })?;
        match state
            .celestia
            .submit_blob(&state.config.namespace, &blob_bytes)
            .await
        {
            Ok(result) => Some(result),
            Err(e) => {
                tracing::warn!("Failed to post to Celestia: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Update root history
    state
        .root_history
        .push((new_root, celestia_result.as_ref().map(|r| r.height)));

    Ok(Json(ApplyTransitionResponse {
        sequence,
        prev_root: hex::encode(prev_root),
        new_root: hex::encode(new_root),
        celestia_height: celestia_result.as_ref().map(|r| r.height),
        proof_size_bytes: proof_bytes.len(),
    }))
}

// Helper functions

fn decode_key(
    key: &str,
    encoding: Option<&str>,
) -> Result<Vec<u8>, (StatusCode, Json<ErrorResponse>)> {
    match encoding {
        Some("hex") => hex::decode(key).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("invalid hex key: {}", e),
                }),
            )
        }),
        _ => Ok(key.as_bytes().to_vec()),
    }
}

fn parse_operation_type(value: &serde_json::Value) -> Result<OperationType, String> {
    // Handle simple string variant
    if let Some(s) = value.as_str() {
        if s == "Set" {
            return Ok(OperationType::Set);
        }
    }

    // Handle object variants
    let obj = value.as_object().ok_or("operation type must be an object or string")?;

    if let Some(create_account) = obj.get("CreateAccount") {
        let initial_balance = create_account
            .get("initial_balance")
            .and_then(|v| v.as_u64())
            .ok_or("CreateAccount must have initial_balance")?;
        Ok(OperationType::CreateAccount { initial_balance })
    } else if let Some(transfer) = obj.get("Transfer") {
        let from = transfer
            .get("from")
            .and_then(|v| v.as_str())
            .ok_or("Transfer must have from")?
            .as_bytes()
            .to_vec();
        let to = transfer
            .get("to")
            .and_then(|v| v.as_str())
            .ok_or("Transfer must have to")?
            .as_bytes()
            .to_vec();
        let amount = transfer
            .get("amount")
            .and_then(|v| v.as_u64())
            .ok_or("Transfer must have amount")?;
        Ok(OperationType::Transfer { from, to, amount })
    } else if let Some(mint) = obj.get("Mint") {
        let amount = mint
            .get("amount")
            .and_then(|v| v.as_u64())
            .ok_or("Mint must have amount")?;
        Ok(OperationType::Mint { amount })
    } else if let Some(burn) = obj.get("Burn") {
        let amount = burn
            .get("amount")
            .and_then(|v| v.as_u64())
            .ok_or("Burn must have amount")?;
        Ok(OperationType::Burn { amount })
    } else {
        Err("unknown operation type".to_string())
    }
}
