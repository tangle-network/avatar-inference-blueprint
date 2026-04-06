use blueprint_sdk::contexts::tangle::TangleClientContext;
use blueprint_sdk::runner::config::BlueprintEnvironment;
use blueprint_sdk::runner::tangle::config::TangleConfig;
use blueprint_sdk::runner::BlueprintRunner;
use blueprint_sdk::tangle::{TangleConsumer, TangleProducer};

use avatar_inference::config::OperatorConfig;
use avatar_inference::{detect_gpus, AvatarInferenceServer};

fn setup_log() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).init();
}

#[tokio::main]
#[allow(clippy::result_large_err)]
async fn main() -> Result<(), blueprint_sdk::Error> {
    setup_log();
    dotenvy::dotenv().ok();

    tracing::info!("Avatar Inference Blueprint starting...");

    let config = OperatorConfig::load(None)
        .map_err(|e| blueprint_sdk::Error::Other(format!("config: {e}")))?;

    tracing::info!(
        backend = %config.avatar.backend,
        price_per_second = config.avatar.price_per_second,
        max_duration = config.avatar.max_duration_seconds,
        "Config loaded"
    );

    match detect_gpus().await {
        Ok(gpus) => {
            for gpu in &gpus {
                tracing::info!(name = %gpu.name, vram_mib = gpu.memory_total_mib, "GPU");
            }
        }
        Err(e) => tracing::info!(error = %e, "No GPUs detected (API proxy mode is fine)"),
    }

    let env = BlueprintEnvironment::load()?;
    let tangle_client = env
        .tangle_client()
        .await
        .map_err(|e| blueprint_sdk::Error::Other(e.to_string()))?;

    let service_id = env
        .protocol_settings
        .tangle()
        .map_err(|e| blueprint_sdk::Error::Other(e.to_string()))?
        .service_id
        .ok_or_else(|| blueprint_sdk::Error::Other("No service_id".to_string()))?;

    let tangle_producer = TangleProducer::new(tangle_client.clone(), service_id);
    let tangle_consumer = TangleConsumer::new(tangle_client);

    let server = AvatarInferenceServer::new(config);

    tracing::info!("Starting BlueprintRunner...");

    BlueprintRunner::builder(TangleConfig::default(), env)
        .router(avatar_inference::router())
        .producer(tangle_producer)
        .consumer(tangle_consumer)
        .background_service(server)
        .run()
        .await?;

    Ok(())
}
