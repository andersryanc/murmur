//! `murmur transcribe <file.wav>`: the whole pipeline on a file, without the daemon or the
//! microphone, printing each stage and how long it took. For checking the model, the API key
//! and the cleanup from a terminal.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use murmur_core::{Settings, TargetContext};

use super::{config, option};

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let (path, context, cleanup) = parse(args)?;
    let config = config::load();
    let samples = murmur_audio::load_wav(Path::new(path))?;

    let start = Instant::now();
    let mut last = String::new();
    let transcriber = murmur_engine::load_model(&config.options, |status| {
        // Download progress repeats the same prefix; print each new line once.
        if status != last {
            eprintln!("{status}");
            last = status;
        }
    })?;
    println!("Model loaded in {}", seconds(start.elapsed()));

    let start = Instant::now();
    let raw = transcriber.transcribe(&samples)?;
    println!("Speech ({}): {raw}", seconds(start.elapsed()));
    if !cleanup {
        return Ok(());
    }

    let dictionary = murmur_engine::load_dictionary();
    let corrected = dictionary.apply_replacements(&raw);
    let settings = &config.settings;
    let cleaner = murmur_engine::cleaner(settings.cleanup, &settings.ollama_model, dictionary.vocabulary())
        .context("Cleanup failed")?;
    let Some(cleaner) = cleaner else {
        println!("Cleanup is off in {}", Settings::path().display());
        return Ok(());
    };
    let start = Instant::now();
    let cleaned = cleaner.clean(&corrected, context).context("Cleanup failed")?;
    println!(
        "Cleanup [{}, {}] ({}): {cleaned}",
        cleaner.id(),
        context.as_str(),
        seconds(start.elapsed())
    );
    Ok(())
}

/// The file, the `--context` (plain by default) and whether to clean up.
fn parse(args: &[String]) -> anyhow::Result<(&str, TargetContext, bool)> {
    let mut path = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--context" => {
                iter.next();
            }
            "--no-cleanup" => {}
            flag if flag.starts_with("--") => bail!("Unknown option {flag}"),
            file => path = path.or(Some(file)),
        }
    }
    let Some(path) = path else {
        bail!("Usage: murmur transcribe <file.wav> [--context plain|chat|email|notes|code] [--no-cleanup]");
    };
    let context = match option(args, "--context") {
        None => TargetContext::Plain,
        Some(name) => TargetContext::parse(name).with_context(|| format!("Unknown context {name}"))?,
    };
    Ok((path, context, !args.iter().any(|a| a == "--no-cleanup")))
}

pub fn seconds(duration: Duration) -> String {
    format!("{:.2} s", duration.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_the_file_wherever_it_is() {
        let a = args(&["--context", "chat", "clip.wav", "--no-cleanup"]);
        assert_eq!(parse(&a).unwrap(), ("clip.wav", TargetContext::Chat, false));
        let a = args(&["clip.wav"]);
        assert_eq!(parse(&a).unwrap(), ("clip.wav", TargetContext::Plain, true));
        assert!(parse(&args(&["--no-cleanup"])).is_err());
        assert!(parse(&args(&["clip.wav", "--context", "shouting"])).is_err());
    }
}
