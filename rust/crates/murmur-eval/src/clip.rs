//! The clips the eval scores: `<name>.wav` recordings in `eval/clips/`, each with an optional
//! `<name>.json` sidecar saying what was said and what should have been typed.

use std::path::{Path, PathBuf};

use anyhow::Context;
use murmur_core::TargetContext;
use serde::{Deserialize, Serialize};

/// `rust/eval/`, found from this crate's location so the eval works from any directory.
pub fn eval_dir() -> PathBuf {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new("."));
    crates.parent().unwrap_or(crates).join("eval")
}

pub fn clips_dir() -> PathBuf {
    eval_dir().join("clips")
}

pub fn results_dir() -> PathBuf {
    eval_dir().join("results")
}

/// The sidecar, `<name>.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ClipMeta {
    /// Exactly what was said, word for word. Scores the raw transcript (WER).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// The text you'd want typed. For reading against the cleanup output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intended: Option<String>,
    #[serde(default)]
    pub context: TargetContext,
}

pub struct Clip {
    pub name: String,
    pub audio: PathBuf,
    pub meta: ClipMeta,
}

/// Every `.wav` in `dir`, by name, each with its sidecar if there is one.
pub fn load_all(dir: &Path) -> anyhow::Result<Vec<Clip>> {
    let entries = std::fs::read_dir(dir).with_context(|| format!("couldn't read {}", dir.display()))?;
    let mut audio: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("wav")))
        .collect();
    audio.sort();
    audio
        .into_iter()
        .map(|audio| {
            let name = audio.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            let sidecar = dir.join(format!("{name}.json"));
            let meta = if sidecar.exists() {
                serde_json::from_slice(&std::fs::read(&sidecar)?)
                    .with_context(|| format!("{} isn't a clip sidecar", sidecar.display()))?
            } else {
                ClipMeta::default()
            };
            Ok(Clip { name, audio, meta })
        })
        .collect()
}

pub fn save_meta(meta: &ClipMeta, name: &str, dir: &Path) -> anyhow::Result<()> {
    let path = dir.join(format!("{name}.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(meta)?)
        .with_context(|| format!("couldn't write {}", path.display()))
}

/// Writes 16 kHz mono samples as a 32-bit float WAV.
pub fn save_wav(samples: &[f32], path: &Path) -> anyhow::Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: murmur_core::SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer =
        hound::WavWriter::create(path, spec).with_context(|| format!("couldn't write {}", path.display()))?;
    for &sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}
