# Murmur

Push-to-talk dictation for macOS. Hold a key, speak, and polished text appears wherever your cursor is.

The pipeline:

```
hotkey ─▶ mic capture ─▶ on-device STT ─▶ Claude cleanup ─▶ paste into frontmost app
                         (Parakeet / ANE)  (Haiku 5.5)
```

Most of the quality comes from the cleanup pass. It fixes misheard words, drops filler and false starts, keeps only the final version of a self-correction, adds punctuation, and formats spoken lists as markdown when the target app allows it.

## Layout

| Folder | What |
|---|---|
| [`App/`](App) | The menu-bar app. `App/project.yml` generates the Xcode project with XcodeGen. |
| [`Core/`](Core) | `MurmurCore`: audio capture, speech engines, Claude cleanup and the cleanup prompt. Shared by the app and the eval. |
| [`Eval/`](Eval) | CLI harness that scores engines and the cleanup prompt against recordings of your voice. |

## The app

```bash
brew install xcodegen   # once
./scripts/install.sh    # build Release, install to /Applications, launch
```

On first launch, allow **Microphone** and turn on Murmur under **System Settings › Privacy & Security › Accessibility**. Accessibility lets Murmur see the push-to-talk key and paste at your cursor. Both permissions persist across rebuilds because the app is signed with your Apple Development certificate.

Hold **fn** (changeable in the menu), speak, and release. The text is pasted wherever your cursor is.

- **fn key:** set **System Settings › Keyboard › "Press 🌐 key to"** to **Do Nothing**, or macOS will open the emoji picker or its own dictation. You can pick Right ⌥ or Right ⌘ instead.
- **Chords are ignored:** typing another key while holding the hotkey (e.g. ⌥← to jump words) cancels the recording.
- **Context:** the frontmost app sets the cleanup style. Slack and Messages get chat style, Mail gets email, Notes and Obsidian get markdown notes, editors and terminals get code.
- **Cleanup** (menu): **Claude Haiku** (cloud, most reliable, ~750 ms), **Local Model** via Ollama (default `qwen3.5:9b`, ~250 ms, occasional slips such as an invented heading or a dropped "yeah"; `ollama pull qwen3.5:9b` first), or **Off** (paste Parakeet's transcript as-is). The menu shows how long the last dictation's speech and cleanup steps took.
- **Fallbacks:** if cleanup fails (no API key, Ollama not running), Murmur pastes the raw transcript and says so. Without Accessibility, it copies the text so you can ⌘V.
- **Microphone** (menu): System Default or any connected mic. Murmur remembers your choice without changing the system-wide default, and uses the default while that mic is unplugged.
- **Sound Effects** (menu): a start and stop sound in one of three styles, all synthesized in code: **Wood** (low, woody; default), **Soft** (pure round tones with a faint grace note), **Chime** (bright, bell-like) or **Bubble** (a single note that swoops up or down). Picking one plays a preview.
- **Clipboard:** the dictated text stays on the clipboard after pasting, so you can paste it again (or yourself, if no text field had focus).

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

# Time a cleaner on built-in test dictations: claude (default), apple, or ollama:<model>.
.build/debug/MurmurEval bench --cleaner ollama:qwen3.5:9b

# Try the cleanup pass without audio. Add --prompt draft.md to test a prompt edit.
.build/debug/MurmurEval clean "um so first update the readme second tag the release" --context notes
```

Run the binary directly rather than `swift run`, so macOS attributes the mic prompt to your terminal.

- **Clips** live in `Eval/clips/` as `<name>.wav` plus a `<name>.json` sidecar (`reference`, `intended`, `context`). Any `.wav`/`.m4a` you drop in works too. Audio is gitignored.
- **Contexts**: `plain`, `chat`, `email`, `notes`, `code`. They set how the cleanup pass formats output.
- **Vocabulary**: `Eval/vocabulary.txt`, holding names and jargon the cleanup pass should spell correctly.
- **Cleanup prompt**: `Core/Sources/MurmurCore/CleanupPrompt.swift`, shared with the app. Try a draft first with `--prompt draft.md`.
