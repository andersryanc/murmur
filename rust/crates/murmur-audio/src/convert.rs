//! Format conversion shared by capture and playback: channel mix-down and resampling.

use anyhow::{Context, Result};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

/// Averages interleaved frames down to one channel. Averaging (rather than taking the first
/// channel) keeps speech from a mic array or a stereo interface whose voice is only on one side,
/// at the cost of halving the level of a one-sided signal, which the recogniser doesn't mind.
pub fn mix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    let scale = 1.0 / channels as f32;
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() * scale)
        .collect()
}

/// Resamples mono audio with rubato's FFT resampler, which band-limits properly so energy above
/// the new Nyquist frequency doesn't fold back into the speech band as it would with naive
/// decimation. The resampler's own delay is trimmed, so the output lines up with the input and
/// has `len * to / from` samples (rounded up).
pub fn resample(samples: &[f32], from: u32, to: u32) -> Result<Vec<f32>> {
    if from == to || samples.is_empty() {
        return Ok(samples.to_vec());
    }
    let mut resampler =
        Fft::<f32>::new(from as usize, to as usize, 1024, 1, FixedSync::Input).context("creating resampler")?;
    let input = InterleavedSlice::new(samples, 1, samples.len()).context("wrapping input")?;
    let mut out = vec![0.0; resampler.process_all_needed_output_len(samples.len())];
    let out_len = out.len();
    let mut output = InterleavedSlice::new_mut(&mut out, 1, out_len).context("wrapping output")?;
    let (_, produced) = resampler
        .process_all_into_buffer(&input, &mut output, samples.len(), None)
        .context("resampling")?;
    out.truncate(produced);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn sine(freq: f32, rate: u32, seconds: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize)
            .map(|i| (TAU * freq * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    /// Frequency estimated from rising zero crossings, which is plenty for a clean sine.
    fn frequency(samples: &[f32], rate: u32) -> f32 {
        let crossings = samples.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        crossings as f32 * rate as f32 / samples.len() as f32
    }

    #[test]
    fn mono_passes_through() {
        assert_eq!(mix_to_mono(&[0.1, 0.2, 0.3], 1), vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn stereo_frames_are_averaged() {
        assert_eq!(mix_to_mono(&[1.0, 0.0, 0.5, 0.5, -1.0, 1.0], 2), vec![0.5, 0.5, 0.0]);
    }

    #[test]
    fn many_channels_and_a_partial_frame() {
        // A trailing partial frame can't be mixed and is dropped.
        let mono = mix_to_mono(&[0.3, 0.3, 0.3, 0.3, 0.6, 0.0, 0.0, 0.0, 0.1], 4);
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.3).abs() < 1e-6);
        assert!((mono[1] - 0.15).abs() < 1e-6);
    }

    #[test]
    fn same_rate_is_untouched() {
        let s = sine(440.0, 16_000, 0.1);
        assert_eq!(resample(&s, 16_000, 16_000).unwrap(), s);
    }

    #[test]
    fn stereo_48k_sine_becomes_16k_mono_with_same_pitch() {
        let left = sine(440.0, 48_000, 1.0);
        let interleaved: Vec<f32> = left.iter().flat_map(|&s| [s, s]).collect();
        let mono = mix_to_mono(&interleaved, 2);
        let out = resample(&mono, 48_000, 16_000).unwrap();

        assert!((out.len() as i64 - 16_000).abs() <= 1, "len {}", out.len());
        let f = frequency(&out, 16_000);
        assert!((f - 440.0).abs() < 2.0, "frequency {f}");
        // Amplitude survives: the middle of the clip should still peak near 0.5.
        let peak = out[4_000..12_000].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.02, "peak {peak}");
    }

    #[test]
    fn odd_rate_44_1k_resamples_to_the_right_length() {
        let s = sine(1000.0, 44_100, 0.5);
        let out = resample(&s, 44_100, 16_000).unwrap();
        assert!((out.len() as i64 - 8_000).abs() <= 1, "len {}", out.len());
        assert!((frequency(&out, 16_000) - 1000.0).abs() < 4.0);
    }

    #[test]
    fn content_above_new_nyquist_is_filtered_out() {
        // 10 kHz is above 8 kHz Nyquist; nearest-neighbour would alias it to 6 kHz at full level.
        let s = sine(10_000.0, 48_000, 0.5);
        let out = resample(&s, 48_000, 16_000).unwrap();
        let rms = (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt();
        assert!(rms < 0.01, "rms {rms}");
    }
}
