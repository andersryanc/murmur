# Murmur

Push-to-talk dictation for macOS. Hold a key, speak, and polished text appears wherever your cursor is.

The pipeline:

```
hotkey ─▶ mic capture ─▶ on-device STT ─▶ Claude cleanup ─▶ paste into frontmost app
                         (Parakeet / ANE)  (Haiku 5.5)
```

Most of the quality comes from the cleanup pass. It fixes misheard words, drops filler and false starts, keeps only the final version of a self-correction, adds punctuation, and formats spoken lists as markdown when the target app allows it.

## Status

**Phase 0: eval harness** ([`Eval/`](Eval)). Checks STT engines and the cleanup prompt against your own voice before any app UI exists.

## Eval harness

Requires macOS 26 and Xcode 26. The first run downloads the Parakeet models (~500 MB) and Apple's speech assets.

**API key.** Create a key at [platform.claude.com](https://platform.claude.com/settings/keys), then store it in your login Keychain. The command prompts for the key, so it never lands in shell history:

```bash
security add-generic-password -s com.plyhdrn.murmur -a anthropic-api-key -w
```

`ANTHROPIC_API_KEY` in the environment overrides the Keychain if set. To replace a stored key, add `-U` to the command above.

```bash
cd Eval
swift build

# Record clips. After each one, type what you said verbatim (for WER) and what you'd want typed.
.build/debug/MurmurEval record standup --context chat
.build/debug/MurmurEval record todo-list --context notes

# Transcribe every clip with each engine, clean each transcript, and write a report to results/.
.build/debug/MurmurEval run
.build/debug/MurmurEval run --engines parakeet,parakeet-ultra,apple

# Iterate on prompts/cleanup.md without audio.
.build/debug/MurmurEval clean "um so first update the readme second tag the release" --context notes
```

Run the binary directly rather than `swift run`, so macOS attributes the mic prompt to your terminal.

- **Clips** live in `Eval/clips/` as `<name>.wav` plus a `<name>.json` sidecar (`reference`, `intended`, `context`). Any `.wav`/`.m4a` you drop in works too. Audio is gitignored.
- **Contexts**: `plain`, `chat`, `email`, `notes`, `code`. They set how the cleanup pass formats output.
- **Vocabulary**: `Eval/vocabulary.txt`, holding names and jargon the cleanup pass should spell correctly.
- **Cleanup prompt**: `Eval/prompts/cleanup.md`. It's read at runtime, so edits need no rebuild.
