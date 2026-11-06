use alloy_sol_types::SolValue;
use avatar_inference::config::OperatorConfig;
use avatar_inference::AvatarInferenceServer;

fn registration_payload(config: &OperatorConfig) -> Vec<u8> {
    (
        config.avatar.backend.clone(),
        config.gpu.expected_gpu_count,
        config.gpu.min_vram_mib,
        format!("http://{}:{}", config.server.host, config.server.port),
    )
        .abi_encode()
}

tangle_inference_core::run_operator!(
    OperatorConfig::load(None),
    registration_payload,
    avatar_inference::router(),
    |config: std::sync::Arc<OperatorConfig>| AvatarInferenceServer::new((*config).clone()),
    no_qos
);
