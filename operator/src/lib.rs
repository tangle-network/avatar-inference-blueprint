pub mod avatar;
pub mod config;
pub mod server;

pub use tangle_inference_core::{
    detect_gpus, AppState, BillingClient, CostModel, CostParams, GpuInfo, NonceStore,
    PerSecondCostModel, RequestGuard, SpendAuthPayload,
};

use crate::config::OperatorConfig;
use crate::server::AvatarAppBackend;

use alloy_sol_types::sol;
use blueprint_sdk::macros::debug_job;
use blueprint_sdk::router::Router;
use blueprint_sdk::runner::error::RunnerError;
use blueprint_sdk::runner::BackgroundService;
use blueprint_sdk::std::sync::Arc;
use blueprint_sdk::tangle::extract::{TangleArg, TangleResult};
use blueprint_sdk::tangle::layers::TangleLayer;
use blueprint_sdk::Job;
use tokio::sync::{oneshot, watch};

sol! {
    #[allow(missing_docs)]
    struct AvatarJobRequest {
        string audioUrl;
        string imageUrl;
        uint32 maxDurationSeconds;
    }

    #[allow(missing_docs)]
    struct AvatarJobResult {
        string videoUrl;
        uint32 durationSeconds;
    }
}

pub const AVATAR_JOB: u8 = 0;

pub fn router() -> Router {
    Router::new().route(AVATAR_JOB, run_avatar.layer(TangleLayer))
}

#[debug_job]
pub async fn run_avatar(
    TangleArg(request): TangleArg<AvatarJobRequest>,
) -> Result<TangleResult<AvatarJobResult>, RunnerError> {
    tracing::info!(
        audio = %request.audioUrl,
        image = %request.imageUrl,
        "Received on-chain avatar request"
    );

    Ok(TangleResult(AvatarJobResult {
        videoUrl: "use /v1/avatar/generate HTTP endpoint".to_string(),
        durationSeconds: 0,
    }))
}

/// Background service that starts the avatar HTTP server.
pub struct AvatarInferenceServer {
    config: OperatorConfig,
}

impl AvatarInferenceServer {
    pub fn new(config: OperatorConfig) -> Self {
        Self { config }
    }
}

impl BackgroundService for AvatarInferenceServer {
    async fn start(&self) -> Result<oneshot::Receiver<Result<(), RunnerError>>, RunnerError> {
        let (tx, rx) = oneshot::channel();
        let config = Arc::new(self.config.clone());

        tokio::spawn(async move {
            let backend = AvatarAppBackend::new(config.clone());

            let state = match AppState::from_config(
                &config.tangle,
                &config.server,
                &config.billing,
                config.server.max_concurrent_requests,
                backend,
            ) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!(error = %e, "failed to build AppState");
                    let _ = tx.send(Err(RunnerError::Other(e.to_string().into())));
                    return;
                }
            };

            let (shutdown_tx, shutdown_rx) = watch::channel(false);

            match server::start(state, shutdown_rx).await {
                Ok(handle) => {
                    tracing::info!("Avatar HTTP server started");
                    // Keep running until the server stops
                    let _ = handle.await;
                    let _ = tx.send(Ok(()));
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to start HTTP server");
                    let _ = tx.send(Err(RunnerError::Other(e.to_string().into())));
                }
            }

            drop(shutdown_tx);
        });

        Ok(rx)
    }
}
