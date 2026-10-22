//! Records 3 s from the default microphone, printing levels and how long `Recorder::start`
//! took, then writes the 16 kHz result to `recording.wav`.
//!
//! `cargo run -p murmur-audio --example record [-- "Device name"]`

use std::io::Write;
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let wanted = std::env::args().nth(1);
    println!("input devices: {:?}", murmur_audio::input_devices());
    println!("default: {:?}", murmur_audio::default_input_device());

    // Warm-up run is not done on purpose: the first start is what a user hits after launch.
    for attempt in 1..=3 {
        let t = Instant::now();
        let rec = murmur_audio::Recorder::start(wanted.as_deref(), |_| {})?;
        println!("start #{attempt}: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
        rec.stop();
    }

    let t = Instant::now();
    let rec = murmur_audio::Recorder::start(wanted.as_deref(), |l| {
        let bar = "#".repeat((l * 40.0) as usize);
        print!("\rlevel {l:.2} {bar:<40}");
        let _ = std::io::stdout().flush();
    })?;
    println!(
        "recording start: {:.1} ms; speak for 3 s",
        t.elapsed().as_secs_f64() * 1000.0
    );
    std::thread::sleep(Duration::from_secs(3));
    let t = Instant::now();
    let samples = rec.stop();
    println!("\nstop + resample: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);

    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    println!(
        "{} samples ({:.2} s at 16 kHz), peak {peak:.3}, level {:.2}",
        samples.len(),
        samples.len() as f64 / 16_000.0,
        murmur_core::voice::level(&samples)
    );
    if peak == 0.0 {
        println!("all silent: the terminal may lack microphone permission");
    }

    let path = std::env::current_dir()?.join("recording.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: murmur_core::voice::SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&path, spec)?;
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    w.finalize()?;
    println!("wrote {}", path.display());
    Ok(())
}
