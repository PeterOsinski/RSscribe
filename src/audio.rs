use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use std::sync::{Arc, Mutex};

/// Holds an in-progress recording. Keep the `Stream` alive for the
/// whole recording — dropping it stops capture.
pub struct Recording {
    stream: Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    pub sample_rate: u32,
}

impl Recording {
    /// Snapshot all samples captured so far, without stopping the stream. Used
    /// by continuous dictation to poll the mic while it is still recording.
    pub fn snapshot(&self) -> Vec<f32> {
        self.samples.lock().unwrap().clone()
    }
}

/// Start capturing from the default input device.
pub fn start_recording() -> Result<Recording> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("no input (microphone) device found"))?;
    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let sample_format = config.sample_format();

    let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
    let samples_cb = samples.clone();

    // Downmix to mono as we go so we don't have to post-process later.
    let err_fn = |e| log::error!("audio stream error: {e}");

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _| push_mono(&samples_cb, data, channels, |s| s),
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _| {
                push_mono(&samples_cb, data, channels, |s| s as f32 / i16::MAX as f32)
            },
            err_fn,
            None,
        )?,
        SampleFormat::U16 => device.build_input_stream(
            &config.into(),
            move |data: &[u16], _| {
                push_mono(&samples_cb, data, channels, |s| {
                    (s as f32 - 32768.0) / 32768.0
                })
            },
            err_fn,
            None,
        )?,
        other => return Err(anyhow!("unsupported sample format: {other:?}")),
    };

    stream.play()?;

    Ok(Recording {
        stream,
        samples,
        sample_rate,
    })
}

fn push_mono<T: Copy>(
    buf: &Arc<Mutex<Vec<f32>>>,
    data: &[T],
    channels: usize,
    to_f32: impl Fn(T) -> f32,
) {
    let mut out = buf.lock().unwrap();
    for frame in data.chunks(channels.max(1)) {
        let sum: f32 = frame.iter().map(|s| to_f32(*s)).sum();
        out.push(sum / channels as f32);
    }
}

/// Stop capture and return the recorded samples at the device's native sample
/// rate, along with that rate. We deliberately do NOT resample here — the
/// whisper-server resamples to 16 kHz itself.
pub fn stop_recording(rec: Recording) -> Result<(Vec<f32>, u32)> {
    drop(rec.stream); // stops the stream
    let samples = rec.samples.lock().unwrap().clone();
    Ok((samples, rec.sample_rate))
}

/// Write mono samples to a WAV file at the given sample rate.
pub fn write_wav(samples: &[f32], sample_rate: u32, path: &std::path::Path) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for s in samples {
        let clamped = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer.write_sample(clamped)?;
    }
    writer.finalize()?;
    Ok(())
}

/// Root-mean-square amplitude of a mono sample slice (in -1..1).
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Simple streaming energy-based voice-activity detector. Tracks the noise
/// floor and classifies a frame's RMS level as speech or silence.
pub struct Vad {
    noise_floor: f32,
}

impl Vad {
    pub fn new() -> Self {
        Vad { noise_floor: 0.003 }
    }

    /// Classify a frame by its RMS level; adapts the noise floor on quiet
    /// frames so it tracks the current environment.
    pub fn classify(&mut self, level: f32) -> bool {
        let threshold = (self.noise_floor * 4.0).max(0.008);
        if level > threshold {
            true
        } else {
            if level < self.noise_floor * 3.0 {
                self.noise_floor = 0.9 * self.noise_floor + 0.1 * level;
            }
            false
        }
    }
}

impl Default for Vad {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{rms, Vad};

    #[test]
    fn vad_distinguishes_speech_from_silence() {
        let mut vad = Vad::new();
        // Seed the noise floor with quiet frames.
        for _ in 0..20 {
            assert!(!vad.classify(0.001));
        }
        // A loud frame is speech.
        assert!(vad.classify(0.1));
        // And silence again is not.
        assert!(!vad.classify(0.001));
    }

    #[test]
    fn rms_scales_with_amplitude() {
        let quiet = vec![0.01; 100];
        let loud = vec![0.5; 100];
        assert!(rms(&loud) > rms(&quiet));
        assert!((rms(&quiet) - 0.01).abs() < 1e-4);
    }
}
