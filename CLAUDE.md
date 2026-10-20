# Murmur

Push-to-talk dictation: hold a key, speak, and cleaned-up text is pasted at the cursor. See [README.md](README.md) for the layout and [ROADMAP.md](ROADMAP.md) for what's next.

- **macOS:** the menu-bar app in `App/`, plus `MurmurCore` (macOS-only audio and speech engines) in `Core/`.
- **Linux (Hyprland/Omarchy):** `Linux/`. If you're running on Linux, start with [Linux/README.md](Linux/README.md): it has the setup steps (Swift, system packages, `Linux/scripts/install.sh`, the API key, Hyprland bindings), what has and hasn't been tested yet, and a testing checklist to work through.
- **Shared:** `Core/Sources/MurmurKit` (cleanup prompt, cleaners, Dictionary, sounds) must stay plain Foundation so it builds on both. `cd Core && swift test` runs its tests on either system.
- The cleanup prompt (`Core/Sources/MurmurKit/CleanupPrompt.swift`) drives most of the quality; check changes with the eval (`Eval/`, macOS only).
