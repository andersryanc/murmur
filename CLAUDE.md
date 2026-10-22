# Murmur

Push-to-talk dictation: hold a key, speak, and cleaned-up text is pasted at the cursor. See [README.md](README.md) for the layout and [ROADMAP.md](ROADMAP.md) for what's next.

- **Murmur is one Rust workspace** (`crates/`), for macOS (the menu-bar app) and Linux (the Hyprland daemon). [DEVELOPMENT.md](DEVELOPMENT.md) lists the crates; the interfaces between them are in [CONTRACTS.md](CONTRACTS.md), so change it together with its callers. `cargo test --workspace` runs the tests; CI (`.github/workflows/rust.yml`) also runs `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` on macOS and Linux.
- **Linux (Hyprland/Omarchy):** if you're running on Linux, start with [linux/README.md](linux/README.md): setup, system packages, `linux/install.sh`, the API key, Hyprland bindings, and a testing checklist.
- **Eval:** `murmur-eval` (`crates/murmur-eval`) scores speech-to-text and the cleanup prompt against recordings in `eval/clips/`; see "Eval harness" in the README.
- The cleanup prompt drives most of the quality. It lives in `crates/murmur-core/src/prompt.rs`. Check changes with `murmur-eval run` (and `--prompt <file>` for a draft).
