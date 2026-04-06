//! HTTP server — async job model for avatar generation.
//!
//! POST /v1/avatar/generate  → 202 Accepted + job_id
//! GET  /v1/avatar/jobs/:id  → poll for completion
//! GET  /health              → operator health
//! GET  /health/gpu          → GPU info
//! GET  /metrics             → prometheus

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use dashmap::DashMap;
use tokio::sync::watch;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use tangle_inference_core::server::{
    acquire_permit, billing_gate, error_response, gpu_health_handler, metrics_handler,
    settle_billing,
};
use tangle_inference_core::{AppState, CostModel, CostParams, PerSecondCostModel, RequestGuard};

use crate::avatar::{AvatarBackend, AvatarRequest, JobInfo, JobStatus};
use crate::config::OperatorConfig;

/// In-memory job registry for async polling.
type Jobs = Arc<DashMap<String, JobInfo>>;

/// Backend attached to AppState.
pub struct AvatarAppBackend {
    pub avatar: Arc<AvatarBackend>,
    pub config: Arc<OperatorConfig>,
    pub cost_model: Arc<PerSecondCostModel>,
    pub jobs: Jobs,
}

impl AvatarAppBackend {
    pub fn new(config: Arc<OperatorConfig>) -> Self {
        Self {
            cost_model: Arc::new(PerSecondCostModel {
                price_per_second: config.avatar.price_per_second,
            }),
            avatar: Arc::new(AvatarBackend::new(Arc::new(config.avatar.clone()))),
            jobs: Arc::new(DashMap::new()),
            config,
        }
    }
}

pub fn build_router(state: AppState) -> Router {
    let max_body = state.server_config.max_request_body_bytes;
    let timeout = state.server_config.stream_timeout_secs;
    Router::new()
        .route("/v1/avatar/generate", post(generate))
        .route("/v1/avatar/jobs/{job_id}", get(get_job))
        .route("/health", get(health))
        .route("/health/gpu", get(gpu_health_handler))
        .route("/metrics", get(metrics_handler))
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(max_body))
        .layer(TimeoutLayer::new(std::time::Duration::from_secs(timeout)))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

pub async fn start(
    state: AppState,
    mut shutdown_rx: watch::Receiver<bool>,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let bind = format!(
        "{}:{}",
        state.server_config.host, state.server_config.port
    );
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(bind = %bind, "Avatar HTTP server listening");

    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.wait_for(|&v| v).await;
            })
            .await
        {
            tracing::error!(error = %e, "HTTP server error");
        }
    });

    Ok(handle)
}

// ── Handlers ────────────────────────────────────────────────────────────

async fn generate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AvatarRequest>,
) -> Response {
    let backend = state
        .backend::<AvatarAppBackend>()
        .expect("AvatarAppBackend");

    // Validate duration
    let duration = req.duration_seconds.min(backend.avatar.max_duration());
    if duration == 0 {
        return error_response(
            StatusCode::BAD_REQUEST,
            "duration_seconds must be > 0".into(),
            "validation_error",
            "invalid_duration",
        );
    }

    // Concurrency gate
    let _permit = match acquire_permit(&state) {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    // Billing gate — estimate based on requested duration
    let estimated_cost = duration * backend.avatar.price_per_second();
    let (spend_auth, preauth) =
        match billing_gate(&state, &headers, None, estimated_cost).await {
            Ok(v) => v,
            Err(resp) => return resp,
        };

    let mut guard = RequestGuard::new("avatar");

    // Submit to backend
    let job_id = match backend.avatar.submit(&req).await {
        Ok(id) => id,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("backend submit failed: {e}"),
                "backend_error",
                "submit_failed",
            );
        }
    };

    // Register job
    let info = JobInfo {
        job_id: job_id.clone(),
        status: JobStatus::Processing,
        result: None,
        error: None,
    };
    backend.jobs.insert(job_id.clone(), info.clone());

    // Spawn background poller that settles billing on completion
    let avatar = backend.avatar.clone();
    let cost_model = backend.cost_model.clone();
    let jobs = backend.jobs.clone();
    let billing = state.billing.clone();
    let jid = job_id.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            match avatar.poll(&jid).await {
                Ok(info) => {
                    let done = matches!(info.status, JobStatus::Completed | JobStatus::Failed);
                    let actual_duration = info
                        .result
                        .as_ref()
                        .map(|r| r.duration_seconds)
                        .unwrap_or(0.0);

                    jobs.insert(jid.clone(), info.clone());

                    if done {
                        // Settle billing based on actual duration
                        if let Some(ref auth) = spend_auth {
                            let cost = cost_model.calculate_cost(&CostParams {
                                extra: std::collections::HashMap::from([(
                                    "centiseconds".into(),
                                    (actual_duration * 100.0) as u64,
                                )]),
                                ..Default::default()
                            });
                            if let Err(e) =
                                settle_billing(&billing, auth, preauth.unwrap_or(0), cost).await
                            {
                                tracing::error!(error = %e, "settlement failed");
                            }
                        }
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!(job_id = %jid, error = %e, "poll failed, retrying");
                }
            }
        }
    });

    guard.set_success();

    // Return 202 Accepted with job info
    (StatusCode::ACCEPTED, Json(info)).into_response()
}

async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Response {
    let backend = state
        .backend::<AvatarAppBackend>()
        .expect("AvatarAppBackend");

    match backend.jobs.get(&job_id) {
        Some(info) => Json(info.value().clone()).into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            format!("job {job_id} not found"),
            "not_found",
            "job_not_found",
        ),
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "avatar-inference",
    }))
}
