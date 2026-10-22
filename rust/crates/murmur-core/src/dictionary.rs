//! Your words and fixes.
//!
//! - **Words** are correct spellings (names, jargon, product names). The cleanup model gets them
//!   as preferred spellings, so it can fix near-misses from context.
//! - **Replacements** are exact "when I say X, write Y" rules, applied to the transcript before
//!   cleanup. They work even with cleanup off.
//!
//! Stored as plain, human-editable JSON, the same file the Swift app reads:
//! ```json
//! {
//!   "replacements" : [ { "from" : "cube control", "to" : "kubectl" } ],
//!   "words" : [ "Plyhdrn", "Kubernetes" ]
//! }
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use fancy_regex::{NoExpand, Regex};
use serde::{Deserialize, Serialize};

/// Fields are declared in alphabetical order so the saved JSON has sorted keys, like the Swift
/// app's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dictionary {
    #[serde(default)]
    pub replacements: Vec<Replacement>,
    #[serde(default)]
    pub words: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replacement {
    /// What the speech recognizer writes when you say it, e.g. "cube control".
    pub from: String,
    /// What should be typed instead, e.g. "kubectl".
    pub to: String,
}

impl Replacement {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

impl Dictionary {
    /// Spellings for the cleanup model: your words, plus every replacement's output so the model
    /// keeps those exactly as written (e.g. doesn't capitalize "kubectl").
    pub fn vocabulary(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        self.words
            .iter()
            .chain(self.replacements.iter().map(|r| &r.to))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty() && seen.insert(s.to_lowercase()))
            .map(String::from)
            .collect()
    }

    /// Applies every replacement to `text`. Matching ignores case, only matches whole words (so
    /// "art" never changes "start"), and treats spaces, hyphens and commas between words as
    /// interchangeable, since recognizers vary ("cube control", "Cube-control", "cube, control").
    /// Longer phrases are applied first, so a specific rule wins over a shorter one inside it.
    pub fn apply_replacements(&self, text: &str) -> String {
        let mut rules: Vec<(&str, &str)> = self
            .replacements
            .iter()
            .map(|r| (r.from.trim(), r.to.as_str()))
            .filter(|(from, _)| !from.is_empty())
            .collect();
        rules.sort_by_key(|(from, _)| std::cmp::Reverse(from.chars().count()));

        let mut result = text.to_string();
        for (from, to) in rules {
            let Some(regex) = pattern(from) else { continue };
            result = regex.replace_all(&result, NoExpand(to)).into_owned();
        }
        result
    }

    /// `dictionary.json` in `paths::data_dir()`.
    pub fn default_path() -> PathBuf {
        crate::paths::data_dir().join("dictionary.json")
    }

    /// The dictionary at `path`, or an empty one if the file doesn't exist yet.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        write_atomically(path, &serde_json::to_vec_pretty(self)?)
    }
}

fn pattern(phrase: &str) -> Option<Regex> {
    let words: Vec<String> = phrase
        .split(|c: char| c.is_whitespace() || c == '-' || c == ',')
        .filter(|w| !w.is_empty())
        .map(fancy_regex::escape)
        .map(String::from)
        .collect();
    if words.is_empty() {
        return None;
    }
    let body = words.join(r"[\s\-,]+");
    Regex::new(&format!(r"(?i)(?<![\p{{L}}\p{{N}}]){body}(?![\p{{L}}\p{{N}}])")).ok()
}

/// Writes to a temporary file beside `path`, then renames it over `path`, so a crash never
/// leaves a half-written file.
pub fn write_atomically(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    std::fs::write(&temp, data)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(rules: &[(&str, &str)], text: &str) -> String {
        let replacements = rules.iter().map(|(from, to)| Replacement::new(*from, *to)).collect();
        Dictionary {
            words: vec![],
            replacements,
        }
        .apply_replacements(text)
    }

    #[test]
    fn ignores_case() {
        assert_eq!(
            apply(&[("cube control", "kubectl")], "Run Cube Control get pods"),
            "Run kubectl get pods"
        );
    }

    #[test]
    fn treats_spaces_hyphens_and_commas_alike() {
        assert_eq!(
            apply(&[("cube control", "kubectl")], "cube-control, cube,  control"),
            "kubectl, kubectl"
        );
    }

    #[test]
    fn only_matches_whole_words() {
        assert_eq!(apply(&[("art", "ART")], "start the art party"), "start the ART party");
    }

    #[test]
    fn applies_longer_phrases_first() {
        let rules = [("ply", "Ply"), ("ply hydrant", "Plyhdrn")];
        assert_eq!(apply(&rules, "ply hydrant and ply"), "Plyhdrn and Ply");
    }

    #[test]
    fn writes_replacement_literally() {
        assert_eq!(
            apply(&[("dollar sign one", "$1 \\0")], "costs dollar sign one"),
            "costs $1 \\0"
        );
    }

    #[test]
    fn treats_pattern_characters_in_from_literally() {
        assert_eq!(apply(&[("c++", "C++")], "I like c++ a lot"), "I like C++ a lot");
    }

    #[test]
    fn skips_blank_rules() {
        assert_eq!(apply(&[("", "x"), ("  ", "y")], "unchanged"), "unchanged");
    }

    #[test]
    fn keeps_punctuation_around_match() {
        assert_eq!(
            apply(&[("my email", "me@example.com")], "Send it to my email."),
            "Send it to me@example.com."
        );
    }

    #[test]
    fn unicode_word_boundaries() {
        assert_eq!(
            apply(&[("café", "Café")], "Ein CAFÉ, kein Xcafé"),
            "Ein Café, kein Xcafé"
        );
    }

    fn temp_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("murmur-test-{name}-{}-{nanos}.json", std::process::id()))
    }

    #[test]
    fn round_trips_as_plain_json() {
        let path = temp_path("dictionary");
        let dictionary = Dictionary {
            words: vec!["Plyhdrn".into()],
            replacements: vec![Replacement::new("cube control", "kubectl")],
        };
        dictionary.save(&path).unwrap();

        let json = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(
            json.contains(r#""words": ["#) && json.contains(r#""Plyhdrn""#),
            "{json}"
        );
        assert!(!json.contains("id"));
        assert!(
            json.find("\"replacements\"") < json.find("\"words\""),
            "keys are sorted"
        );
        assert_eq!(serde_json::from_str::<Dictionary>(&json).unwrap(), dictionary);
    }

    #[test]
    fn reads_the_swift_format() {
        let json = r#"{
  "replacements" : [ { "from" : "cube control", "to" : "kubectl" } ],
  "words" : [ "Plyhdrn", "Kubernetes" ]
}"#;
        let dictionary: Dictionary = serde_json::from_str(json).unwrap();
        assert_eq!(dictionary.words, ["Plyhdrn", "Kubernetes"]);
        assert_eq!(dictionary.replacements, [Replacement::new("cube control", "kubectl")]);
    }

    #[test]
    fn missing_file_loads_empty() {
        assert_eq!(Dictionary::load(&temp_path("missing")).unwrap(), Dictionary::default());
    }

    #[test]
    fn vocabulary_includes_replacement_targets_without_duplicates() {
        let dictionary = Dictionary {
            words: vec!["kubectl".into(), "Plyhdrn".into(), " ".into()],
            replacements: vec![
                Replacement::new("cube control", "kubectl"),
                Replacement::new("my email", "me@example.com"),
            ],
        };
        assert_eq!(dictionary.vocabulary(), ["kubectl", "Plyhdrn", "me@example.com"]);
    }
}
