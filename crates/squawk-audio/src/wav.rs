//! Reading WAV files, for `squawkctl transcribe`.

use std::path::Path;

use anyhow::{Context, Result};

use crate::convert::resample;
use crate::playback::decode;

/// Reads a WAV file (16-bit, 24-bit, 32-bit or float, any channel count and rate) as 16 kHz
/// mono, ready for speech recognition.
pub fn load_wav(path: &Path) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path).with_context(|| format!("couldn't read {}", path.display()))?;
    let (mono, rate) = decode(&bytes).with_context(|| format!("{} isn't a WAV file we can read", path.display()))?;
    resample(&mono, rate, squawk_core::SAMPLE_RATE)
}
