# Roadmap

Ideas for what's next, roughly in priority order. Nothing here is committed to; it's a list to think about.

## Dictation quality

- [ ] **"Fix…" on recent dictations.** From the menu's recent dictations, correct a word and save it straight to the Dictionary as a replacement. The closest thing to learning from your edits.
- [ ] **Hear your Words right the first time.** Bias Parakeet toward the Dictionary's Words with FluidAudio's vocabulary boosting (`configureVocabularyBoosting`), so the speech recognizer itself gets names right instead of the cleanup step fixing them afterwards. Likely needs an extra model download; measure with the eval before and after.
- [ ] **Try other local cleanup models.** `qwen3.5:9b` sometimes drops "yeah" in "yeah, okay" and invents headings in notes. Compare newer small models with `murmur-eval bench --cleaner ollama:<model>`.

## Speed

- [ ] **Faster Claude cleanup.** The structured reply that stopped Claude pasting its own reasoning added ~0.45 s. Win some back by streaming the reply, reusing a warm connection, and starting the request the moment recording stops.
- [ ] **Transcribe while still talking.** Stream audio into Parakeet during the recording so only the tail is left to process on release.

## App

- [ ] **Branding.** App icon, a proper menu-bar glyph, and a final palette for the overlay.
- [ ] **First-run onboarding.** Walk through Microphone and Accessibility permissions, the fn key setting ("Press 🌐 key to: Do Nothing"), the API key, and a test dictation.
- [ ] **Settings window.** Paste the Anthropic API key in the app (stored in the Keychain) instead of using the `security` command.

Murmur is build-it-yourself: no prebuilt releases, Developer ID or notarization.

## Retire the Swift app

- [ ] **Make `prompt.rs` the only copy of the cleanup prompt.** Drop the test that compares it with `CleanupPrompt.swift`.
- [ ] **Install the Rust app on macOS.** A script that runs `rust/scripts/bundle-macos.sh` and copies the app to `~/Applications`, like `scripts/install.sh` does for the Swift app, so it runs from a stable path rather than `rust/target/`.
- [ ] **Take over the bundle ID.** Once the Swift app is gone, move from `com.plyhdrn.murmur-rs` to `com.plyhdrn.murmur`.
- [ ] **Delete `App/` and `Core/`**, trim the "port of the Swift …" comments, and consider moving `rust/` to the repo root.

## Linux

- [ ] **Finish the Omarchy checklist.** Recording, sounds, pasting and the pill are confirmed on hardware; the rest is listed under "Status" in [`rust/linux/README.md`](rust/linux/README.md#status) (microphone choice, keyring, install and autostart, taps, notices).
- [ ] **Packaging.** An AUR `PKGBUILD`, so installing doesn't need the repo.

## Windows

- [ ] **The platform layer.** `murmur-platform` has a Windows stub: hotkey, paste, frontmost app.
- [ ] **Check whether signing matters.** A locally built `.exe` should need none (SmartScreen only checks downloaded files); confirm on a real machine.

## Known issues

- Local cleanup (`qwen3.5:9b`) occasionally drops a leading "yeah" or adds a heading in notes. Claude doesn't.
- Pasting sends ⌘V by key code, which assumes a QWERTY-style layout.
- The pasted text replaces your clipboard (by design, so you can paste it again).
