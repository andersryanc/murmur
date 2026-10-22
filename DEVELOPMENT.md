# Developing Squawk

One Rust workspace for macOS and Linux, with Windows to come. The interfaces between the crates
are in [CONTRACTS.md](CONTRACTS.md); setup for using Squawk is in the [README](README.md).

| Crate | What |
|---|---|
| `squawk-core` | Cleanup prompt, Claude and Ollama cleaners, Dictionary, settings, sounds, voice activity, file locations, API key |
| `squawk-stt` | Parakeet TDT 0.6B v3 (int8) on the CPU through sherpa-onnx; downloads the model on first use |
| `squawk-audio` | Mic capture with cpal, resampled to 16 kHz; sound playback |
| `squawk-platform` | Hotkey, paste, permissions, frontmost app. macOS and Linux (Hyprland); Windows is a stub |
| `squawk-control` | The Linux daemon's control socket and its line protocol (Unix only) |
| `squawk-engine` | The dictation loop both front ends share: recording, the queue, transcribe → Dictionary → cleanup → paste, model loading, warm-ups, notices |
| `squawk` | The binary, `squawkctl`. macOS: the tray app (menu, recording pill, Dictionary window, launch at login). Linux: the CLI and daemon |
| `squawk-eval` | The eval: records clips, scores the transcript (WER) and cleanup against them, and times cleaners. macOS and Linux; see "Eval harness" in the [README](README.md#eval-harness) |

## macOS

```bash
scripts/install-macos.sh     # bundle-macos.sh, then move the app to /Applications and relaunch it
scripts/bundle-macos.sh      # release build, signed target/Squawk.app ("Squawk Self-Signed", or SQUAWK_SIGN_IDENTITY)
open target/Squawk.app       # try a build without installing it
tail -f ~/Library/Logs/Squawk/squawk.log
```

Quit the installed Squawk before opening one from `target/`: both have the same bundle ID and
grab the same hotkey.

- The bundle ID is `com.plyhdrn.squawk`. Settings live in
  `~/Library/Application Support/Squawk/settings.json`, the Dictionary in `dictionary.json` beside
  it, and the API key in the Keychain.
- sherpa-onnx is linked as shared libraries on macOS (twice as fast as the static build), so
  the bundle carries `libsherpa-onnx-c-api.dylib` and `libonnxruntime.dylib` in `Contents/Frameworks`.

`cargo run --release -p squawk-stt --example transcribe -- clip.wav` times speech-to-text on a WAV file.

## Linux

The same binary, installed as `squawkctl`, is a command-line tool and daemon for Hyprland, with an
Omarchy pill plugin. Setup, Hyprland bindings and a testing checklist are in [linux/README.md](linux/README.md):

```bash
linux/install.sh             # release build to ~/.local/bin/squawkctl, plus the Omarchy plugin
squawkctl doctor
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
