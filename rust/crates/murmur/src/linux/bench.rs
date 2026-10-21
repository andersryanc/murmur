//! `murmur bench`: cleans each of the fixed test dictations (`murmur_core::bench::CASES`)
//! `--runs` times and prints the timings and outputs. Consecutive cases use different styles,
//! so with Ollama the first run of each pays for reading that style's prompt and the rest show
//! the cached speed; both are reported.

use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use murmur_core::{OllamaCleaner, Settings, TargetContext, TextCleaner};

use super::{config, option};

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let runs = match option(args, "--runs") {
        None => 3,
        Some(n) => n
            .parse::<usize>()
            .with_context(|| format!("--runs {n} isn't a number"))?
            .max(1),
    };
    let settings = config::load().settings;
    let dictionary = murmur_engine::load_dictionary();
    let cleaner: Box<dyn TextCleaner> = match option(args, "--model") {
        Some(model) => Box::new(OllamaCleaner::new(model.to_string(), dictionary.vocabulary())),
        None => match murmur_engine::cleaner(settings.cleanup, &settings.ollama_model, dictionary.vocabulary())? {
            Some(cleaner) => cleaner,
            None => bail!(
                "Cleanup is off in {}; pass --model to bench an Ollama model",
                Settings::path().display()
            ),
        },
    };

    println!("Cleaner: {}", cleaner.id());
    let start = Instant::now();
    cleaner.prepare().context("Cleanup failed")?;
    cleaner
        .clean("warm up", TargetContext::Plain)
        .context("Cleanup failed")?;
    println!("Load and warm-up: {:.1} s\n", start.elapsed().as_secs_f64());

    let mut firsts = Vec::new();
    let mut repeats = Vec::new();
    for (context, text) in murmur_core::bench::CASES {
        let mut times = Vec::new();
        let mut output = String::new();
        for _ in 0..runs {
            let start = Instant::now();
            output = cleaner
                .clean(&dictionary.apply_replacements(text), context)
                .context("Cleanup failed")?;
            times.push(start.elapsed());
        }
        firsts.push(times[0]);
        repeats.extend_from_slice(&times[1..]);
        let mut line = format!("[{}] {:.2} s", context.as_str(), times[0].as_secs_f64());
        if times.len() > 1 {
            line += &format!(", then {:.2} s", median(&times[1..]).as_secs_f64());
        }
        println!("{line}");
        for row in output.split('\n') {
            println!("  {row}");
        }
        println!();
    }
    let mut summary = format!("Median: {:.2} s after a style change", median(&firsts).as_secs_f64());
    if !repeats.is_empty() {
        summary += &format!(", {:.2} s repeated", median(&repeats).as_secs_f64());
    }
    println!("{summary}");
    Ok(())
}

/// The middle value (the upper one of an even count, as the Swift version did).
fn median(values: &[Duration]) -> Duration {
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted[sorted.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_takes_the_upper_middle() {
        let ms = |v: &[u64]| v.iter().map(|&n| Duration::from_millis(n)).collect::<Vec<_>>();
        assert_eq!(median(&ms(&[30, 10, 20])), Duration::from_millis(20));
        assert_eq!(median(&ms(&[40, 10, 30, 20])), Duration::from_millis(30));
    }
}
