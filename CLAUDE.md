# Murmur

Push-to-talk dictation: hold a key, speak, and cleaned-up text is pasted at the cursor. See [README.md](README.md) for the layout and [ROADMAP.md](ROADMAP.md) for what's next.

- **Murmur is the Rust workspace in `rust/`**, for macOS (the menu-bar app) and Linux (the Hyprland daemon). The crates and the interfaces between them are in [rust/CONTRACTS.md](rust/CONTRACTS.md); change it together with its callers. `cd rust && cargo test --workspace` runs the tests; CI (`.github/workflows/rust.yml`) also runs `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` on macOS and Linux.
- **Linux (Hyprland/Omarchy):** if you're running on Linux, start with [rust/linux/README.md](rust/linux/README.md): setup, system packages, `rust/linux/install.sh`, the API key, Hyprland bindings, and a testing checklist.
- **Swift, being retired:** the original macOS app (`App/`, `Core/`) and the eval (`Eval/`, macOS only) stay until the eval and the release scripts are ported. Don't add features there.
- The cleanup prompt drives most of the quality. It lives in `rust/crates/murmur-core/src/prompt.rs` and, byte for byte, in `Core/Sources/MurmurKit/CleanupPrompt.swift`; a Rust test fails if they differ, so edit both. Check changes with the eval.
