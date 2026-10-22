//! How long each stage of a dictation took.

use std::time::Duration;

/// How long each stage of the last dictation took, for the Mac menu and the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timing {
    pub speech: Duration,
    /// How long cleanup took and which cleaner did it; `None` when cleanup was off or failed.
    pub cleanup: Option<(Duration, String)>,
}

impl Timing {
    pub fn summary(&self) -> String {
        let mut text = format!("Last: speech {} ms", self.speech.as_millis());
        if let Some((took, cleaner)) = &self.cleanup {
            text += &format!(" · cleanup {} ms ({cleaner})", took.as_millis());
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_summary() {
        let timing = Timing {
            speech: Duration::from_millis(212),
            cleanup: Some((Duration::from_millis(840), "claude-haiku".into())),
        };
        assert_eq!(timing.summary(), "Last: speech 212 ms · cleanup 840 ms (claude-haiku)");
        let raw = Timing {
            cleanup: None,
            ..timing
        };
        assert_eq!(raw.summary(), "Last: speech 212 ms");
    }
}
