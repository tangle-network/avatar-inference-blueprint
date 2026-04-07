use serde::{Deserialize, Serialize};
pub use tangle_inference_core::{BillingConfig, GpuConfig, ServerConfig, TangleConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorConfig {
    pub tangle: TangleConfig,
    pub server: ServerConfig,
    pub billing: BillingConfig,
    pub gpu: GpuConfig,
    pub avatar: AvatarConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvatarConfig {
    /// Backend to use: "heygen", "did", "replicate", or "comfyui".
    #[serde(default = "default_backend")]
    pub backend: String,

    /// HeyGen API key (required when backend = "heygen").
    pub heygen_api_key: Option<String>,
    /// D-ID API key (required when backend = "did").
    pub did_api_key: Option<String>,
    /// Replicate API token (required when backend = "replicate").
    pub replicate_api_token: Option<String>,
    /// ComfyUI endpoint (required when backend = "comfyui").
    pub comfyui_endpoint: Option<String>,

    /// Price per second of generated avatar video (payment token base units, e.g. 500000 = 0.50 USDC with 6 decimals).
    #[serde(default = "default_price_per_second")]
    pub price_per_second: u64,

    /// Maximum video duration in seconds.
    #[serde(default = "default_max_duration")]
    pub max_duration_seconds: u64,
}

fn default_backend() -> String {
    "heygen".to_string()
}
fn default_price_per_second() -> u64 {
    500_000 // 0.50 payment token per second
}
fn default_max_duration() -> u64 {
    300 // 5 minutes
}

impl OperatorConfig {
    pub fn load(path: Option<&str>) -> anyhow::Result<Self> {
        let builder = config::Config::builder();
        let builder = if let Some(p) = path {
            builder.add_source(config::File::with_name(p))
        } else if let Ok(p) = std::env::var("CONFIG_PATH") {
            builder.add_source(config::File::with_name(&p))
        } else {
            builder.add_source(config::File::with_name("config").required(false))
        };
        let config = builder
            .add_source(config::Environment::with_prefix("AVATAR_OP").separator("__"))
            .build()?;
        Ok(config.try_deserialize()?)
    }
}

impl Default for AvatarConfig {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            heygen_api_key: None,
            did_api_key: None,
            replicate_api_token: None,
            comfyui_endpoint: None,
            price_per_second: default_price_per_second(),
            max_duration_seconds: default_max_duration(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_avatar_config() {
        let cfg = AvatarConfig::default();
        assert_eq!(cfg.backend, "heygen");
        assert_eq!(cfg.price_per_second, 500_000);
        assert_eq!(cfg.max_duration_seconds, 300);
    }
}
