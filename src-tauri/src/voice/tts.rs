//! HTTP-Client für den Piper TTS-Server (OpenAI-kompatibler `/v1/audio/speech`).

use super::trim_base_url;
use crate::VoiceConfig;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;

#[derive(Serialize)]
struct TtsRequest {
    input: String,
    voice: String,
}

pub async fn synthesize(
    config: &VoiceConfig,
    text: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let request = TtsRequest {
        input: text.to_string(),
        voice: config.tts_voice.clone(),
    };

    let resp = client
        .post(format!(
            "{}/v1/audio/speech",
            trim_base_url(&config.tts_url)
        ))
        .json(&request)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("TTS API error {}: {}", status, body).into());
    }

    let audio_bytes = resp.bytes().await?;
    Ok(STANDARD.encode(&audio_bytes))
}
