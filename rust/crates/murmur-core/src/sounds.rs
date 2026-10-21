//! Start and stop sounds. All are synthesized in code, so there are no audio assets to ship and
//! every sound is Murmur's own rather than a stock system sound.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

/// A named pair of start/stop sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SoundTheme {
    /// Low, soft wooden bloops: D4 → A4 to start, A4 → D4 to stop.
    Wood,
    /// Pure, round tones: a faint grace note, then the main note. D4 → A4 to start, A4 → D4 to
    /// stop, with a quick fade into a soft lingering tail.
    Soft,
    /// Bright bell-like taps an octave and a half higher: E6 → B6, B6 → E6.
    Chime,
    /// A single G4 note that swoops up into pitch to start and down into it to stop.
    Bubble,
}

impl SoundTheme {
    pub const ALL: [SoundTheme; 4] = [Self::Wood, Self::Soft, Self::Chime, Self::Bubble];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wood => "wood",
            Self::Soft => "soft",
            Self::Chime => "chime",
            Self::Bubble => "bubble",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Wood => "Wood",
            Self::Soft => "Soft",
            Self::Chime => "Chime",
            Self::Bubble => "Bubble",
        }
    }

    /// The start sound as a 16-bit mono WAV file.
    pub fn start_sound(self) -> Vec<u8> {
        self.start().render()
    }

    /// The stop sound as a 16-bit mono WAV file.
    pub fn stop_sound(self) -> Vec<u8> {
        self.stop().render()
    }

    fn start(self) -> Voice {
        match self {
            Self::Wood => Voice::wood(&[293.66, 440.0]),
            Self::Chime => Voice::chime(&[1318.5, 1975.5]),
            Self::Bubble => Voice::bubble(0.72),
            Self::Soft => Voice::soft(293.66, 440.0, 0.022, 0.07, 0.035, 20.0, 0.18),
        }
    }

    fn stop(self) -> Voice {
        match self {
            Self::Wood => Voice::wood(&[440.0, 293.66]),
            Self::Chime => Voice::chime(&[1975.5, 1318.5]),
            Self::Bubble => Voice::bubble(1.38),
            Self::Soft => Voice::soft(440.0, 293.66, 0.04, 0.1, 0.02, 6.0, 0.22),
        }
    }
}

// MARK: - Synthesis

struct Note {
    frequency: f64,
    /// Seconds from the start of the sound.
    onset: f64,
    gain: f64,
}

/// How one sound is built: a few notes, each with its own start time and level, sharing an
/// envelope, harmonics and pitch glide.
struct Voice {
    notes: Vec<Note>,
    /// Total length of the sound; the last 8 ms fade out so it never ends on a click.
    length: f64,
    /// Exponential decay rate; higher fades faster.
    decay: f64,
    /// Optional second, slower stage: the note falls quickly, then lingers at `tail_level`
    /// (relative to its peak) and fades at `tail_decay`.
    tail_level: f64,
    tail_decay: f64,
    /// Relative amplitudes of the fundamental, 2nd, 3rd… harmonics.
    harmonics: Vec<f64>,
    /// Each note starts at `glide_from` × its pitch and settles at a rate of `glide_speed`.
    glide_from: f64,
    glide_speed: f64,
    attack: f64,
    gain: f64,
}

const SAMPLE_RATE: f64 = 44_100.0;

impl Voice {
    fn new(notes: Vec<Note>, length: f64, decay: f64, harmonics: &[f64]) -> Self {
        Self {
            notes,
            length,
            decay,
            tail_level: 0.0,
            tail_decay: 0.0,
            harmonics: harmonics.to_vec(),
            glide_from: 1.0,
            glide_speed: 70.0,
            attack: 0.003,
            gain: 0.5,
        }
    }

    /// Equal-level notes, one every `spacing` seconds.
    fn sequence(frequencies: &[f64], spacing: f64) -> Vec<Note> {
        frequencies
            .iter()
            .enumerate()
            .map(|(i, &frequency)| Note {
                frequency,
                onset: spacing * i as f64,
                gain: 1.0,
            })
            .collect()
    }

    fn wood(notes: &[f64]) -> Self {
        let length = 0.05 * notes.len() as f64 + 0.09;
        Self {
            glide_from: 1.025,
            glide_speed: 70.0,
            gain: 0.55,
            ..Self::new(Self::sequence(notes, 0.05), length, 32.0, &[1.0, 0.28, 0.08])
        }
    }

    fn chime(notes: &[f64]) -> Self {
        let length = 0.075 * notes.len() as f64 + 0.12;
        Self::new(Self::sequence(notes, 0.075), length, 28.0, &[1.0, 0.18])
    }

    fn bubble(swoop_from: f64) -> Self {
        let note = Note {
            frequency: 392.0,
            onset: 0.0,
            gain: 1.0,
        };
        Self {
            glide_from: swoop_from,
            glide_speed: 38.0,
            gain: 0.6,
            ..Self::new(vec![note], 0.16, 26.0, &[1.0, 0.12])
        }
    }

    fn soft(grace: f64, main: f64, gap: f64, grace_gain: f64, tail_level: f64, tail_decay: f64, length: f64) -> Self {
        let notes = vec![
            Note {
                frequency: grace,
                onset: 0.0,
                gain: grace_gain,
            },
            Note {
                frequency: main,
                onset: gap,
                gain: 1.0,
            },
        ];
        Self {
            tail_level,
            tail_decay,
            attack: 0.004,
            gain: 0.6,
            ..Self::new(notes, length, 180.0, &[1.0, 0.04])
        }
    }

    /// Renders the voice as a 16-bit mono WAV file.
    fn render(&self) -> Vec<u8> {
        let sr = SAMPLE_RATE;
        let total = (sr * self.length) as usize;
        let mut samples = vec![0f32; total];

        for note in &self.notes {
            let offset = (sr * note.onset) as usize;
            if offset >= total {
                continue;
            }
            let mut phase = 0.0;
            for i in 0..(total - offset) {
                let t = i as f64 / sr;
                let glide = 1.0 + (self.glide_from - 1.0) * (-t * self.glide_speed).exp();
                phase += 2.0 * PI * note.frequency * glide / sr;
                let rise = (t / self.attack).min(1.0);
                let fall =
                    (1.0 - self.tail_level) * (-t * self.decay).exp() + self.tail_level * (-t * self.tail_decay).exp();
                let wave: f64 = self
                    .harmonics
                    .iter()
                    .enumerate()
                    .map(|(n, a)| a * ((n + 1) as f64 * phase).sin())
                    .sum();
                samples[offset + i] += (wave * rise * fall * note.gain * self.gain) as f32;
            }
        }

        let fade = ((sr * 0.008) as usize).min(total);
        for i in 0..fade {
            samples[total - 1 - i] *= i as f32 / fade as f32;
        }
        encode_wav(&samples)
    }
}

fn encode_wav(samples: &[f32]) -> Vec<u8> {
    let data_size = (samples.len() * 2) as u32;
    let rate = SAMPLE_RATE as u32;
    let mut data = Vec::with_capacity(44 + samples.len() * 2);
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36 + data_size).to_le_bytes());
    data.extend_from_slice(b"WAVE");
    data.extend_from_slice(b"fmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes()); // PCM
    data.extend_from_slice(&1u16.to_le_bytes()); // mono
    data.extend_from_slice(&rate.to_le_bytes());
    data.extend_from_slice(&(rate * 2).to_le_bytes()); // byte rate
    data.extend_from_slice(&2u16.to_le_bytes()); // block align
    data.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    data.extend_from_slice(b"data");
    data.extend_from_slice(&data_size.to_le_bytes());
    for sample in samples {
        let pcm = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        data.extend_from_slice(&pcm.to_le_bytes());
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_wav() {
        for theme in SoundTheme::ALL {
            for wav in [theme.start_sound(), theme.stop_sound()] {
                assert_eq!(&wav[..4], b"RIFF");
                assert!(wav.len() > 44);
                assert_eq!(
                    u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize,
                    wav.len() - 44
                );
            }
        }
    }

    #[test]
    fn serde_names() {
        assert_eq!(serde_json::to_string(&SoundTheme::Bubble).unwrap(), "\"bubble\"");
        assert_eq!(SoundTheme::parse("soft"), Some(SoundTheme::Soft));
    }
}
