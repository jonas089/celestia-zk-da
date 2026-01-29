//! HTTP API for the app node.

use crate::node::AppNodeState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use blob_schema::TransitionBlobV1;
use merkle::MerkleProof;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

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
