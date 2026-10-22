# Murmur

Murmur, the push-to-talk dictation app: one codebase for macOS and Linux, with Windows to come.
The crates and the interfaces
between them are described in [CONTRACTS.md](CONTRACTS.md); user-facing setup is in the
[top-level README](../README.md).

| Crate | What |
|---|---|
| `murmur-core` | Cleanup prompt, Claude and Ollama cleaners, Dictionary, settings, sounds, voice activity, file locations, API key |
| `murmur-stt` | Parakeet TDT 0.6B v3 (int8) on the CPU through sherpa-onnx; downloads the model on first use |
| `murmur-audio` | Mic capture with cpal, resampled to 16 kHz; sound playback |
| `murmur-platform` | Hotkey, paste, permissions, frontmost app. macOS and Linux (Hyprland); Windows is a stub |
| `murmur-control` | The Linux daemon's control socket and its line protocol (Unix only) |
| `murmur-engine` | The dictation loop both front ends share: recording, the queue, transcribe → Dictionary → cleanup → paste, model loading, warm-ups, notices |
| `murmur` | The binary. macOS: the tray app (menu, recording pill, Dictionary window, launch at login). Linux: the CLI and daemon |
| `murmur-eval` | The eval: records clips, scores the transcript (WER) and cleanup against them, and times cleaners. macOS and Linux; see "Eval harness" in the [top-level README](../README.md#eval-harness) |

## macOS

```bash
scripts/bundle-macos.sh      # release build, signed target/Murmur.app ("Murmur Self-Signed", or MURMUR_SIGN_IDENTITY)
open target/Murmur.app
tail -f ~/Library/Logs/Murmur/murmur.log
```

- The bundle ID is `com.plyhdrn.murmur-rs`. Settings live in
  `~/Library/Application Support/Murmur/settings.json`, the Dictionary in `dictionary.json` beside
  it, and the API key in the Keychain.
- sherpa-onnx is linked as shared libraries on macOS (twice as fast as the static build), so
  the bundle carries `libsherpa-onnx-c-api.dylib` and `libonnxruntime.dylib` in `Contents/Frameworks`.

`cargo run --release -p murmur-stt --example transcribe -- clip.wav` times speech-to-text on a WAV file.

## Linux

The same `murmur` binary is a command-line tool and daemon for Hyprland, with an Omarchy pill
plugin. Setup, Hyprland bindings and a testing checklist are in [linux/README.md](linux/README.md):

```bash
linux/install.sh             # release build to ~/.local/bin/murmur, plus the Omarchy plugin
murmur doctor
```

## Checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace       # tests needing the model, an API key or a desktop are #[ignore]d
```

CI runs all three on macOS and Linux. `scripts/linux-docker-test.sh` runs them in a Debian container from a Mac.

## Not yet

Windows.
