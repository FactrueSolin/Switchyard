// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Bearer-token admin API and the embedded web console.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use switchyard_server::ServerState;
use toml_edit::DocumentMut;

use crate::doc::{self, DocError};
use crate::probe;
use crate::secrets::SecretsStore;
use crate::store::ConfigStore;

const MAX_DECISIONS: usize = 200;

static WEB: Dir = include_dir!("$CARGO_MANIFEST_DIR/web");

pub struct AdminState {
    pub server: ServerState,
    pub store: Arc<ConfigStore>,
    pub secrets: Arc<SecretsStore>,
    pub token: String,
    /// JSONL routing log the data plane appends to, when enabled.
    pub routing_log: Option<PathBuf>,
    /// Serializes read-modify-write console mutations.
    mutation: tokio::sync::Mutex<()>,
}

impl AdminState {
    /// Builds the state from its components.
    pub fn new(
        server: ServerState,
        store: Arc<ConfigStore>,
        secrets: Arc<SecretsStore>,
        token: String,
        routing_log: Option<PathBuf>,
    ) -> Self {
        Self {
            server,
            store,
            secrets,
            token,
            routing_log,
            mutation: tokio::sync::Mutex::new(()),
        }
    }

    /// Holds the mutation lock for a read-modify-write apply cycle.
    async fn mutate<'a>(&'a self) -> tokio::sync::MutexGuard<'a, ()> {
        self.mutation.lock().await
    }
}

/// The console router, mounted under `/admin`.
pub fn admin_router(state: Arc<AdminState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/config", get(get_config).put(put_config))
        .route("/summary", get(get_summary))
        .route("/decision-models", post(add_decision_model))
        .route(
            "/decision-models/{name}",
            put(update_decision_model).delete(delete_decision_model),
        )
        .route("/decision-models/{name}/test", post(test_decision_model))
        .route(
            "/sections/{section}",
            get(list_section).post(add_section_entry),
        )
        .route(
            "/sections/{section}/{name}",
            get(get_section_entry)
                .put(put_section_entry)
                .delete(delete_section_entry),
        )
        .route("/secrets", get(get_secrets).put(put_secret))
        .route("/secrets/{name}", delete(delete_secret))
        .route("/history", get(get_history))
        .route("/history/{version}", get(get_history_entry))
        .route("/history/{version}/restore", post(restore_history))
        .route("/decisions", get(get_decisions))
        .layer(middleware::from_fn_with_state(state.clone(), require_token))
        .with_state(state)
}

/// The embedded web console for any non-`/admin` path.
pub async fn web_ui(request: Request) -> Response {
    let path = request.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match WEB.get_file(path) {
        Some(file) => {
            let mime = match path.rsplit_once('.').map(|(_, ext)| ext) {
                Some("html") => "text/html; charset=utf-8",
                Some("js") => "text/javascript; charset=utf-8",
                Some("css") => "text/css; charset=utf-8",
                Some("svg") => "image/svg+xml",
                _ => "application/octet-stream",
            };
            (
                [(axum::http::header::CONTENT_TYPE, mime.to_string())],
                file.contents(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }
}

impl From<DocError> for ApiError {
    fn from(error: DocError) -> Self {
        ApiError::bad_request(error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

async fn require_token(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let authorized = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|provided| provided == state.token);
    if authorized {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response()
    }
}

#[derive(Serialize)]
struct ConfigResponse {
    version: u64,
    source: String,
}

#[derive(Deserialize)]
struct SourceBody {
    source: String,
}

#[derive(Serialize)]
struct ApplyResponse {
    version: u64,
}

async fn health(State(state): State<Arc<AdminState>>) -> Json<Value> {
    let version = state.store.current().1;
    Json(json!({ "status": "ok", "version": version }))
}

async fn get_config(State(state): State<Arc<AdminState>>) -> Json<ConfigResponse> {
    let (source, version) = state.store.current();
    Json(ConfigResponse { version, source })
}

async fn put_config(
    State(state): State<Arc<AdminState>>,
    Json(body): Json<SourceBody>,
) -> Result<Json<ApplyResponse>, ApiError> {
    let report = state
        .store
        .apply(&state.server, &body.source)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(ApplyResponse {
        version: report.version,
    }))
}

async fn get_summary(State(state): State<Arc<AdminState>>) -> Result<Json<Value>, ApiError> {
    let (source, _) = state.store.current();
    let doc = doc::parse(&source)?;
    Ok(Json(doc::summary(&doc)))
}

fn doc_from_store(state: &AdminState) -> Result<DocumentMut, ApiError> {
    let (source, _) = state.store.current();
    doc::parse(&source).map_err(ApiError::from)
}

async fn apply_doc(
    state: &Arc<AdminState>,
    doc: &DocumentMut,
) -> Result<Json<ApplyResponse>, ApiError> {
    let source = doc.to_string();
    let report = state
        .store
        .apply(&state.server, &source)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(ApplyResponse {
        version: report.version,
    }))
}

/// Suggests an api_key_env for a new decision model from its name.
fn suggested_key_env(name: &str) -> String {
    format!("{}_API_KEY", name.to_ascii_uppercase().replace('-', "_"))
}

#[derive(Deserialize)]
struct DecisionModelBody {
    /// Name of the entry; required for `POST`, ignored for `PUT`.
    #[serde(default)]
    name: Option<String>,
    base_url: String,
    model: String,
    #[serde(default)]
    api_key_env: Option<String>,
}

async fn add_decision_model(
    State(state): State<Arc<AdminState>>,
    Json(body): Json<DecisionModelBody>,
) -> Result<Json<ApplyResponse>, ApiError> {
    let name = body
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("name is required"))?;
    let _guard = state.mutate().await;
    let mut doc = doc_from_store(&state)?;
    let api_key_env = body
        .api_key_env
        .clone()
        .filter(|key_env| !key_env.trim().is_empty())
        .unwrap_or_else(|| suggested_key_env(&name));
    doc::upsert_decision_model(&mut doc, &name, &body.base_url, &body.model, &api_key_env)?;
    apply_doc(&state, &doc).await
}

async fn update_decision_model(
    State(state): State<Arc<AdminState>>,
    Path(name): Path<String>,
    Json(body): Json<DecisionModelBody>,
) -> Result<Json<ApplyResponse>, ApiError> {
    let _guard = state.mutate().await;
    let mut doc = doc_from_store(&state)?;
    doc::decision_model_info(&doc, &name)
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    let api_key_env = body
        .api_key_env
        .clone()
        .filter(|key_env| !key_env.trim().is_empty())
        .unwrap_or_else(|| suggested_key_env(&name));
    doc::upsert_decision_model(&mut doc, &name, &body.base_url, &body.model, &api_key_env)?;
    apply_doc(&state, &doc).await
}

async fn delete_decision_model(
    State(state): State<Arc<AdminState>>,
    Path(name): Path<String>,
) -> Result<Json<ApplyResponse>, ApiError> {
    let _guard = state.mutate().await;
    let mut doc = doc_from_store(&state)?;
    let referencing = doc::referencing_routes(&doc, &name);
    if !referencing.is_empty() {
        return Err(ApiError::conflict(format!(
            "decision model {name} is used by routes: {}",
            referencing.join(", ")
        )));
    }
    doc::remove_decision_model(&mut doc, &name)?;
    apply_doc(&state, &doc).await
}

#[derive(Deserialize)]
struct TestBody {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    api_key_env: Option<String>,
}

/// Probes the saved decision model, or the body's values when given.
async fn test_decision_model(
    State(state): State<Arc<AdminState>>,
    Path(name): Path<String>,
    Json(body): Json<TestBody>,
) -> Result<Json<Value>, ApiError> {
    let (base_url, model, api_key_env) = if body.base_url.is_none() && body.model.is_none() {
        let doc = doc_from_store(&state)?;
        doc::decision_model_info(&doc, &name)
    } else {
        Ok((
            body.base_url.unwrap_or_default(),
            body.model.unwrap_or_default(),
            body.api_key_env.unwrap_or_default(),
        ))
    }?;
    if base_url.trim().is_empty() || model.trim().is_empty() || api_key_env.trim().is_empty() {
        return Err(ApiError::bad_request(
            "base_url, model, and api_key_env are all needed to test",
        ));
    }
    let api_key = state.secrets.resolve(&api_key_env).ok_or_else(|| {
        ApiError::bad_request(format!(
            "api key env {api_key_env} is not set; store it under Secrets first"
        ))
    })?;
    match probe::probe(&base_url, &model, Some(api_key)).await {
        Ok(outcome) => Ok(Json(json!({ "ok": true, "outcome": outcome }))),
        Err(message) => Ok(Json(json!({ "ok": false, "error": message }))),
    }
}

async fn list_section(
    State(state): State<Arc<AdminState>>,
    Path(section): Path<String>,
) -> Result<Json<Value>, ApiError> {
    doc::validate_section(&section)?;
    let doc = doc_from_store(&state)?;
    let entries = doc
        .get(&section)
        .and_then(toml_edit::Item::as_table)
        .map(|section| {
            section
                .iter()
                .map(|(name, _)| name.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(Json(json!({ "section": section, "entries": entries })))
}

#[derive(Serialize)]
struct SectionEntryResponse {
    section: String,
    name: String,
    block: String,
}

async fn get_section_entry(
    State(state): State<Arc<AdminState>>,
    Path((section, name)): Path<(String, String)>,
) -> Result<Json<SectionEntryResponse>, ApiError> {
    doc::validate_section(&section)?;
    let doc = doc_from_store(&state)?;
    let block = doc::section_block(&doc, &section, &name)
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    Ok(Json(SectionEntryResponse {
        section,
        name,
        block,
    }))
}

#[derive(Deserialize)]
struct SectionBlockBody {
    /// Name of the entry; required for `POST`, ignored for `PUT`.
    #[serde(default)]
    name: Option<String>,
    block: String,
}

async fn add_section_entry(
    State(state): State<Arc<AdminState>>,
    Path(section): Path<String>,
    Json(body): Json<SectionBlockBody>,
) -> Result<Json<ApplyResponse>, ApiError> {
    let name = body
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("name is required"))?;
    let _guard = state.mutate().await;
    let mut doc = doc_from_store(&state)?;
    doc::upsert_section(&mut doc, &section, &name, &body.block)?;
    apply_doc(&state, &doc).await
}

async fn put_section_entry(
    State(state): State<Arc<AdminState>>,
    Path((section, name)): Path<(String, String)>,
    Json(body): Json<SectionBlockBody>,
) -> Result<Json<ApplyResponse>, ApiError> {
    let _guard = state.mutate().await;
    let mut doc = doc_from_store(&state)?;
    doc::upsert_section(&mut doc, &section, &name, &body.block)?;
    apply_doc(&state, &doc).await
}

async fn delete_section_entry(
    State(state): State<Arc<AdminState>>,
    Path((section, name)): Path<(String, String)>,
) -> Result<Json<ApplyResponse>, ApiError> {
    let _guard = state.mutate().await;
    let mut doc = doc_from_store(&state)?;
    doc::remove_section_entry(&mut doc, &section, &name)?;
    apply_doc(&state, &doc).await
}

async fn get_secrets(State(state): State<Arc<AdminState>>) -> Json<Value> {
    Json(json!({ "names": state.secrets.list() }))
}

#[derive(Deserialize)]
struct SecretBody {
    name: String,
    value: String,
}

async fn put_secret(
    State(state): State<Arc<AdminState>>,
    Json(body): Json<SecretBody>,
) -> Result<Json<Value>, ApiError> {
    state
        .secrets
        .set(&body.name, &body.value)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    // Rebuild the running deployment so new keys reach new routes now.
    match state.store.reload(&state.server) {
        Ok(report) => Ok(Json(json!({
            "reloaded": true,
            "version": report.version,
        }))),
        Err(error) => Ok(Json(json!({
            "reloaded": false,
            "error": error.to_string(),
        }))),
    }
}

async fn delete_secret(
    State(state): State<Arc<AdminState>>,
    Path(name): Path<String>,
) -> Json<Value> {
    let removed = state.secrets.remove(&name);
    Json(json!({ "removed": removed }))
}

async fn get_history(State(state): State<Arc<AdminState>>) -> Result<Json<Value>, ApiError> {
    let entries = state
        .store
        .list_history()
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let version = state.store.current().1;
    Ok(Json(json!({ "version": version, "entries": entries })))
}

async fn get_history_entry(
    State(state): State<Arc<AdminState>>,
    Path(version): Path<u64>,
) -> Result<Json<Value>, ApiError> {
    let source = state.store.history_source(version).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ApiError::not_found(format!("history version {version} does not exist"))
        } else {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        }
    })?;
    Ok(Json(json!({ "version": version, "source": source })))
}

async fn restore_history(
    State(state): State<Arc<AdminState>>,
    Path(version): Path<u64>,
) -> Result<Json<ApplyResponse>, ApiError> {
    let _guard = state.mutate().await;
    let source = state.store.history_source(version).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ApiError::not_found(format!("history version {version} does not exist"))
        } else {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        }
    })?;
    let report = state
        .store
        .apply(&state.server, &source)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(ApplyResponse {
        version: report.version,
    }))
}

#[derive(Deserialize)]
struct DecisionsQuery {
    model: Option<String>,
    limit: Option<usize>,
}

/// Recent routing decisions from the data plane's routing log.
async fn get_decisions(
    State(state): State<Arc<AdminState>>,
    Query(query): Query<DecisionsQuery>,
) -> Result<Json<Value>, ApiError> {
    let Some(path) = state.routing_log.as_ref() else {
        return Ok(Json(json!({ "decisions": [], "routing_log": false })));
    };
    let source =
        fs::read_to_string(path).map_err(|_| ApiError::not_found("routing log is not readable"))?;
    let limit = query.limit.unwrap_or(50).min(MAX_DECISIONS);
    let mut decisions = Vec::new();
    // Newest last in the file; walk it backwards.
    for line in source.lines().rev() {
        if decisions.len() >= limit {
            break;
        }
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(model) = &query.model
            && record.get("route_id").and_then(Value::as_str) != Some(model.as_str())
        {
            continue;
        }
        decisions.push(json!({
            "ts": record.get("ts"),
            "route_id": record.get("route_id"),
            "algorithm": record.get("algorithm"),
            "model": record.get("model"),
            "tier": record.get("tier"),
            "session_id": record.get("session_id"),
            "choice": record.get("evidence_choice"),
            "probabilities": record.get("evidence_probabilities"),
            "total_tokens": record.get("total_tokens"),
        }));
    }
    Ok(Json(json!({ "decisions": decisions })))
}
