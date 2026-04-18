//! HTTP server — async job model for avatar generation.
//!
//! POST /v1/avatar/generate  → 202 Accepted + job_id
//! GET  /v1/avatar/jobs/:id  → poll for completion
//! GET  /health              → operator health
//! GET  /health/gpu          → GPU info
//! GET  /metrics             → prometheus

use std::sync::Arc;

use axum::extract::{Path, State};
use serde::Deserialize;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use dashmap::DashMap;
use tokio::sync::watch;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use tangle_inference_core::server::{
    acquire_permit, billing_gate, error_response, gpu_health_handler, metrics_handler,
    settle_billing,
};
use tangle_inference_core::{AppState, CostModel, CostParams, PerSecondCostModel, RequestGuard};

use blueprint_webhooks::notifier::{
    JobEvent, JobNotifier, NotifierConfig, JobStatus as NotifierJobStatus,
};

use crate::avatar::{AvatarBackend, AvatarRequest, JobInfo, JobStatus};
use crate::config::OperatorConfig;

/// HTTP request body for the generate endpoint.
/// Wraps `AvatarRequest` with an optional webhook callback URL.
#[derive(Debug, Deserialize)]
struct GenerateRequest {
    #[serde(flatten)]
    avatar: AvatarRequest,
    /// Optional webhook URL for push notifications on job status changes.
    #[serde(default)]
    webhook_url: Option<String>,
}

/// In-memory job registry for async polling.
type Jobs = Arc<DashMap<String, JobEntry>>;

/// Per-job metadata including optional webhook URL.
#[derive(Debug, Clone)]
pub struct JobEntry {
    pub info: JobInfo,
    pub webhook_url: Option<String>,
}

/// Backend attached to AppState.
pub struct AvatarAppBackend {
    pub avatar: Arc<AvatarBackend>,
    pub config: Arc<OperatorConfig>,
    pub cost_model: Arc<PerSecondCostModel>,
    pub jobs: Jobs,
    pub notifier: Arc<JobNotifier>,
}

impl AvatarAppBackend {
    pub fn new(config: Arc<OperatorConfig>) -> Self {
        let notifier = Arc::new(JobNotifier::new(NotifierConfig::default()));
        Self {
            cost_model: Arc::new(PerSecondCostModel {
                price_per_second: config.avatar.price_per_compute_second,
            }),
            avatar: Arc::new(AvatarBackend::new(Arc::new(config.avatar.clone()))),
            jobs: Arc::new(DashMap::new()),
            notifier,
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
        .route("/v1/jobs/{job_id}/events", get(sse_handler))
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
    Json(req): Json<GenerateRequest>,
) -> Response {
    let backend = state
        .backend::<AvatarAppBackend>()
        .expect("AvatarAppBackend");

    let webhook_url = req.webhook_url.clone();
    let req = req.avatar;

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

    // Billing gate — estimate based on expected GPU compute time (not output duration).
    // Conservative: avatar generation typically takes ~10x the output duration.
    let estimated_compute_secs = duration * 10;
    let estimated_cost = estimated_compute_secs * backend.avatar.price_per_compute_second();
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
    backend.jobs.insert(
        job_id.clone(),
        JobEntry {
            info: info.clone(),
            webhook_url: webhook_url.clone(),
        },
    );

    // Notify: job is now processing
    let notifier = backend.notifier.clone();
    let _ = notifier
        .notify(
            &job_id,
            JobEvent {
                status: NotifierJobStatus::Processing,
                ..Default::default()
            },
            webhook_url.as_deref(),
        )
        .await;

    // Spawn background poller that settles billing on completion.
    // Track wall-clock compute time for accurate billing.
    let avatar = backend.avatar.clone();
    let cost_model = backend.cost_model.clone();
    let jobs = backend.jobs.clone();
    let billing = state.billing.clone();
    let jid = job_id.clone();
    tokio::spawn(async move {
        let start_time = std::time::Instant::now();
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            match avatar.poll(&jid).await {
                Ok(info) => {
                    let done = matches!(info.status, JobStatus::Completed | JobStatus::Failed);

                    jobs.insert(
                        jid.clone(),
                        JobEntry {
                            info: info.clone(),
                            webhook_url: webhook_url.clone(),
                        },
                    );

                    // Emit notification on status change
                    let notifier_status = match info.status {
                        JobStatus::Completed => NotifierJobStatus::Completed,
                        JobStatus::Failed => NotifierJobStatus::Failed,
                        JobStatus::Processing => NotifierJobStatus::Processing,
                        JobStatus::Queued => NotifierJobStatus::Queued,
                    };
                    let event = JobEvent {
                        status: notifier_status,
                        result: info.result.as_ref().map(|r| {
                            serde_json::to_value(r).unwrap_or_default()
                        }),
                        error: info.error.clone(),
                        ..Default::default()
                    };
                    let _ = notifier
                        .notify(&jid, event, webhook_url.as_deref())
                        .await;

                    if done {
                        // Settle billing based on actual GPU compute time (wall-clock)
                        let compute_secs = start_time.elapsed().as_secs();
                        if let Some(ref auth) = spend_auth {
                            let cost = cost_model.calculate_cost(&CostParams {
                                extra: std::collections::HashMap::from([(
                                    "centiseconds".into(),
                                    compute_secs * 100,
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
        Some(entry) => Json(entry.value().info.clone()).into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            format!("job {job_id} not found"),
            "not_found",
            "job_not_found",
        ),
    }
}

async fn sse_handler(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> axum::response::Sse<impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {
    let backend = state
        .backend::<AvatarAppBackend>()
        .expect("AvatarAppBackend");
    let rx = backend.notifier.subscribe(&job_id).await;
    // Unwrap the Option — if no broadcast exists for this job, create an empty receiver.
    // This matches the pre-API-change behavior where subscribe always returned a Receiver.
    let rx = match rx {
        Some(rx) => rx,
        None => {
            // Create a dummy broadcast channel and immediately return its receiver.
            // The stream will be empty (no events) which is correct for a nonexistent job.
            let (_, rx) = tokio::sync::broadcast::channel::<JobEvent>(1);
            rx
        }
    };
    let stream = tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(event) => {
            let data = serde_json::to_string(&event)
                .unwrap_or_else(|_| r#"{"error":"serialize"}"#.to_string());
            let sse_event = axum::response::sse::Event::default()
                .event(event.status.to_string())
                .data(data);
            Some(Ok(sse_event))
        }
        Err(_) => None,
    });
    axum::response::Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("ping"),
    )
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "avatar-inference",
    }))
}
