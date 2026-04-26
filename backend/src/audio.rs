use std::{sync::Arc, sync::Mutex, time::Duration};

use anyhow::{anyhow, Context};
use audiopus::{
    coder::Encoder as OpusEncoder, Application as OpusApplication, Channels as OpusChannels,
    SampleRate as OpusSampleRate,
};
use bytes::Bytes;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use webrtc::{
    media::Sample, track::track_local::track_local_static_sample::TrackLocalStaticSample,
};

const TARGET_SAMPLE_RATE: u32 = 48_000;
const FRAME_SIZE_SAMPLES: usize = 960; // 20ms at 48kHz mono
const OPUS_MAX_PACKET_SIZE: usize = 1500;

#[derive(Debug, Clone)]
pub struct InputDeviceInfo {
    pub index: usize,
    pub name: String,
    pub default: bool,
}

pub fn list_input_devices() -> anyhow::Result<Vec<InputDeviceInfo>> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();
    let devices = host
        .input_devices()
        .context("Could not enumerate input devices")?;

    let mut out = Vec::new();
    for (index, device) in devices.enumerate() {
        let name = device.name().unwrap_or_else(|_| "<unknown>".to_string());
        out.push(InputDeviceInfo {
            index,
            default: name == default_name,
            name,
        });
    }
    Ok(out)
}

pub struct AudioBridge {
    _stream: cpal::Stream,
    _encoder_task: tokio::task::JoinHandle<()>,
}

pub fn resolve_input_device(selector: Option<&str>) -> anyhow::Result<cpal::Device> {
    let host = cpal::default_host();
    let Some(selector) = selector else {
        return host
            .default_input_device()
            .ok_or_else(|| anyhow!("No default input device available"));
    };

    let devices = host
        .input_devices()
        .context("Could not enumerate input devices")?;
    let all: Vec<cpal::Device> = devices.collect();

    if let Ok(idx) = selector.parse::<usize>() {
        return all
            .get(idx)
            .cloned()
            .ok_or_else(|| anyhow!("Input device index {idx} is out of range"));
    }

    let needle = selector.to_lowercase();
    for dev in all {
        let name = dev.name().unwrap_or_default();
        if name.to_lowercase().contains(&needle) {
            return Ok(dev);
        }
    }
    Err(anyhow!(
        "No input device found matching selector '{}'",
        selector
    ))
}

pub fn choose_stream_config(device: &cpal::Device) -> anyhow::Result<cpal::SupportedStreamConfig> {
    let mut preferred: Option<cpal::SupportedStreamConfig> = None;

    if let Ok(configs) = device.supported_input_configs() {
        for cfg in configs {
            if cfg.min_sample_rate().0 <= TARGET_SAMPLE_RATE
                && cfg.max_sample_rate().0 >= TARGET_SAMPLE_RATE
            {
                let c = cfg.with_sample_rate(cpal::SampleRate(TARGET_SAMPLE_RATE));
                if c.channels() == 1 {
                    return Ok(c);
                }
                if preferred.is_none() {
                    preferred = Some(c);
                }
            }
        }
    }

    preferred
        .or_else(|| device.default_input_config().ok())
        .ok_or_else(|| anyhow!("No supported input config found"))
}

struct FrameAccumulator {
    source_rate: u32,
    channels: usize,
    resample_cursor: f64,
    frame: Vec<i16>,
}

impl FrameAccumulator {
    fn new(source_rate: u32, channels: usize) -> Self {
        Self {
            source_rate,
            channels,
            resample_cursor: 0.0,
            frame: Vec::with_capacity(FRAME_SIZE_SAMPLES * 2),
        }
    }

    fn push_interleaved_f32(&mut self, input: &[f32], tx: &mpsc::Sender<Vec<i16>>) {
        for chunk in input.chunks(self.channels) {
            if chunk.is_empty() {
                continue;
            }
            let mono = chunk.iter().copied().sum::<f32>() / chunk.len() as f32;
            self.push_resampled(mono, tx);
        }
    }

    fn push_resampled(&mut self, sample: f32, tx: &mpsc::Sender<Vec<i16>>) {
        self.resample_cursor += TARGET_SAMPLE_RATE as f64;
        while self.resample_cursor >= self.source_rate as f64 {
            self.resample_cursor -= self.source_rate as f64;
            let clamped = sample.clamp(-1.0, 1.0);
            self.frame.push((clamped * i16::MAX as f32) as i16);
            if self.frame.len() == FRAME_SIZE_SAMPLES {
                let packet = self.frame.clone();
                if tx.try_send(packet).is_err() {
                    // The encoder task is slower than capture.
                    warn!("Dropping audio frame because encoder channel is full");
                }
                self.frame.clear();
            }
        }
    }
}

pub fn start_audio_bridge(
    input_selector: Option<&str>,
    track: Arc<TrackLocalStaticSample>,
) -> anyhow::Result<AudioBridge> {
    let device = resolve_input_device(input_selector)?;
    let config = choose_stream_config(&device)?;
    let stream_config = config.config();

    let device_name = device.name().unwrap_or_else(|_| "<unknown>".to_string());
    info!(
        "Using input device '{}' at {}Hz, {} channel(s), {:?}",
        device_name,
        stream_config.sample_rate.0,
        stream_config.channels,
        config.sample_format()
    );

    let (tx, mut rx) = mpsc::channel::<Vec<i16>>(64);
    let accumulator = Arc::new(Mutex::new(FrameAccumulator::new(
        stream_config.sample_rate.0,
        stream_config.channels as usize,
    )));

    let err_fn = |e| warn!("Input stream error: {e}");

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let acc = Arc::clone(&accumulator);
            let tx = tx.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    if let Ok(mut lock) = acc.lock() {
                        lock.push_interleaved_f32(data, &tx);
                    }
                },
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let acc = Arc::clone(&accumulator);
            let tx = tx.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    if let Ok(mut lock) = acc.lock() {
                        let converted: Vec<f32> =
                            data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                        lock.push_interleaved_f32(&converted, &tx);
                    }
                },
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::U16 => {
            let acc = Arc::clone(&accumulator);
            let tx = tx.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[u16], _| {
                    if let Ok(mut lock) = acc.lock() {
                        let converted: Vec<f32> = data
                            .iter()
                            .map(|s| ((*s as f32 / u16::MAX as f32) * 2.0) - 1.0)
                            .collect();
                        lock.push_interleaved_f32(&converted, &tx);
                    }
                },
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::U8 => {
            let acc = Arc::clone(&accumulator);
            let tx = tx.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[u8], _| {
                    if let Ok(mut lock) = acc.lock() {
                        let converted: Vec<f32> =
                            data.iter().map(|s| ((*s as f32 / u8::MAX as f32) * 2.0) - 1.0).collect();
                        lock.push_interleaved_f32(&converted, &tx);
                    }
                },
                err_fn,
                None,
            )?
        }
        other => return Err(anyhow!("Unsupported sample format: {other:?}")),
    };

    stream.play().context("Could not start microphone stream")?;

    let encoder_task = tokio::spawn(async move {
        let encoder = match OpusEncoder::new(
            OpusSampleRate::Hz48000,
            OpusChannels::Mono,
            OpusApplication::Voip,
        ) {
            Ok(e) => e,
            Err(e) => {
                warn!("Could not create Opus encoder: {e}");
                return;
            }
        };
        let mut output = [0u8; OPUS_MAX_PACKET_SIZE];
        while let Some(frame) = rx.recv().await {
            let len = match encoder.encode(&frame, &mut output) {
                Ok(n) => n,
                Err(e) => {
                    warn!("Opus encoding failed: {e}");
                    continue;
                }
            };
            let sample = Sample {
                data: Bytes::copy_from_slice(&output[..len]),
                duration: Duration::from_millis(20),
                ..Default::default()
            };
            if let Err(e) = track.write_sample(&sample).await {
                debug!("Track write dropped (usually no active peer yet): {e}");
            }
        }
    });

    Ok(AudioBridge {
        _stream: stream,
        _encoder_task: encoder_task,
    })
}
