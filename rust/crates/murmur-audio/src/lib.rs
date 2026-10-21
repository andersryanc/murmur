//! Microphone capture and sound playback on macOS (CoreAudio), Windows (WASAPI) and Linux
//! (ALSA, which also reaches PipeWire and PulseAudio through their ALSA plugins).
//!
//! Capture takes whatever format the device offers and converts once, at stop, to the 16 kHz
//! mono `f32` that speech recognition wants. Converting late keeps the audio callback cheap and
//! lets the resampler see the whole recording, which avoids chunk-boundary artifacts.

mod capture;
mod convert;
mod playback;

pub use capture::{Recorder, default_input_device, input_devices};
pub use playback::play_wav;
