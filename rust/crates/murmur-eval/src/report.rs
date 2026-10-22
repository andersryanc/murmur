//! Scoring and the report: word error rate on the raw transcript, and `results/<time>.md` (to
//! read) plus `.json` (to diff between runs).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use murmur_core::TargetContext;
use serde::Serialize;

use crate::clip::results_dir;

/// One engine's go at one clip.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineResult {
    pub engine: String,
    pub raw: String,
    pub stt_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wer: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleaned: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipResult {
    pub clip: String,
    pub context: TargetContext,
    pub audio_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intended: Option<String>,
    pub engines: Vec<EngineResult>,
}

// MARK: - Word error rate

/// Lowercased words with punctuation dropped, keeping apostrophes inside words ("don't").
fn normalized_words(text: &str) -> Vec<String> {
    let kept: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' })
        .collect();
    kept.split_whitespace()
        .map(|w| w.trim_matches('\'').to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// (substitutions + deletions + insertions) / reference length, by word-level edit distance.
pub fn word_error_rate(hypothesis: &str, reference: &str) -> f64 {
    let reference = normalized_words(reference);
    let hypothesis = normalized_words(hypothesis);
    if reference.is_empty() {
        return if hypothesis.is_empty() { 0.0 } else { 1.0 };
    }
    let mut previous: Vec<usize> = (0..=hypothesis.len()).collect();
    for (i, r) in reference.iter().enumerate() {
        let mut current = vec![i + 1; hypothesis.len() + 1];
        for (j, h) in hypothesis.iter().enumerate() {
            let cost = usize::from(r != h);
            current[j + 1] = (previous[j + 1] + 1).min(current[j] + 1).min(previous[j] + cost);
        }
        previous = current;
    }
    previous[hypothesis.len()] as f64 / reference.len() as f64
}

// MARK: - Output

/// Writes the report as `.md` and `.json` and returns the Markdown file's path.
pub fn write(results: &[ClipResult], engines: &[String]) -> anyhow::Result<PathBuf> {
    let dir = results_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("couldn't create {}", dir.display()))?;
    let now = utc_now();
    // No colons, so the name works on every file system.
    let base = dir.join(now.replace(':', "-"));

    std::fs::write(base.with_extension("json"), serde_json::to_vec_pretty(results)?)?;
    let markdown = base.with_extension("md");
    std::fs::write(&markdown, render(results, engines, &now))?;
    Ok(markdown)
}

pub fn summary_table(results: &[ClipResult], engines: &[String]) -> String {
    let mut lines = vec![
        "| Engine | Mean WER | Mean STT latency | Mean cleanup latency |".to_string(),
        "|---|---|---|---|".to_string(),
    ];
    for engine in engines {
        let rows: Vec<&EngineResult> = results
            .iter()
            .flat_map(|c| &c.engines)
            .filter(|r| &r.engine == engine && r.error.is_none())
            .collect();
        let wers: Vec<f64> = rows.iter().filter_map(|r| r.wer).collect();
        let stt: Vec<f64> = rows.iter().map(|r| r.stt_seconds).collect();
        let cleanup: Vec<f64> = rows.iter().filter_map(|r| r.cleanup_seconds).collect();
        let cell = |values: &[f64], format: fn(f64) -> String| {
            if values.is_empty() {
                "–".to_string()
            } else {
                format(mean(values))
            }
        };
        lines.push(format!(
            "| {engine} | {} | {} | {} |",
            cell(&wers, pct),
            cell(&stt, ms),
            cell(&cleanup, ms)
        ));
    }
    lines.join("\n")
}

fn render(results: &[ClipResult], engines: &[String], now: &str) -> String {
    let mut out = format!("# Murmur eval: {now}\n\n");
    out += "WER is measured on the **raw** speech-to-text output against each clip's verbatim `reference`.\n\n";
    out += &summary_table(results, engines);
    out += "\n";

    for clip in results {
        out += &format!("\n---\n\n## {}\n\n", clip.clip);
        out += &format!("`{}` · {:.1}s audio\n\n", clip.context.as_str(), clip.audio_seconds);
        if let Some(reference) = &clip.reference {
            out += &format!("**Reference (verbatim):** {reference}\n\n");
        }
        if let Some(intended) = &clip.intended {
            out += &format!("**Intended:**\n\n{}\n\n", quote(intended));
        }
        for engine in &clip.engines {
            out += &format!("### {}\n\n", engine.engine);
            if let Some(error) = &engine.error {
                out += &format!("> ⚠️ {error}\n\n");
                continue;
            }
            let wer = engine.wer.map(|w| format!(" · WER {}", pct(w))).unwrap_or_default();
            out += &format!("Raw, {}{wer}:\n\n{}\n\n", ms(engine.stt_seconds), quote(&engine.raw));
            if let Some(cleaned) = &engine.cleaned {
                out += &format!(
                    "Cleaned, {}:\n\n{}\n\n",
                    ms(engine.cleanup_seconds.unwrap_or_default()),
                    quote(cleaned)
                );
            }
        }
    }
    out
}

fn quote(text: &str) -> String {
    if text.is_empty() {
        return "> _(empty)_".into();
    }
    text.split('\n')
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

pub fn pct(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

pub fn ms(seconds: f64) -> String {
    format!("{:.0} ms", seconds * 1000.0)
}

/// The current time in UTC, like `2026-10-22T05:26:04Z`.
fn utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Days since 1970-01-01 to a (year, month, day) date, after Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wer_counts_word_edits() {
        assert_eq!(word_error_rate("the quick brown fox", "the quick brown fox"), 0.0);
        // One substitution in four words.
        assert_eq!(word_error_rate("the quick brown box", "the quick brown fox"), 0.25);
        // One deletion and one insertion.
        assert_eq!(word_error_rate("quick brown fox jumps", "the quick brown fox"), 0.5);
    }

    #[test]
    fn wer_ignores_case_and_punctuation() {
        assert_eq!(word_error_rate("Don't stop, okay?", "don't stop okay"), 0.0);
        assert_eq!(word_error_rate("'quoted' words", "quoted words"), 0.0);
    }

    #[test]
    fn wer_with_an_empty_reference() {
        assert_eq!(word_error_rate("", ""), 0.0);
        assert_eq!(word_error_rate("something", ""), 1.0);
    }

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_383), (2025, 10, 22));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }
}
