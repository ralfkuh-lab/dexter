//! Mikrofon-Aufnahme via cpal.

use crate::AppState;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;

const VAD_FRAME_MS: u32 = 30;
const VAD_SILENCE_END_MS: u32 = 800;
const VAD_PREROLL_MS: u32 = 250;
const VAD_TRAILING_MS: u32 = 200;
const VAD_MIN_SEGMENT_MS: u32 = 250;
const VAD_CALIBRATION_MS: u32 = 500;
const VAD_DEFAULT_THRESHOLD: f32 = 0.015;
const VAD_MIN_THRESHOLD: f32 = 0.010;
const VAD_MAX_THRESHOLD: f32 = 0.080;

#[derive(Clone, Debug)]
pub struct AudioSegment {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

#[derive(Clone, Serialize)]
struct VadEvent {
    rms: f32,
    threshold: f32,
    speech: bool,
}

fn select_input_device(host: &cpal::Host, wanted: &str) -> Option<cpal::Device> {
    if wanted.is_empty() {
        return host.default_input_device();
    }

    if let Ok(devices) = host.input_devices() {
        for device in devices {
            if device.name().ok().as_deref() == Some(wanted) {
                return Some(device);
            }
        }
    }

    eprintln!(
        "Configured input device {:?} not found; using system default",
        wanted
    );
    host.default_input_device()
}

/// Record audio on the current thread until `is_recording` is set to false.
/// Writes samples directly into AppState's shared buffer.
pub fn record_audio(
    app: &tauri::AppHandle,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let host = cpal::default_host();
    let wanted = {
        let state = app.state::<AppState>();
        let config = state.config.lock().unwrap();
        config.input_device.clone()
    };
    let device = select_input_device(&host, &wanted).ok_or("No input device available")?;

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

/// Record continuously and split speech into segments with a simple energy VAD.
/// Segments are sent through `segment_tx` until `cancel` is cancelled.
pub fn record_continuous(
    app: &tauri::AppHandle,
    segment_tx: mpsc::Sender<AudioSegment>,
    cancel: CancellationToken,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let host = cpal::default_host();
    let wanted = {
        let state = app.state::<AppState>();
        let config = state.config.lock().unwrap();
        config.input_device.clone()
    };
    let device = select_input_device(&host, &wanted).ok_or("No input device available")?;

    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let stream_config: cpal::StreamConfig = config.clone().into();
    let vad = Arc::new(Mutex::new(EnergyVad::new(
        sample_rate,
        segment_tx,
        Some(app.clone()),
    )));

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let app_ref = app.clone();
            let vad_ref = vad.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if *app_ref.state::<AppState>().is_speaking.lock().unwrap() {
                        if let Ok(mut vad) = vad_ref.lock() {
                            vad.reset_for_gate();
                        }
                        return;
                    }

                    let samples: Vec<f32> = if channels <= 1 {
                        data.to_vec()
                    } else {
                        data.chunks(channels)
                            .map(|frame| frame.iter().copied().sum::<f32>() / frame.len() as f32)
                            .collect()
                    };

                    if let Ok(mut vad) = vad_ref.lock() {
                        vad.append_samples(&samples);
                    }
                },
                |err| eprintln!("Continuous audio stream error: {}", err),
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let app_ref = app.clone();
            let vad_ref = vad.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if *app_ref.state::<AppState>().is_speaking.lock().unwrap() {
                        if let Ok(mut vad) = vad_ref.lock() {
                            vad.reset_for_gate();
                        }
                        return;
                    }

                    let samples: Vec<f32> = if channels <= 1 {
                        data.iter().map(|&s| s as f32 / 32768.0).collect()
                    } else {
                        data.chunks(channels)
                            .map(|frame| {
                                frame.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                                    / frame.len() as f32
                            })
                            .collect()
                    };

                    if let Ok(mut vad) = vad_ref.lock() {
                        vad.append_samples(&samples);
                    }
                },
                |err| eprintln!("Continuous audio stream error: {}", err),
                None,
            )?
        }
        format => {
            return Err(format!("Unsupported sample format: {:?}", format).into());
        }
    };

    stream.play()?;

    while !cancel.is_cancelled() {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    drop(stream);
    if let Ok(mut vad) = vad.lock() {
        vad.finish();
    }
    let _ = app.emit(
        "dictation_vad",
        VadEvent {
            rms: 0.0,
            threshold: VAD_DEFAULT_THRESHOLD,
            speech: false,
        },
    );

    Ok(())
}

struct EnergyVad {
    sample_rate: u32,
    frame_size: usize,
    silence_frames_required: usize,
    calibration_frames_required: usize,
    threshold: f32,
    calibrated: bool,
    calibration_rms: Vec<f32>,
    frame: Vec<f32>,
    preroll: VecDeque<f32>,
    current_segment: Vec<f32>,
    speech_active: bool,
    silent_frames: usize,
    max_preroll_samples: usize,
    min_segment_samples: usize,
    trailing_keep_samples: usize,
    tx: mpsc::Sender<AudioSegment>,
    app: Option<tauri::AppHandle>,
}

impl EnergyVad {
    fn new(
        sample_rate: u32,
        tx: mpsc::Sender<AudioSegment>,
        app: Option<tauri::AppHandle>,
    ) -> Self {
        let frame_size = ((sample_rate as u64 * VAD_FRAME_MS as u64) / 1000).max(1) as usize;
        let silence_frames_required =
            ((VAD_SILENCE_END_MS + VAD_FRAME_MS - 1) / VAD_FRAME_MS) as usize;
        let calibration_frames_required =
            ((VAD_CALIBRATION_MS + VAD_FRAME_MS - 1) / VAD_FRAME_MS) as usize;

        Self {
            sample_rate,
            frame_size,
            silence_frames_required,
            calibration_frames_required,
            threshold: VAD_DEFAULT_THRESHOLD,
            calibrated: false,
            calibration_rms: Vec::with_capacity(calibration_frames_required),
            frame: Vec::with_capacity(frame_size),
            preroll: VecDeque::with_capacity(
                ((sample_rate as u64 * VAD_PREROLL_MS as u64) / 1000) as usize,
            ),
            current_segment: Vec::new(),
            speech_active: false,
            silent_frames: 0,
            max_preroll_samples: ((sample_rate as u64 * VAD_PREROLL_MS as u64) / 1000) as usize,
            min_segment_samples: ((sample_rate as u64 * VAD_MIN_SEGMENT_MS as u64) / 1000) as usize,
            trailing_keep_samples: ((sample_rate as u64 * VAD_TRAILING_MS as u64) / 1000) as usize,
            tx,
            app,
        }
    }

    fn append_samples(&mut self, samples: &[f32]) {
        for &sample in samples {
            if self.speech_active {
                self.current_segment.push(sample);
            } else {
                self.preroll.push_back(sample);
                while self.preroll.len() > self.max_preroll_samples {
                    self.preroll.pop_front();
                }
            }

            self.frame.push(sample);
            if self.frame.len() >= self.frame_size {
                let rms = rms(&self.frame);
                self.frame.clear();
                self.process_frame(rms);
            }
        }
    }

    fn process_frame(&mut self, rms: f32) {
        if !self.calibrated {
            if rms < VAD_DEFAULT_THRESHOLD {
                self.calibration_rms.push(rms);
                if self.calibration_rms.len() >= self.calibration_frames_required {
                    let avg = self.calibration_rms.iter().sum::<f32>()
                        / self.calibration_rms.len() as f32;
                    self.threshold = (avg * 3.0).clamp(VAD_MIN_THRESHOLD, VAD_MAX_THRESHOLD);
                    self.calibrated = true;
                }
            } else {
                self.calibrated = true;
            }
        }

        let above_threshold = rms >= self.threshold;
        if above_threshold {
            if !self.speech_active {
                self.speech_active = true;
                self.current_segment = self.preroll.iter().copied().collect();
                self.preroll.clear();
                self.emit(rms);
            }
            self.silent_frames = 0;
        } else if self.speech_active {
            self.silent_frames += 1;
            if self.silent_frames >= self.silence_frames_required {
                self.finish_segment();
            }
        }

        if !above_threshold || !self.speech_active {
            self.emit(rms);
        }
    }

    fn reset_for_gate(&mut self) {
        if self.speech_active {
            self.emit_speech(false, 0.0);
        }
        self.frame.clear();
        self.preroll.clear();
        self.current_segment.clear();
        self.speech_active = false;
        self.silent_frames = 0;
    }

    fn finish(&mut self) {
        if self.speech_active {
            self.finish_segment();
        }
    }

    fn finish_segment(&mut self) {
        let silence_samples = self.silent_frames * self.frame_size;
        if silence_samples > self.trailing_keep_samples
            && self.current_segment.len() > silence_samples
        {
            let trim = silence_samples - self.trailing_keep_samples;
            let new_len = self.current_segment.len().saturating_sub(trim);
            self.current_segment.truncate(new_len);
        }

        if self.current_segment.len() >= self.min_segment_samples {
            let samples = std::mem::take(&mut self.current_segment);
            let _ = self.tx.send(AudioSegment {
                samples,
                sample_rate: self.sample_rate,
            });
        } else {
            self.current_segment.clear();
        }

        self.speech_active = false;
        self.silent_frames = 0;
        self.preroll.clear();
        self.emit_speech(false, 0.0);
    }

    fn emit(&self, rms: f32) {
        self.emit_speech(self.speech_active, rms);
    }

    fn emit_speech(&self, speech: bool, rms: f32) {
        if let Some(app) = &self.app {
            let _ = app.emit(
                "dictation_vad",
                VadEvent {
                    rms,
                    threshold: self.threshold,
                    speech,
                },
            );
        }
    }
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
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
