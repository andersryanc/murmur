//! Plays a short generated tone through `play_wav`, twice, back to back.
//!
//! `cargo run -p squawk-audio --example beep`

use std::io::Cursor;
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let rate = 44_100;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav = Vec::new();
    {
        let mut w = hound::WavWriter::new(Cursor::new(&mut wav), spec)?;
        let n = rate as usize / 5; // 0.2 s
        for i in 0..n {
            let t = i as f32 / rate as f32;
            let fade = 1.0 - i as f32 / n as f32; // decay so it ends without a click
            let s = (std::f32::consts::TAU * 880.0 * t).sin() * fade * 0.5;
            w.write_sample((s * i16::MAX as f32) as i16)?;
        }
        w.finalize()?;
    }

    let t = Instant::now();
    squawk_audio::play_wav(wav, 0.8);
    println!("play_wav returned after {:?}", t.elapsed());
    // The process must outlive the sound; the app does, this example has to wait.
    std::thread::sleep(Duration::from_millis(600));

    println!("done");
    Ok(())
}
