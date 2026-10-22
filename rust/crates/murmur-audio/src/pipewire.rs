//! Recording from a PipeWire node by name, on Linux.
//!
//! Linux settings name the microphone by PipeWire node name (`pw-record --list-targets`, e.g.
//! `alsa_input.usb-Blue_Yeti-00.analog-stereo`). Capture goes through ALSA, where those names
//! mean nothing, so such a name is opened through pipewire-alsa's `pipewire` PCM instead, whose
//! `NODE` argument becomes the stream's `target.object` (a node name or serial). That plugin is
//! what makes ALSA reach PipeWire at all on a PipeWire desktop, so it's already installed there.
//!
//! The `PIPEWIRE_NODE` environment variable would also pick the node, but the plugin applies it
//! to every stream in the process, so the start/stop sounds would be sent to the microphone too.
//! The PCM argument targets just this stream. cpal can't open a PCM by an arbitrary name (only
//! ones ALSA lists), so this opens it with the `alsa` crate cpal itself uses.

use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};
use anyhow::{Context, Result};

use crate::capture::Sink;

/// Asked of the plugin, which converts from whatever the node records: what speech
/// recognition wants, so nothing needs resampling afterwards.
pub(crate) const RATE: u32 = murmur_core::SAMPLE_RATE;

/// 50 ms reads, so a stop is noticed quickly and levels arrive at the usual pace.
const PERIOD: alsa::pcm::Frames = (RATE / 20) as alsa::pcm::Frames;

/// Longest wait for audio before checking whether to stop.
const WAIT_MS: u32 = 200;

/// Whether `name` is meant as a PipeWire node rather than one of the ALSA devices cpal lists.
/// Node names never match those (ALSA lists descriptions like "PipeWire Sound Server").
pub(crate) fn is_node_name(name: &str) -> bool {
    !crate::input_devices().iter().any(|device| device == name)
}

/// The ALSA name that opens `node` through pipewire-alsa. Quoted, since node names hold dots
/// and dashes; a name with a quote or backslash can't be a node name ALSA could pass on.
fn pcm_name(node: &str) -> Option<String> {
    (!node.is_empty() && !node.contains(['"', '\\', '\n'])).then(|| format!("pipewire:NODE=\"{node}\""))
}

/// Opens and configures the node's capture stream; fails if pipewire-alsa isn't installed or
/// PipeWire can't be reached. (PipeWire falls back to the default source by itself if the node
/// is gone, as cpal's default device would.)
pub(crate) fn open(node: &str) -> Result<PCM> {
    let name = pcm_name(node).context("not a PipeWire node name")?;
    let pcm = PCM::new(&name, Direction::Capture, false).with_context(|| format!("opening ALSA {name}"))?;
    {
        let params = HwParams::any(&pcm)?;
        params.set_access(Access::RWInterleaved)?;
        params.set_format(Format::float())?;
        params.set_channels(1)?;
        params.set_rate(RATE, ValueOr::Nearest)?;
        params.set_period_size_near(PERIOD, ValueOr::Nearest)?;
        pcm.hw_params(&params).context("setting the recording format")?;
    }
    let rate = pcm.hw_params_current()?.get_rate()?;
    anyhow::ensure!(rate == RATE, "the plugin offered {rate} Hz, not {RATE} Hz");
    Ok(pcm)
}

/// The capture thread: reads periods into `sink` until told to stop.
pub(crate) fn run(pcm: PCM, mut sink: Sink, ready: Sender<Result<()>>, stop: Receiver<()>) {
    let io = match pcm.io_f32().context("reading the microphone") {
        Ok(io) => io,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    if let Err(e) = pcm.start() {
        let _ = ready.send(Err(anyhow::Error::new(e).context("starting the microphone")));
        return;
    }
    let _ = ready.send(Ok(()));
    let mut buffer = vec![0.0f32; PERIOD as usize];
    // Returns on stop() or when the Recorder is dropped (the sender goes away).
    while matches!(stop.try_recv(), Err(TryRecvError::Empty)) {
        // Waits with a timeout rather than blocking in the read, so a stalled stream can't keep
        // `Recorder::stop` waiting for this thread forever.
        match pcm.wait(Some(WAIT_MS)) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                if let Err(e) = pcm.try_recover(e, true) {
                    log::warn!("microphone stream error: {e}");
                    break;
                }
                continue;
            }
        }
        match io.readi(&mut buffer) {
            Ok(frames) => sink.push(&buffer[..frames]),
            Err(e) => {
                // An overrun (we fell behind) is recoverable; anything else ends the recording.
                if let Err(e) = pcm.try_recover(e, true) {
                    log::warn!("microphone stream error: {e}");
                    break;
                }
            }
        }
    }
    let _ = pcm.drop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_names_are_quoted_for_alsa() {
        assert_eq!(
            pcm_name("alsa_input.usb-Blue_Yeti-00.analog-stereo").as_deref(),
            Some(r#"pipewire:NODE="alsa_input.usb-Blue_Yeti-00.analog-stereo""#)
        );
        assert_eq!(pcm_name(""), None);
        assert_eq!(pcm_name("bad\"name"), None);
    }

    #[test]
    fn opening_a_node_without_pipewire_fails_cleanly() {
        // The test machines have no PipeWire server; this must be an error, not a hang or crash.
        assert!(open("alsa_input.murmur-test-no-such-node").is_err());
    }
}
