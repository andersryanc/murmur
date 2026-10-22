# Murmur in Rust

A cross-platform rewrite of Murmur, the push-to-talk dictation app. One codebase for macOS,
Windows and Linux. The Swift app in `../App` stays the daily driver until this matches it on the
eval. The crates and the interfaces between them are described in [CONTRACTS.md](CONTRACTS.md).

| Crate | What |
|---|---|
| `murmur-core` | Cleanup prompt (checked against the Swift copy by a test), Claude and Ollama cleaners, Dictionary, settings, sounds, voice activity, file locations, API key |
| `murmur-stt` | Parakeet TDT 0.6B v3 (int8) on the CPU through sherpa-onnx; downloads the model on first use |
| `murmur-audio` | Mic capture with cpal, resampled to 16 kHz; sound playback |
| `murmur-platform` | Hotkey, paste, permissions, frontmost app. macOS is done, Linux (Hyprland) is written but untested on hardware, Windows is a stub |
| `murmur-control` | The Linux daemon's control socket and its line protocol (Unix only) |
| `murmur-engine` | The dictation loop both front ends share: recording, the queue, transcribe → Dictionary → cleanup → paste, model loading, warm-ups, notices |
| `murmur` | The binary. macOS: the tray app (menu, recording pill, Dictionary window, launch at login). Linux: the CLI and daemon |

## macOS

```bash
scripts/bundle-macos.sh      # release build, signed target/Murmur.app
open target/Murmur.app
tail -f ~/Library/Logs/Murmur/murmur.log
```

- **Quit the Swift Murmur first**: both listen for fn.
- The bundle ID is `com.plyhdrn.murmur-rs`, so it gets its own Accessibility and Microphone
  grants, separate from the Swift app's. Settings live in `~/Library/Application Support/Murmur/settings.json`;
  the Dictionary and the API key (Keychain) are shared with the Swift app.
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

## Not yet

Windows, and the eval.
