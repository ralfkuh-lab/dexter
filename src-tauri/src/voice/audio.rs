//! Mikrofon-Aufnahme via cpal.

use crate::AppState;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::Manager;

/// Record audio on the current thread until `is_recording` is set to false.
/// Writes samples directly into AppState's shared buffer.
pub fn record_audio(
    app: &tauri::AppHandle,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("No input device available")?;

    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    {
        let state = app.state::<AppState>();
        *state.recording_sample_rate.lock().unwrap() = sample_rate;
    }

    let app_clone = app.clone();

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let app_ref = app.clone();
            device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let state = app_ref.state::<AppState>();
                    if channels <= 1 {
                        state
                            .recorded_samples
                            .lock()
                            .unwrap()
                            .extend_from_slice(data);
                    } else {
                        let mono: Vec<f32> = data
                            .chunks(channels)
                            .map(|frame| frame.iter().copied().sum::<f32>() / frame.len() as f32)
                            .collect();
                        state
                            .recorded_samples
                            .lock()
                            .unwrap()
                            .extend_from_slice(&mono);
                    }
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let app_ref = app.clone();
            device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let floats: Vec<f32> = if channels <= 1 {
                        data.iter().map(|&s| s as f32 / 32768.0).collect()
                    } else {
                        data.chunks(channels)
                            .map(|frame| {
                                frame.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                                    / frame.len() as f32
                            })
                            .collect()
                    };
                    let state = app_ref.state::<AppState>();
                    state
                        .recorded_samples
                        .lock()
                        .unwrap()
                        .extend_from_slice(&floats);
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )?
        }
        format => {
            return Err(format!("Unsupported sample format: {:?}", format).into());
        }
    };

    stream.play()?;

    loop {
        let is_rec = *app_clone.state::<AppState>().is_recording.lock().unwrap();
        if !is_rec {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    Ok(())
}

/// Linear-Interpolations-Resampler (Recording → 16 kHz für Whisper).
pub(super) fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    let ratio = to_rate as f64 / from_rate as f64;
    let output_len = (input.len() as f64 * ratio) as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_idx = i as f64 / ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;

        let sample = if idx + 1 < input.len() {
            input[idx] as f64 * (1.0 - frac) + input[idx + 1] as f64 * frac
        } else if idx < input.len() {
            input[idx] as f64
        } else {
            0.0
        };

        output.push(sample as f32);
    }

    output
}
