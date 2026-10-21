//! Speech to text: NVIDIA Parakeet TDT 0.6B v3 (int8 ONNX) through sherpa-onnx, on the CPU.
//!
//! The same model the Mac app runs on the Neural Engine and the Swift Linux build runs through
//! sherpa-onnx's C API. sherpa-onnx and onnxruntime are linked statically (the `sherpa-onnx`
//! crate's default), so there are no shared libraries to ship; the ~480 MB model is downloaded on
//! first run instead of bundled.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, anyhow, bail};
use sherpa_onnx::{OfflineModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

/// The model `download_model` fetches: Parakeet TDT v3 (multilingual), quantized to int8.
pub const DEFAULT_MODEL: &str = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8";

/// sherpa-onnx publishes its converted models as assets on this release.
const MODEL_RELEASE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models";

/// What Parakeet needs from the model folder. The archive also holds test WAVs and a README,
/// which we don't need but leave in place.
const MODEL_FILES: [&str; 4] = [
    "encoder.int8.onnx",
    "decoder.int8.onnx",
    "joiner.int8.onnx",
    "tokens.txt",
];

/// Everything the app transcribes is 16 kHz mono (see CONTRACTS.md), which is also what
/// Parakeet was trained on, so sherpa-onnx never has to resample.
const SAMPLE_RATE: i32 = 16_000;

/// Where the app keeps the default model.
pub fn default_model_dir() -> PathBuf {
    murmur_core::paths::models_dir().join(DEFAULT_MODEL)
}

/// Whether `dir` holds every file the recognizer loads. `download_model` only moves a model into
/// place once it is complete, so this is enough to tell a finished download from none.
pub fn model_present(dir: &Path) -> bool {
    MODEL_FILES.iter().all(|name| dir.join(name).is_file())
}

/// Downloads `DEFAULT_MODEL` into `dir`, replacing whatever is there.
///
/// The archive is decompressed and unpacked as it streams in, into a temporary folder next to
/// `dir` (same filesystem, so the final rename is atomic). Only a complete model is renamed into
/// place, so an interrupted download never looks finished. `progress` gets the compressed bytes
/// received so far and the total, if the server sent one. Blocking: run it off the main thread.
pub fn download_model(dir: &Path, mut progress: impl FnMut(u64, Option<u64>)) -> anyhow::Result<()> {
    let parent = dir
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| anyhow!("model folder has no parent: {}", dir.display()))?;
    let name = dir
        .file_name()
        .ok_or_else(|| anyhow!("model folder has no name: {}", dir.display()))?
        .to_string_lossy();
    fs::create_dir_all(parent).with_context(|| format!("couldn't create {}", parent.display()))?;

    // A leftover from an earlier, interrupted download is just garbage now.
    let staging = parent.join(format!(".{name}.download"));
    remove_if_present(&staging)?;
    fs::create_dir_all(&staging)?;

    let result = fetch_and_unpack(&staging, &mut progress).and_then(|unpacked| {
        if !model_present(&unpacked) {
            bail!("the downloaded archive is missing model files");
        }
        remove_if_present(dir)?;
        fs::rename(&unpacked, dir).with_context(|| format!("couldn't move the model into {}", dir.display()))
    });
    let _ = fs::remove_dir_all(&staging);
    result
}

/// Streams the archive into `staging` and returns the folder it unpacked to.
fn fetch_and_unpack(staging: &Path, progress: &mut dyn FnMut(u64, Option<u64>)) -> anyhow::Result<PathBuf> {
    let url = format!("{MODEL_RELEASE_URL}/{DEFAULT_MODEL}.tar.bz2");
    let mut response = ureq::get(&url)
        .call()
        .with_context(|| format!("couldn't download the speech model from {url}"))?;
    let total = response.body().content_length();
    progress(0, total);

    let reader = Progress {
        inner: response.body_mut().as_reader(),
        read: 0,
        total,
        callback: progress,
    };
    tar::Archive::new(bzip2::read::BzDecoder::new(reader))
        .unpack(staging)
        .context("couldn't download and unpack the speech model")?;

    // The archive holds one top-level folder named after the model.
    let unpacked = staging.join(DEFAULT_MODEL);
    if unpacked.is_dir() {
        Ok(unpacked)
    } else {
        Ok(staging.to_path_buf())
    }
}

fn remove_if_present(path: &Path) -> anyhow::Result<()> {
    match fs::remove_dir_all(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("couldn't remove {}", path.display()))
        }
        _ => Ok(()),
    }
}

/// Counts the bytes passing through, for the download's progress callback.
struct Progress<'a, R> {
    inner: R,
    read: u64,
    total: Option<u64>,
    callback: &'a mut dyn FnMut(u64, Option<u64>),
}

impl<R: Read> Read for Progress<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        (self.callback)(self.read, self.total);
        Ok(n)
    }
}

/// Parakeet loaded and ready. Loading takes a second or two, so the app keeps one around.
pub struct Transcriber {
    // sherpa-onnx marks its recognizer Send + Sync, but dictation transcribes one clip at a time
    // anyway, so a Mutex costs nothing and doesn't lean on that promise.
    recognizer: Mutex<OfflineRecognizer>,
}

impl Transcriber {
    /// Loads the model in `model_dir` to run on `threads` CPU threads.
    pub fn new(model_dir: &Path, threads: usize) -> anyhow::Result<Self> {
        let missing: Vec<&str> = MODEL_FILES
            .into_iter()
            .filter(|name| !model_dir.join(name).is_file())
            .collect();
        if !missing.is_empty() {
            bail!(
                "speech model not found in {}: missing {}. Download it first.",
                model_dir.display(),
                missing.join(", ")
            );
        }

        let file = |name: &str| Some(model_dir.join(name).to_string_lossy().into_owned());
        let config = OfflineRecognizerConfig {
            decoding_method: Some("greedy_search".into()),
            model_config: OfflineModelConfig {
                transducer: OfflineTransducerModelConfig {
                    encoder: file(MODEL_FILES[0]),
                    decoder: file(MODEL_FILES[1]),
                    joiner: file(MODEL_FILES[2]),
                },
                tokens: file(MODEL_FILES[3]),
                model_type: Some("nemo_transducer".into()),
                provider: Some("cpu".into()),
                num_threads: threads.clamp(1, i32::MAX as usize) as i32,
                ..Default::default()
            },
            // Default: 16 kHz, 80-dim fbank features, which is what Parakeet expects.
            ..Default::default()
        };

        let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
            anyhow!(
                "sherpa-onnx couldn't load the speech model from {}",
                model_dir.display()
            )
        })?;
        Ok(Self {
            recognizer: Mutex::new(recognizer),
        })
    }

    /// 16 kHz mono samples in, text out (trimmed; empty for silence).
    pub fn transcribe(&self, samples: &[f32]) -> anyhow::Result<String> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        // A panic mid-decode leaves nothing half-updated on our side, so a poisoned lock is safe
        // to keep using.
        let recognizer = self.recognizer.lock().unwrap_or_else(|e| e.into_inner());
        let stream = recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE, samples);
        recognizer.decode(&stream);
        let result = stream
            .get_result()
            .ok_or_else(|| anyhow!("sherpa-onnx returned no transcription result"))?;
        Ok(result.text.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("murmur-stt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn transcriber_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Transcriber>();
    }

    #[test]
    fn default_model_dir_is_named_after_the_model() {
        assert!(default_model_dir().ends_with(DEFAULT_MODEL));
    }

    #[test]
    fn model_present_needs_every_file() {
        let dir = temp_dir("present");
        assert!(!model_present(&dir));
        for name in &MODEL_FILES[..3] {
            fs::write(dir.join(name), b"").unwrap();
        }
        assert!(!model_present(&dir));
        fs::write(dir.join("tokens.txt"), b"").unwrap();
        assert!(model_present(&dir));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_model_names_the_missing_files() {
        let dir = temp_dir("missing");
        fs::write(dir.join("tokens.txt"), b"").unwrap();
        let message = Transcriber::new(&dir, 1).err().unwrap().to_string();
        assert!(message.contains("encoder.int8.onnx"), "{message}");
        assert!(!message.contains("tokens.txt"), "{message}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_folder_is_an_error() {
        assert!(Transcriber::new(Path::new("/nonexistent/murmur-model"), 1).is_err());
    }

    /// Runs only where the model has been downloaded (`cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn silence_transcribes_to_nothing_much() {
        let transcriber = Transcriber::new(&default_model_dir(), 2).unwrap();
        assert_eq!(transcriber.transcribe(&[]).unwrap(), "");
        let text = transcriber.transcribe(&vec![0.0; 16_000]).unwrap();
        assert!(text.len() < 10, "{text}");
    }
}
