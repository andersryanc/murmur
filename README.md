# Murmur

Push-to-talk dictation for macOS and Linux (Hyprland). Hold a key, speak, and polished text appears wherever your cursor is.

<p align="center">
  <img src="docs/screenshots/demo.gif" width="946" alt="Dictating into a prompt box: the recording pill shows the live mic level, then &quot;Murmuring…&quot;, then &quot;Help me set up a new project.&quot; is pasted at the cursor">
</p>

The pipeline:

```
hotkey ─▶ mic capture ─▶ on-device STT ─▶ Claude cleanup ─▶ paste into frontmost app
                         (Parakeet, CPU)   (Haiku 5.5)
```

Most of the quality comes from the cleanup pass. It fixes misheard words, drops filler and false starts, keeps only the final version of a self-correction, adds punctuation, and formats spoken lists as markdown when the target app allows it.

## Layout

| Folder | What |
|---|---|
| [`crates/`](crates) | One Rust workspace for both platforms: the macOS menu-bar app and the Linux daemon share the dictation engine, cleanup prompt, Dictionary and speech model. The crates are described in [`DEVELOPMENT.md`](DEVELOPMENT.md) and [`CONTRACTS.md`](CONTRACTS.md). |
| [`linux/`](linux) | Linux setup: the install script, Hyprland bindings and the Omarchy pill and bar menu. Start with [`linux/README.md`](linux/README.md). |
| [`scripts/`](scripts) | Building the macOS app bundle, and checking the Linux build from a Mac. |
| [`eval/`](eval) | Recordings of your voice for the [eval](#eval-harness), and its reports. Both stay local. |

Murmur is open source and isn't distributed as a prebuilt app: you build it on the machine that runs it.

## macOS

You need [Rust](https://rustup.rs) (`rustup default stable`), the Xcode Command Line Tools (`xcode-select --install`), and a self-signed code-signing certificate, which takes a minute to make and needs no Apple account (see [Signing](#signing)).

```bash
scripts/bundle-macos.sh   # release build, signed target/Murmur.app
open target/Murmur.app
tail -f ~/Library/Logs/Murmur/murmur.log
```

The first launch downloads the Parakeet model (~480 MB). Allow **Microphone** and turn on Murmur under **System Settings › Privacy & Security › Accessibility**. Accessibility lets Murmur see the push-to-talk key and paste at your cursor. Both permissions persist across rebuilds as long as you sign with the same certificate. Store your [API key](#api-key) for Claude cleanup. The menu's **Launch at Login** starts it with your Mac.

If you still have the earlier Swift version of Murmur installed, quit it first: both listen for fn. They have different bundle IDs, so each has its own permission grants; the Dictionary and API key are shared.

Hold **fn** (changeable in the menu), speak, and release. The text is pasted wherever your cursor is.

<p align="center">
  <img src="docs/screenshots/menu.webp" width="320" alt="The menu-bar menu: last dictation timings, recent dictations to copy, and settings for the key, cleanup, model, microphone and sounds">
  &nbsp;&nbsp;
  <img src="docs/screenshots/dictionary.png" width="420" alt="The Dictionary window's Replacements tab, mapping spoken phrases such as &quot;cube control&quot; to kubectl">
</p>

- **fn key:** set **System Settings › Keyboard › "Press 🌐 key to"** to **Do Nothing**, or macOS will open the emoji picker or its own dictation. You can pick Right ⌥ or Right ⌘ instead. A quick fn tap (under 200 ms) is left alone, so it still works as the Globe key.
- **Chords are ignored:** typing another key while holding the hotkey (e.g. ⌥← to jump words) cancels the recording.
- **Context:** the frontmost app sets the cleanup style. Slack and Messages get chat style, Mail gets email, Notes and Obsidian get markdown notes, editors and terminals get code.
- **Cleanup** (menu): **Claude Haiku** (cloud, most reliable, ~1.3 s), **Local Model** via Ollama (default `qwen3.5:9b`, ~250 ms, occasional slips such as an invented heading or a dropped "yeah"; `ollama pull qwen3.5:9b` first), or **Off** (paste Parakeet's transcript as-is). The menu shows how long the last dictation's speech and cleanup steps took.
- **Fallbacks:** if cleanup fails (no API key, Ollama not running), Murmur pastes the raw transcript and says so. Without Accessibility, it copies the text so you can ⌘V.
- **Dictionary** (menu › Dictionary…): **Words** are spellings to prefer (names, jargon), used by the cleanup step to fix near-misses. **Replacements** are exact "when I say X, write Y" fixes, applied to every dictation even with cleanup off; they ignore case and match whole words only. Both live in `~/Library/Application Support/Murmur/dictionary.json`, which you can also edit by hand; changes apply on the next dictation.
- **Microphone** (menu): System Default or any connected mic. Murmur remembers your choice without changing the system-wide default, and uses the default while that mic is unplugged.
- **Sound Effects** (menu): a start and stop sound in one of four styles, all synthesized in code: **Wood** (low, woody; default), **Soft** (pure round tones with a faint grace note), **Chime** (bright, bell-like) or **Bubble** (a single note that swoops up or down). Picking one plays a preview.
- **Clipboard:** the dictated text stays on the clipboard after pasting, so you can paste it again (or yourself, if no text field had focus).
- **Settings** live in `~/Library/Application Support/Murmur/settings.json`.

### Signing

macOS ties the Microphone and Accessibility permissions to the certificate an app is signed with, so Murmur has to be signed, and with the same certificate every time, or macOS asks again after each rebuild. A self-signed certificate does that: it's only trusted on the Mac that made it, which is all a build you run yourself needs.

1. In **Keychain Access**, choose **Keychain Access › Certificate Assistant › Create a Certificate…**. Name it `Murmur Self-Signed`, set **Identity Type** to **Self-Signed Root** and **Certificate Type** to **Code Signing**, and click **Create**.
2. Run `scripts/bundle-macos.sh`, which signs with `Murmur Self-Signed` by default. The first signing may ask to use the certificate's key; choose **Always Allow**.

Keep the certificate: a new one, even with the same name, makes macOS ask for the permissions again. To sign with a different certificate you already have, such as an Apple Development one, set `MURMUR_SIGN_IDENTITY` to its name; the script lists the ones in your Keychain if it can't find it.

Linux needs no signing.

## Linux

The same `murmur` binary is a daemon and command-line tool for Hyprland, driven by key bindings, with an Omarchy pill and bar menu. Setup, bindings and troubleshooting are in [`linux/README.md`](linux/README.md):

```bash
linux/install.sh   # release build to ~/.local/bin/murmur, plus the Omarchy plugin
murmur doctor
```

## API key

Create a key at [platform.claude.com](https://platform.claude.com/settings/keys). `ANTHROPIC_API_KEY` in the environment overrides the stored key if set.

On macOS, store it in your login Keychain. The command prompts for the key, so it never lands in shell history; add `-U` to replace a stored key:

```bash
security add-generic-password -s com.plyhdrn.murmur -a anthropic-api-key -w
```

On Linux, it's read from the Secret Service keyring; [`linux/README.md`](linux/README.md) has the command.

## Eval harness

`murmur-eval` scores speech-to-text and the cleanup prompt against recordings of your voice, with the same speech model, prompt and Dictionary as the app. It runs on macOS and Linux, and uses the same [API key](#api-key). The first run downloads the speech model if the app hasn't already.

```bash
cargo build --release -p murmur-eval
E=target/release/murmur-eval

# Record clips. After each one, type what you said verbatim (for WER) and what you'd want typed.
$E record standup --context chat
$E record todo-list --context notes

# Transcribe every clip, clean each transcript, and write a report to eval/results/.
$E run
$E run --models ~/models/parakeet-a,~/models/parakeet-b   # compare sherpa-onnx model folders

# Time a cleaner on built-in test dictations: claude (default) or ollama:<model>.
$E bench --cleaner ollama:qwen3.5:9b

# Try the cleanup pass without audio. Add --prompt draft.md to test a prompt edit.
$E clean "um so first update the readme second tag the release" --context notes
```

`$E help` lists every command and option. On macOS the microphone prompt is for your terminal app, which records on its behalf.

- **Clips** live in `eval/clips/` as `<name>.wav` plus a `<name>.json` sidecar (`reference`, `intended`, `context`). Any WAV file you drop in works too. Audio and reports are gitignored.
- **Contexts**: `plain`, `chat`, `email`, `notes`, `code`. They set how the cleanup pass formats output.
- **Dictionary**: every command uses the app's dictionary (`dictionary.json`) so results match real use. Pass `--dictionary <file>` to try another, or `--dictionary none` for none.
- **Cleanup prompt**: `crates/murmur-core/src/prompt.rs`. Try a draft first with `--prompt draft.md`.

