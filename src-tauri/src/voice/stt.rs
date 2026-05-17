//! HTTP-Client für den faster-whisper Server.

use super::{audio, trim_base_url};

pub async fn transcribe_audio_http(
    base_url: &str,
    samples: &[f32],
    source_sample_rate: u32,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let audio_16k = if source_sample_rate != 16000 {
        audio::resample(samples, source_sample_rate, 16000)
    } else {
        samples.to_vec()
    };

    let mut body = Vec::with_capacity(audio_16k.len() * std::mem::size_of::<f32>());
    for sample in audio_16k {
        body.extend_from_slice(&sample.to_le_bytes());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let resp = client
        .post(format!("{}/transcribe", trim_base_url(base_url)))
        .body(body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Whisper HTTP error {}: {}", status, body).into());
    }

    let json: serde_json::Value = resp.json().await?;
    if let Some(error) = json.get("error").and_then(|v| v.as_str()) {
        return Err(format!("Whisper HTTP error: {}", error).into());
    }

    Ok(json
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string())
}
