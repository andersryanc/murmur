//! A cheap check for whether a recording contains any speech at all, and the level meter.
//!
//! Speech models guess words from silence or room noise (Parakeet tends to hear "Yeah."), so a
//! quick tap of the hotkey would otherwise paste a phantom word. Murmur skips transcription when
//! there's too little voiced audio.

/// Sample rate every speech engine takes: 16 kHz mono.
pub const SAMPLE_RATE: u32 = 16_000;

/// 20 ms frames louder than this count as voiced. Speech into a laptop or headset mic is usually
/// -40 to -15 dBFS; a quiet room is around -60 dBFS.
pub const THRESHOLD_DB: f32 = -50.0;

/// Less voiced audio than this (seconds) is treated as no speech. A quick, soft "yes" has
/// ~120 ms of voice; a keyboard click ~20 ms.
pub const MINIMUM_SPEECH: f64 = 0.08;

/// Seconds of audio above `THRESHOLD_DB`, for 16 kHz mono samples.
pub fn voiced_duration(samples: &[f32]) -> f64 {
    let frame = (SAMPLE_RATE as usize) / 50;
    let threshold = 10f32.powf(THRESHOLD_DB / 20.0);
    let voiced = samples.chunks_exact(frame).filter(|f| rms(f) > threshold).count();
    voiced as f64 * 0.02
}

pub fn contains_speech(samples: &[f32]) -> bool {
    voiced_duration(samples) >= MINIMUM_SPEECH
}

/// Loudness for the level meter: roughly -50 dB...0 dB mapped to 0...1.
pub fn level(samples: &[f32]) -> f32 {
    let db = 20.0 * rms(samples).max(1e-6).log10();
    ((db + 50.0) / 50.0).clamp(0.0, 1.0)
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_and_a_tone() {
        assert!(!contains_speech(&vec![0.0; 16_000]));
        let tone: Vec<f32> = (0..16_000).map(|i| 0.1 * (i as f32 * 0.1).sin()).collect();
        assert!(contains_speech(&tone));
        assert!((voiced_duration(&tone) - 1.0).abs() < 0.03);
        assert_eq!(level(&[]), 0.0);
        assert!(level(&tone) > 0.6);
    }
}
