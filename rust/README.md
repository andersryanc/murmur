# Murmur in Rust

A cross-platform rewrite of Murmur, the push-to-talk dictation app. One codebase for macOS,
Windows and Linux. The Swift app in `../App` stays the daily driver until this matches it on the
eval. The crates and the interfaces between them are described in [CONTRACTS.md](CONTRACTS.md).

| Crate | What |
|---|---|
| `murmur-core` | Cleanup prompt (checked against the Swift copy by a test), Claude and Ollama cleaners, Dictionary, settings, sounds, voice activity, file locations, API key |
| `murmur-stt` | Parakeet TDT 0.6B v3 (int8) on the CPU through sherpa-onnx; downloads the model on first use |
| `murmur-audio` | Mic capture with cpal, resampled to 16 kHz; sound playback |
| `murmur-platform` | Hotkey, paste, permissions, frontmost app. macOS is done; Windows and Linux are stubs |
| `murmur` | The tray app: menu, dictation queue, startup |

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

## Not yet

The recording pill, the Dictionary window (Dictionary… opens the JSON), launch at login,
Windows and Linux platform code, and the eval.
