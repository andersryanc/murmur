# Roadmap

Ideas for what's next, roughly in priority order. Nothing here is committed to; it's a list to think about.

## Dictation quality

- [ ] **"Fix…" on recent dictations.** From the menu's recent dictations, correct a word and save it straight to the Dictionary as a replacement. The closest thing to learning from your edits.
- [ ] **Hear your Words right the first time.** Bias Parakeet toward the Dictionary's Words with FluidAudio's vocabulary boosting (`configureVocabularyBoosting`), so the speech recognizer itself gets names right instead of the cleanup step fixing them afterwards. Likely needs an extra model download; measure with the eval before and after.
- [ ] **Try other local cleanup models.** `qwen3.5:9b` sometimes drops "yeah" in "yeah, okay" and invents headings in notes. Compare newer small models with `MurmurEval bench --cleaner ollama:<model>`.

## Speed

- [ ] **Faster Claude cleanup.** The structured reply that stopped Claude pasting its own reasoning added ~0.45 s. Win some back by streaming the reply, reusing a warm connection, and starting the request the moment recording stops.
- [ ] **Transcribe while still talking.** Stream audio into Parakeet during the recording so only the tail is left to process on release.

## App

- [ ] **Branding.** App icon, a proper menu-bar glyph, and a final palette for the overlay.
- [ ] **First-run onboarding.** Walk through Microphone and Accessibility permissions, the fn key setting ("Press 🌐 key to: Do Nothing"), the API key, and a test dictation.
- [ ] **Settings window.** Paste the Anthropic API key in the app (stored in the Keychain) instead of using the `security` command.
- [ ] **Distribution.** Sign with a personal Developer ID, notarize, and ship updates (e.g. Sparkle).

## Retire the Swift app

- [ ] **Port the eval to Rust.** `murmur eval record/run/bench/clean` against the sherpa-onnx Parakeet the app actually runs, then delete `Eval/`.
- [ ] **Make `prompt.rs` the only copy of the cleanup prompt.** Drop the test that compares it with `CleanupPrompt.swift`.
- [ ] **Release and install the Rust app.** Rust versions of `scripts/install.sh`, `release.sh` and `get.sh`, built on `rust/scripts/bundle-macos.sh`. Decide whether the bundle ID moves from `com.plyhdrn.murmur-rs` to `com.plyhdrn.murmur`, which keeps existing permission grants and the update path.
- [ ] **Delete `App/` and `Core/`**, trim the "port of the Swift …" comments, and consider moving `rust/` to the repo root.

## Linux

- [ ] **Test on the Omarchy machine.** Work through the testing checklist in [`rust/linux/README.md`](rust/linux/README.md) (mic, paste, key bindings, keyring, latency).
- [ ] **Packaging.** An AUR `PKGBUILD`, so installing doesn't need the repo.

## Windows

- [ ] **The platform layer.** `murmur-platform` has a Windows stub: hotkey, paste, frontmost app.

## Known issues

- Local cleanup (`qwen3.5:9b`) occasionally drops a leading "yeah" or adds a heading in notes. Claude doesn't.
- Pasting sends ⌘V by key code, which assumes a QWERTY-style layout.
- The pasted text replaces your clipboard (by design, so you can paste it again).
