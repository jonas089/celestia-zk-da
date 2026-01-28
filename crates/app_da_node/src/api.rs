//! HTTP API for the app node.

use crate::node::{AppNode, AppNodeState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
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
    let celestia_height = state
        .root_history
        .last()
        .and_then(|(_, h)| *h);

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
    let (value, proof) = state
        .store
        .get_with_proof(&key)
        .map_err(|e| {
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
