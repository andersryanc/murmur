//! Short sound effects (the start and stop sounds).
//!
//! Uses cpal directly rather than rodio: the sounds are tiny, already decoded into memory, and
//! this keeps a single cpal version in the build.

use std::io::Cursor;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, StreamConfig};

use crate::convert::{mix_to_mono, resample};

/// Extra time the stream stays open after the last sample, so the device's buffer drains
/// instead of being cut off mid-sound.
const TAIL: Duration = Duration::from_millis(150);

/// Plays an in-memory WAV on the default output device without blocking. The sound runs on its
/// own short-lived thread, so it finishes even though this returns immediately. Failures (no
/// output device, a malformed WAV) are logged, since a missing click shouldn't stop dictation.
pub fn play_wav(wav: Vec<u8>, volume: f32) {
    let spawned = std::thread::Builder::new().name("murmur-sound".into()).spawn(move || {
        if let Err(e) = play_blocking(&wav, volume) {
            log::warn!("playing sound failed: {e:#}");
        }
    });
    if let Err(e) = spawned {
        log::warn!("starting the sound thread failed: {e}");
    }
}

fn play_blocking(wav: &[u8], volume: f32) -> Result<()> {
    let (mono, wav_rate) = decode(wav)?;
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow!("no output device"))?;
    let supported = device.default_output_config().context("reading the output format")?;
    let format = supported.sample_format();
    let config = supported.config();

    let gain = volume.clamp(0.0, 1.0);
    let mut sound = resample(&mono, wav_rate, config.sample_rate)?;
    sound.iter_mut().for_each(|s| *s *= gain);
    let duration = Duration::from_secs_f64(sound.len() as f64 / config.sample_rate as f64);

    let stream = match format {
        SampleFormat::I8 => build::<i8>(&device, &config, sound),
        SampleFormat::I16 => build::<i16>(&device, &config, sound),
        SampleFormat::I24 => build::<cpal::I24>(&device, &config, sound),
        SampleFormat::I32 => build::<i32>(&device, &config, sound),
        SampleFormat::I64 => build::<i64>(&device, &config, sound),
        SampleFormat::U8 => build::<u8>(&device, &config, sound),
        SampleFormat::U16 => build::<u16>(&device, &config, sound),
        SampleFormat::U24 => build::<cpal::U24>(&device, &config, sound),
        SampleFormat::U32 => build::<u32>(&device, &config, sound),
        SampleFormat::U64 => build::<u64>(&device, &config, sound),
        SampleFormat::F32 => build::<f32>(&device, &config, sound),
        SampleFormat::F64 => build::<f64>(&device, &config, sound),
        other => return Err(anyhow!("unsupported output sample format {other}")),
    }?;
    stream.play().context("starting playback")?;
    std::thread::sleep(duration + TAIL);
    Ok(())
}

/// Decodes a WAV (any channel count, integer or float) to mono `f32`.
pub(crate) fn decode(wav: &[u8]) -> Result<(Vec<f32>, u32)> {
    let reader = hound::WavReader::new(Cursor::new(wav)).context("reading WAV")?;
    let spec = reader.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.into_samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .into_samples::<i32>()
                .map(|s| s.map(|s| s as f32 * scale))
                .collect::<Result<_, _>>()?
        }
    };
    Ok((mix_to_mono(&interleaved, spec.channels as usize), spec.sample_rate))
}

/// Builds an output stream that plays `sound` on every channel, then silence.
fn build<T>(device: &cpal::Device, config: &StreamConfig, sound: Vec<f32>) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let mut pos = 0;
    let stream = device.build_output_stream::<T, _, _>(
        *config,
        move |out: &mut [T], _| {
            for frame in out.chunks_mut(channels) {
                let s = sound.get(pos).copied().unwrap_or(0.0);
                pos += 1;
                frame.iter_mut().for_each(|o| *o = s.to_sample::<T>());
            }
        },
        |e| log::warn!("sound stream error: {e}"),
        None,
    )?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_16_bit_stereo_wav_to_mono() {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 22_050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut bytes = Vec::new();
        {
            let mut w = hound::WavWriter::new(Cursor::new(&mut bytes), spec).unwrap();
            for _ in 0..100 {
                w.write_sample(16_384i16).unwrap();
                w.write_sample(0i16).unwrap();
            }
            w.finalize().unwrap();
        }
        let (mono, rate) = decode(&bytes).unwrap();
        assert_eq!(rate, 22_050);
        assert_eq!(mono.len(), 100);
        assert!(mono.iter().all(|&s| (s - 0.25).abs() < 1e-4));
    }
}
