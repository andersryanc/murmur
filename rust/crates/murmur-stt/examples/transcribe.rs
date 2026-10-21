//! Transcribes WAV files with the default model, downloading it first if it's missing, and prints
//! how long loading and each transcription took.
//!
//! cargo run --release -p murmur-stt --example transcribe -- <file.wav>...

use std::io::Write;
use std::path::Path;
use std::time::Instant;

use anyhow::Context;
use murmur_stt::{Transcriber, default_model_dir, download_model, model_present};

fn main() -> anyhow::Result<()> {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        anyhow::bail!("usage: transcribe <file.wav>...");
    }

    let dir = default_model_dir();
    if !model_present(&dir) {
        println!("Downloading {} to {}", murmur_stt::DEFAULT_MODEL, dir.display());
        let mut last_mb = u64::MAX;
        download_model(&dir, |done, total| {
            let mb = done / 1_000_000;
            if mb != last_mb {
                last_mb = mb;
                match total {
                    Some(total) => print!("\r  {mb} / {} MB", total / 1_000_000),
                    None => print!("\r  {mb} MB"),
                }
                let _ = std::io::stdout().flush();
            }
        })?;
        println!();
    }

    // Up to 4, as the Swift Linux build used; MURMUR_THREADS overrides it for measuring.
    let threads = std::env::var("MURMUR_THREADS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get().min(4)));
    let start = Instant::now();
    let transcriber = Transcriber::new(&dir, threads)?;
    println!("Loaded in {:.2} s ({threads} threads)", start.elapsed().as_secs_f64());

    for file in &files {
        let samples = read_16k_mono(Path::new(file))?;
        let seconds = samples.len() as f64 / 16_000.0;
        let start = Instant::now();
        let text = transcriber.transcribe(&samples)?;
        let took = start.elapsed().as_secs_f64();
        println!(
            "\n{file}\n  {seconds:.1} s of audio in {:.0} ms ({:.0}x real time)\n  {text}",
            took * 1000.0,
            seconds / took
        );
    }
    Ok(())
}

/// Reads a WAV, mixes it to mono and resamples it to 16 kHz by linear interpolation, which is
/// plenty for speech going into a recognizer.
fn read_16k_mono(path: &Path) -> anyhow::Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path).with_context(|| format!("can't read {}", path.display()))?;
    let spec = reader.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|s| s as f32 * scale))
                .collect::<Result<_, _>>()?
        }
    };
    let channels = spec.channels as usize;
    let mono: Vec<f32> = interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();

    if spec.sample_rate == 16_000 || mono.is_empty() {
        return Ok(mono);
    }
    let step = spec.sample_rate as f64 / 16_000.0;
    let count = (mono.len() as f64 / step) as usize;
    Ok((0..count)
        .map(|i| {
            let pos = i as f64 * step;
            let j = pos as usize;
            let frac = (pos - j as f64) as f32;
            let next = mono.get(j + 1).copied().unwrap_or(mono[j]);
            mono[j] + (next - mono[j]) * frac
        })
        .collect())
}
