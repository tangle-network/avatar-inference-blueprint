//! Avatar generation backends. Operators choose their backend via config.
//!
//! Supported backends:
//! - HeyGen (commercial API, best quality)
//! - D-ID (commercial API, budget option)
//! - Replicate (hosted open-source models)
//! - ComfyUI (self-hosted, e.g. SadTalker/MuseTalk nodes)

use crate::config::AvatarConfig;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Request to generate an avatar video.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvatarRequest {
    /// URL to narration audio (wav/mp3).
    pub audio_url: String,
    /// URL to face image, OR a preset avatar ID.
    #[serde(default)]
    pub image_url: Option<String>,
    /// Preset avatar identifier (provider-specific).
    #[serde(default)]
    pub avatar_id: Option<String>,
    /// Target duration in seconds (capped by operator's max_duration_seconds).
    #[serde(default = "default_duration")]
    pub duration_seconds: u64,
    /// Output format.
    #[serde(default = "default_format")]
    pub output_format: String,
}

fn default_duration() -> u64 {
    30
}
fn default_format() -> String {
    "mp4".to_string()
}

/// Result of an avatar generation job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvatarResult {
    pub video_url: String,
    pub duration_seconds: f64,
    pub format: String,
}

/// Job status for async polling.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobInfo {
    pub job_id: String,
    pub status: JobStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<AvatarResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Unified avatar backend that dispatches to the configured provider.
pub struct AvatarBackend {
    config: Arc<AvatarConfig>,
    http: reqwest::Client,
}

impl AvatarBackend {
    pub fn new(config: Arc<AvatarConfig>) -> Self {
        Self {
            config,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("HTTP client"),
        }
    }

    /// Submit a generation job. Returns a provider-specific job ID.
    pub async fn submit(&self, req: &AvatarRequest) -> Result<String> {
        match self.config.backend.as_str() {
            "heygen" => self.submit_heygen(req).await,
            "did" => self.submit_did(req).await,
            "replicate" => self.submit_replicate(req).await,
            "comfyui" => self.submit_comfyui(req).await,
            other => anyhow::bail!("unsupported avatar backend: {other}"),
        }
    }

    /// Poll a job's status. Returns the current status + result if complete.
    pub async fn poll(&self, job_id: &str) -> Result<JobInfo> {
        match self.config.backend.as_str() {
            "heygen" => self.poll_heygen(job_id).await,
            "did" => self.poll_did(job_id).await,
            "replicate" => self.poll_replicate(job_id).await,
            "comfyui" => self.poll_comfyui(job_id).await,
            other => anyhow::bail!("unsupported avatar backend: {other}"),
        }
    }

    pub fn max_duration(&self) -> u64 {
        self.config.max_duration_seconds
    }

    pub fn price_per_second(&self) -> u64 {
        self.config.price_per_second
    }

    // ── HeyGen ──────────────────────────────────────────────────────────

    async fn submit_heygen(&self, req: &AvatarRequest) -> Result<String> {
        let api_key = self
            .config
            .heygen_api_key
            .as_deref()
            .context("heygen_api_key not configured")?;

        let mut body = serde_json::json!({
            "video_inputs": [{
                "voice": {
                    "type": "audio",
                    "audio_url": req.audio_url,
                },
                "character": {},
            }],
            "dimension": { "width": 1920, "height": 1080 },
        });

        // Use avatar preset or custom image
        if let Some(ref avatar_id) = req.avatar_id {
            body["video_inputs"][0]["character"]["type"] = "avatar".into();
            body["video_inputs"][0]["character"]["avatar_id"] = avatar_id.clone().into();
        } else if let Some(ref image_url) = req.image_url {
            body["video_inputs"][0]["character"]["type"] = "talking_photo".into();
            body["video_inputs"][0]["character"]["talking_photo_url"] = image_url.clone().into();
        } else {
            anyhow::bail!("either avatar_id or image_url required");
        }

        let resp = self
            .http
            .post("https://api.heygen.com/v2/video/generate")
            .header("x-api-key", api_key)
            .json(&body)
            .send()
            .await
            .context("HeyGen submit request failed")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("HeyGen submit failed: {body}");
        }

        let json: serde_json::Value = resp.json().await?;
        json["data"]["video_id"]
            .as_str()
            .map(|s| s.to_string())
            .context("HeyGen response missing video_id")
    }

    async fn poll_heygen(&self, job_id: &str) -> Result<JobInfo> {
        let api_key = self
            .config
            .heygen_api_key
            .as_deref()
            .context("heygen_api_key not configured")?;

        let resp = self
            .http
            .get(format!(
                "https://api.heygen.com/v1/video_status.get?video_id={job_id}"
            ))
            .header("x-api-key", api_key)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;
        let data = &json["data"];
        let status_str = data["status"].as_str().unwrap_or("unknown");

        Ok(match status_str {
            "completed" => JobInfo {
                job_id: job_id.to_string(),
                status: JobStatus::Completed,
                result: Some(AvatarResult {
                    video_url: data["video_url"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    duration_seconds: data["duration"].as_f64().unwrap_or(0.0),
                    format: "mp4".to_string(),
                }),
                error: None,
            },
            "failed" => JobInfo {
                job_id: job_id.to_string(),
                status: JobStatus::Failed,
                result: None,
                error: Some(
                    data["error"]["message"]
                        .as_str()
                        .unwrap_or("unknown error")
                        .to_string(),
                ),
            },
            "processing" | "pending" => JobInfo {
                job_id: job_id.to_string(),
                status: JobStatus::Processing,
                result: None,
                error: None,
            },
            _ => JobInfo {
                job_id: job_id.to_string(),
                status: JobStatus::Queued,
                result: None,
                error: None,
            },
        })
    }

    // ── D-ID ────────────────────────────────────────────────────────────

    async fn submit_did(&self, req: &AvatarRequest) -> Result<String> {
        let api_key = self
            .config
            .did_api_key
            .as_deref()
            .context("did_api_key not configured")?;

        let source = if let Some(ref image_url) = req.image_url {
            serde_json::json!({ "type": "image", "url": image_url })
        } else {
            anyhow::bail!("D-ID requires image_url");
        };

        let body = serde_json::json!({
            "source_url": source["url"],
            "script": {
                "type": "audio",
                "audio_url": req.audio_url,
            },
        });

        let resp = self
            .http
            .post("https://api.d-id.com/talks")
            .basic_auth(api_key, Option::<&str>::None)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("D-ID submit failed: {body}");
        }

        let json: serde_json::Value = resp.json().await?;
        json["id"]
            .as_str()
            .map(|s| s.to_string())
            .context("D-ID response missing id")
    }

    async fn poll_did(&self, job_id: &str) -> Result<JobInfo> {
        let api_key = self
            .config
            .did_api_key
            .as_deref()
            .context("did_api_key not configured")?;

        let resp = self
            .http
            .get(format!("https://api.d-id.com/talks/{job_id}"))
            .basic_auth(api_key, Option::<&str>::None)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;
        let status_str = json["status"].as_str().unwrap_or("unknown");

        Ok(match status_str {
            "done" => JobInfo {
                job_id: job_id.to_string(),
                status: JobStatus::Completed,
                result: Some(AvatarResult {
                    video_url: json["result_url"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    duration_seconds: json["duration"].as_f64().unwrap_or(0.0),
                    format: "mp4".to_string(),
                }),
                error: None,
            },
            "error" => JobInfo {
                job_id: job_id.to_string(),
                status: JobStatus::Failed,
                result: None,
                error: json["error"]["description"]
                    .as_str()
                    .map(|s| s.to_string()),
            },
            _ => JobInfo {
                job_id: job_id.to_string(),
                status: JobStatus::Processing,
                result: None,
                error: None,
            },
        })
    }

    // ── Replicate ───────────────────────────────────────────────────────

    async fn submit_replicate(&self, req: &AvatarRequest) -> Result<String> {
        let token = self
            .config
            .replicate_api_token
            .as_deref()
            .context("replicate_api_token not configured")?;

        let image_url = req
            .image_url
            .as_deref()
            .context("Replicate requires image_url")?;

        // Default to SadTalker on Replicate
        let body = serde_json::json!({
            "version": "a519cc0cfebaaeade0724aa4a1a1e2d6dc1bdb44e0249c83ee86b65940e06345",
            "input": {
                "source_image": image_url,
                "driven_audio": req.audio_url,
            }
        });

        let resp = self
            .http
            .post("https://api.replicate.com/v1/predictions")
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Replicate submit failed: {body}");
        }

        let json: serde_json::Value = resp.json().await?;
        json["id"]
            .as_str()
            .map(|s| s.to_string())
            .context("Replicate response missing id")
    }

    async fn poll_replicate(&self, job_id: &str) -> Result<JobInfo> {
        let token = self
            .config
            .replicate_api_token
            .as_deref()
            .context("replicate_api_token not configured")?;

        let resp = self
            .http
            .get(format!(
                "https://api.replicate.com/v1/predictions/{job_id}"
            ))
            .bearer_auth(token)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;
        let status_str = json["status"].as_str().unwrap_or("unknown");

        Ok(match status_str {
            "succeeded" => {
                let output = json["output"].as_str().unwrap_or_default().to_string();
                JobInfo {
                    job_id: job_id.to_string(),
                    status: JobStatus::Completed,
                    result: Some(AvatarResult {
                        video_url: output,
                        duration_seconds: 0.0, // Replicate doesn't return duration
                        format: "mp4".to_string(),
                    }),
                    error: None,
                }
            }
            "failed" | "canceled" => JobInfo {
                job_id: job_id.to_string(),
                status: JobStatus::Failed,
                result: None,
                error: json["error"].as_str().map(|s| s.to_string()),
            },
            _ => JobInfo {
                job_id: job_id.to_string(),
                status: JobStatus::Processing,
                result: None,
                error: None,
            },
        })
    }

    // ── ComfyUI (self-hosted) ───────────────────────────────────────────

    async fn submit_comfyui(&self, req: &AvatarRequest) -> Result<String> {
        let endpoint = self
            .config
            .comfyui_endpoint
            .as_deref()
            .context("comfyui_endpoint not configured")?;

        let image_url = req
            .image_url
            .as_deref()
            .context("ComfyUI requires image_url")?;

        let body = serde_json::json!({
            "prompt": {
                "audio_url": req.audio_url,
                "image_url": image_url,
                "output_format": req.output_format,
            }
        });

        let resp = self
            .http
            .post(format!("{endpoint}/prompt"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("ComfyUI submit failed: {body}");
        }

        let json: serde_json::Value = resp.json().await?;
        json["prompt_id"]
            .as_str()
            .map(|s| s.to_string())
            .context("ComfyUI response missing prompt_id")
    }

    async fn poll_comfyui(&self, job_id: &str) -> Result<JobInfo> {
        let endpoint = self
            .config
            .comfyui_endpoint
            .as_deref()
            .context("comfyui_endpoint not configured")?;

        let resp = self
            .http
            .get(format!("{endpoint}/history/{job_id}"))
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;

        if let Some(outputs) = json[job_id]["outputs"].as_object() {
            // Find the first video output
            for (_node_id, node_output) in outputs {
                if let Some(videos) = node_output["videos"].as_array() {
                    if let Some(video) = videos.first() {
                        let filename = video["filename"].as_str().unwrap_or_default();
                        return Ok(JobInfo {
                            job_id: job_id.to_string(),
                            status: JobStatus::Completed,
                            result: Some(AvatarResult {
                                video_url: format!("{endpoint}/view?filename={filename}"),
                                duration_seconds: 0.0,
                                format: "mp4".to_string(),
                            }),
                            error: None,
                        });
                    }
                }
            }
        }

        // Not done yet or error
        if json[job_id]["status"]["status_str"].as_str() == Some("error") {
            Ok(JobInfo {
                job_id: job_id.to_string(),
                status: JobStatus::Failed,
                result: None,
                error: Some("ComfyUI workflow failed".to_string()),
            })
        } else {
            Ok(JobInfo {
                job_id: job_id.to_string(),
                status: JobStatus::Processing,
                result: None,
                error: None,
            })
        }
    }
}
