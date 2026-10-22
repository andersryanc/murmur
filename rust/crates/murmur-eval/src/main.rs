//! `murmur-eval`: scores speech-to-text and the cleanup pass against recordings of your voice,
//! with the same engine, prompt and Dictionary the app uses. Clips live in `rust/eval/clips/`
//! and reports go to `rust/eval/results/`.

mod clip;
mod report;

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use murmur_core::{ClaudeCleaner, Dictionary, OllamaCleaner, TargetContext, TextCleaner};
use murmur_stt::Transcriber;

use clip::{Clip, ClipMeta, clips_dir};
use report::{ClipResult, EngineResult, ms, pct, word_error_rate};

const USAGE: &str = "\
murmur-eval: score speech-to-text and the cleanup pass on your own voice.

USAGE
  murmur-eval record <name> [--context plain|chat|email|notes|code] [--mic <name>]
      Record from the microphone into eval/clips/<name>.wav until you press Return, then type
      what you said.

  murmur-eval run [--models <dir>,<dir>] [--threads <n>] [--no-cleanup] [cleaner options]
      Transcribe every clip, clean each transcript, and write a report to eval/results/.

  murmur-eval clean \"<raw transcript>\" [--context notes] [cleaner options]
      Run only the cleanup pass on a string.

  murmur-eval bench [--runs 3] [cleaner options]
      Time a cleaner on built-in test dictations and show its output.

  murmur-eval vad <file.wav>…
      Show how much voiced audio each file has, and whether Murmur would transcribe it.

  murmur-eval devices
      List microphones (* = system default).

CLEANER OPTIONS
  --cleaner claude|ollama:<model>   default claude (claude-haiku-5-5)
  --prompt <file>                   a draft system prompt instead of the built-in one
  --thinking, --effort <level>, --plain-reply   Claude request options

MODELS
  --models takes sherpa-onnx model folders; the default is the one the app uses, downloaded on
  first use. Each folder is an engine in the report, named after the folder.

Every command uses your Murmur Dictionary; --dictionary <file> picks another, --dictionary none
uses none. The API key comes from ANTHROPIC_API_KEY or the key store, as in the app.
";

/// `--key value` options, `--flag`s and positionals.
struct Args {
    positionals: Vec<String>,
    options: HashMap<String, String>,
    flags: HashSet<String>,
}

/// The options that take a value; any other `--name` is a flag.
const VALUED: [&str; 9] = [
    "context",
    "mic",
    "models",
    "threads",
    "cleaner",
    "prompt",
    "effort",
    "runs",
    "dictionary",
];

impl Args {
    fn parse(raw: impl IntoIterator<Item = String>) -> anyhow::Result<Self> {
        let mut args = Self {
            positionals: Vec::new(),
            options: HashMap::new(),
            flags: HashSet::new(),
        };
        let mut iter = raw.into_iter();
        while let Some(arg) = iter.next() {
            let Some(key) = arg.strip_prefix("--") else {
                args.positionals.push(arg);
                continue;
            };
            if VALUED.contains(&key) {
                let Some(value) = iter.next() else {
                    bail!("--{key} needs a value")
                };
                args.options.insert(key.to_string(), value);
            } else {
                args.flags.insert(key.to_string());
            }
        }
        Ok(args)
    }

    fn option(&self, key: &str) -> Option<&str> {
        self.options.get(key).map(String::as_str)
    }

    fn flag(&self, key: &str) -> bool {
        self.flags.contains(key)
    }

    fn context(&self) -> anyhow::Result<TargetContext> {
        let Some(raw) = self.option("context") else {
            return Ok(TargetContext::Plain);
        };
        TargetContext::parse(raw).with_context(|| {
            let all: Vec<&str> = TargetContext::ALL.iter().map(|c| c.as_str()).collect();
            format!("unknown context {raw:?}; use one of: {}", all.join(", "))
        })
    }

    fn number(&self, key: &str) -> anyhow::Result<Option<usize>> {
        self.option(key)
            .map(|n| n.parse().with_context(|| format!("--{key} {n} isn't a number")))
            .transpose()
    }

    /// The app's Dictionary, another file, or none, so results match real use.
    fn dictionary(&self) -> anyhow::Result<Dictionary> {
        match self.option("dictionary") {
            Some("none") => Ok(Dictionary::default()),
            Some(path) => Dictionary::load(Path::new(path)).with_context(|| format!("couldn't read {path}")),
            None => Ok(murmur_engine::load_dictionary()),
        }
    }

    /// The cleanup pass the options ask for. `None` when it's Claude and there's no API key.
    fn cleaner(&self, dictionary: &Dictionary) -> anyhow::Result<Option<Box<dyn TextCleaner>>> {
        let system = match self.option("prompt") {
            Some(path) => Some(std::fs::read_to_string(path).with_context(|| format!("couldn't read {path}"))?),
            None => None,
        };
        let vocabulary = dictionary.vocabulary();
        let choice = self.option("cleaner").unwrap_or("claude");

        if let Some(model) = choice.strip_prefix("ollama:") {
            let mut cleaner = OllamaCleaner::new(model.to_string(), vocabulary);
            if let Some(system) = system {
                cleaner.system_prompt = system;
            }
            return Ok(Some(Box::new(cleaner)));
        }
        if choice != "claude" {
            bail!("unknown cleaner {choice:?}; use claude or ollama:<model>");
        }
        let Some(key) = murmur_core::api_key::load() else {
            return Ok(None);
        };
        let mut cleaner = ClaudeCleaner::new(key, vocabulary);
        if let Some(system) = system {
            cleaner.system_prompt = system;
        }
        cleaner.options.thinking = self.flag("thinking");
        cleaner.options.structured_reply = !self.flag("plain-reply");
        if let Some(effort) = self.option("effort") {
            cleaner.options.effort = effort.to_string();
        }
        Ok(Some(Box::new(cleaner)))
    }

    /// Like `cleaner`, but a missing API key is an error.
    fn required_cleaner(&self, dictionary: &Dictionary) -> anyhow::Result<Box<dyn TextCleaner>> {
        self.cleaner(dictionary)?
            .context("no Anthropic API key; set ANTHROPIC_API_KEY or store one in the key store (see the README)")
    }
}

fn main() -> ExitCode {
    let mut argv = std::env::args().skip(1);
    let command = argv.next().unwrap_or_else(|| "help".into());
    let result = Args::parse(argv).and_then(|args| match command.as_str() {
        "record" => record(&args),
        "run" => run(&args),
        "clean" => clean(&args),
        "bench" => bench(&args),
        "vad" => vad(&args),
        "devices" => {
            devices();
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        other => bail!("unknown command {other:?}; run murmur-eval help"),
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

// MARK: - Commands

fn record(args: &Args) -> anyhow::Result<()> {
    let Some(name) = args.positionals.first() else {
        bail!("record needs a clip name")
    };
    let context = args.context()?;
    let dir = clips_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("couldn't create {}", dir.display()))?;
    let path = dir.join(format!("{name}.wav"));

    let recorder = murmur_audio::Recorder::start(args.option("mic"), |_| {})?;
    let start = Instant::now();
    println!("● Recording… press Return to stop.");
    read_line();
    let samples = recorder.stop();
    clip::save_wav(&samples, &path)?;
    println!(
        "Saved {} ({:.1}s, {:.1}s voiced)\n",
        path.display(),
        start.elapsed().as_secs_f64(),
        murmur_core::voice::voiced_duration(&samples)
    );

    let meta = ClipMeta {
        reference: ask("Type exactly what you said, word for word (Return to skip):"),
        intended: ask("Type what you'd want it to come out as (Return to skip):"),
        context,
    };
    clip::save_meta(&meta, name, &dir)?;
    println!("Saved {name}.json");
    Ok(())
}

/// A speech model to compare, named after its folder.
struct Engine {
    id: String,
    transcriber: Transcriber,
}

fn run(args: &Args) -> anyhow::Result<()> {
    let clips = clip::load_all(&clips_dir())?;
    if clips.is_empty() {
        bail!(
            "no clips in {}; record one with `murmur-eval record <name>`",
            clips_dir().display()
        );
    }
    let dictionary = args.dictionary()?;
    let cleaner = if args.flag("no-cleanup") {
        None
    } else {
        let cleaner = args.cleaner(&dictionary)?;
        match &cleaner {
            Some(cleaner) => cleaner.prepare().context("preparing the cleaner")?,
            None => println!("⚠️  No Anthropic API key found, so the cleanup pass is skipped.\n"),
        }
        cleaner
    };

    let threads = args.number("threads")?;
    let model_dirs: Vec<Option<PathBuf>> = match args.option("models") {
        None => vec![None],
        Some(list) => list.split(',').map(|dir| Some(PathBuf::from(dir))).collect(),
    };
    let mut engines = Vec::new();
    for model_dir in model_dirs {
        let options = murmur_engine::Options { model_dir, threads };
        let dir = murmur_engine::default_model_dir(&options);
        let id = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
        println!("Loading {id}…");
        // Loading also warms the model up, so the first clip's time is a fair one.
        let transcriber = murmur_engine::load_model(&options, |status| eprintln!("  {status}"))?;
        engines.push(Engine { id, transcriber });
    }
    let engine_ids: Vec<String> = engines.iter().map(|e| e.id.clone()).collect();

    let mut results = Vec::new();
    for clip in &clips {
        println!("\n▸ {}", clip.name);
        results.push(run_clip(clip, &engines, &dictionary, cleaner.as_deref()));
    }

    let path = report::write(&results, &engine_ids)?;
    println!("\n{}", report::summary_table(&results, &engine_ids));
    println!("\nReport: {}", path.display());
    Ok(())
}

fn run_clip(clip: &Clip, engines: &[Engine], dictionary: &Dictionary, cleaner: Option<&dyn TextCleaner>) -> ClipResult {
    let samples = murmur_audio::load_wav(&clip.audio);
    let mut result = ClipResult {
        clip: clip.name.clone(),
        context: clip.meta.context,
        audio_seconds: samples
            .as_ref()
            .map_or(0.0, |s| s.len() as f64 / f64::from(murmur_core::SAMPLE_RATE)),
        reference: clip.meta.reference.clone(),
        intended: clip.meta.intended.clone(),
        engines: Vec::new(),
    };
    for engine in engines {
        let samples = match &samples {
            Ok(samples) => samples,
            Err(e) => {
                println!("  {} failed: {e:#}", engine.id);
                result
                    .engines
                    .push(failed(&engine.id, format!("Reading the clip failed: {e:#}")));
                continue;
            }
        };
        let start = Instant::now();
        let raw = match engine.transcriber.transcribe(samples) {
            Ok(raw) => raw,
            Err(e) => {
                println!("  {} failed: {e:#}", engine.id);
                result
                    .engines
                    .push(failed(&engine.id, format!("Transcription failed: {e:#}")));
                continue;
            }
        };
        let stt_seconds = start.elapsed().as_secs_f64();
        let wer = clip.meta.reference.as_deref().map(|r| word_error_rate(&raw, r));
        let wer_note = wer.map(|w| format!(", WER {}", pct(w))).unwrap_or_default();
        println!("  {} [{}{wer_note}]: {raw}", engine.id, ms(stt_seconds));

        let mut engine_result = EngineResult {
            engine: engine.id.clone(),
            raw: raw.clone(),
            stt_seconds,
            wer,
            ..Default::default()
        };
        if let Some(cleaner) = cleaner {
            let start = Instant::now();
            match cleaner.clean(&dictionary.apply_replacements(&raw), clip.meta.context) {
                Ok(cleaned) => {
                    let seconds = start.elapsed().as_secs_f64();
                    println!("    ↳ cleaned [{}]: {}", ms(seconds), cleaned.replace('\n', " ⏎ "));
                    engine_result.cleaned = Some(cleaned);
                    engine_result.cleanup_seconds = Some(seconds);
                }
                Err(e) => {
                    println!("    ↳ cleanup failed: {e:#}");
                    engine_result.error = Some(format!("Cleanup failed: {e:#}"));
                }
            }
        }
        result.engines.push(engine_result);
    }
    result
}

fn failed(engine: &str, error: String) -> EngineResult {
    EngineResult {
        engine: engine.to_string(),
        error: Some(error),
        ..Default::default()
    }
}

fn clean(args: &Args) -> anyhow::Result<()> {
    let Some(text) = args.positionals.first() else {
        bail!("clean needs a transcript string")
    };
    let context = args.context()?;
    let dictionary = args.dictionary()?;
    let cleaner = args.required_cleaner(&dictionary)?;
    cleaner.prepare().context("preparing the cleaner")?;
    let start = Instant::now();
    let cleaned = cleaner.clean(&dictionary.apply_replacements(text), context)?;
    println!("{cleaned}");
    println!("\n({})", ms(start.elapsed().as_secs_f64()));
    Ok(())
}

/// Cleans each of `murmur_core::bench::CASES` `--runs` times with one warm cleaner. Consecutive
/// cases use different styles, so with Ollama the first run of each pays for reading that
/// style's prompt and the rest show the cached speed; both are reported.
fn bench(args: &Args) -> anyhow::Result<()> {
    let runs = args.number("runs")?.unwrap_or(3).max(1);
    let dictionary = args.dictionary()?;
    let cleaner = args.required_cleaner(&dictionary)?;
    println!("Cleaner: {}", cleaner.id());
    let start = Instant::now();
    cleaner.prepare().context("preparing the cleaner")?;
    cleaner.clean("warm up", TargetContext::Plain)?;
    println!("Load and warm-up: {:.1} s\n", start.elapsed().as_secs_f64());

    let mut firsts = Vec::new();
    let mut repeats = Vec::new();
    for (context, text) in murmur_core::bench::CASES {
        let mut times = Vec::new();
        let mut output = String::new();
        for _ in 0..runs {
            let start = Instant::now();
            output = cleaner.clean(&dictionary.apply_replacements(text), context)?;
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

fn vad(args: &Args) -> anyhow::Result<()> {
    if args.positionals.is_empty() {
        bail!("vad needs one or more WAV files");
    }
    for path in &args.positionals {
        let samples = murmur_audio::load_wav(Path::new(path))?;
        let voiced = murmur_core::voice::voiced_duration(&samples);
        let total = samples.len() as f64 / f64::from(murmur_core::SAMPLE_RATE);
        let verdict = if murmur_core::voice::contains_speech(&samples) {
            "speech"
        } else {
            "skip"
        };
        println!("{path}: {voiced:.2}s voiced of {total:.2}s → {verdict}");
    }
    Ok(())
}

fn devices() {
    let default = murmur_audio::default_input_device();
    for name in murmur_audio::input_devices() {
        let mark = if Some(&name) == default.as_ref() { "*" } else { " " };
        println!("{mark} {name}");
    }
}

// MARK: - Helpers

/// The middle value (the upper one of an even count).
fn median(values: &[Duration]) -> Duration {
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted[sorted.len() / 2]
}

fn read_line() -> String {
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    line.trim().to_string()
}

/// Asks `question` and returns the answer, or `None` if it was left empty.
fn ask(question: &str) -> Option<String> {
    print!("{question} ");
    let _ = std::io::stdout().flush();
    Some(read_line()).filter(|answer| !answer.is_empty())
}
