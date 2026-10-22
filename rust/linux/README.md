# Murmur for Linux (Rust)

Push-to-talk dictation on Linux with Hyprland (built for [Omarchy](https://omarchy.org)). Hold a key, speak, and polished text is pasted wherever your cursor is. It uses the same speech model, cleanup prompt, Dictionary and dictation engine as the Mac app.

```
Hyprland key bind ─▶ murmur start/stop ─▶ daemon ─▶ ALSA (pipewire-alsa) ─▶ Parakeet (sherpa-onnx, CPU)
                                                  ─▶ Claude / Ollama cleanup ─▶ wl-copy + wtype
```

- **`murmur daemon`** runs in the background. It loads Parakeet v3 once (downloading it the first time) and listens on a Unix socket (`$XDG_RUNTIME_DIR/murmur/murmur.sock`).
- **`murmur start`** and **`murmur stop`** are bound to a key's press and release in Hyprland. Wayland doesn't let apps watch keys globally, so the compositor does it.
- The recording, queue, cleanup, Dictionary, start/stop sounds and per-app style are the shared [`murmur-engine`](../crates/murmur-engine) crate, the same code the Mac tray app runs. Your Mac Dictionary file works here unchanged.
- **The on-screen pill** (like the Mac pill) is an [Omarchy shell](https://omarchy.org) plugin in [`omarchy-plugin/`](omarchy-plugin). It shows the mic level while you speak, "Murmuring…" while it works, and any problem for a moment afterwards.
- **The bar menu** (like the Mac menu-bar menu) is in the same plugin: a mic icon in the Omarchy bar that turns red while recording and dims when the daemon isn't running. Click it for the cleanup mode and local model, the microphone, the sound effects, the last five dictations (click one to copy it), the Dictionary, and starting, restarting or quitting the daemon.

This replaces the Swift Linux build (`Linux/`, removed from the repo; it's in git history before the Rust rewrite). It reads that build's `~/.config/murmur/config.json` and model folder as they are, so switching needs no setup beyond installing.

## Status

Tested on an Omarchy machine (Hyprland, PipeWire). Confirmed there (October 2026):

- Recording from the real microphone, with the pill's level meter following your voice.
- The start and stop sounds.
- Pasting with `wl-copy` + `wtype` into a browser text field, a terminal and a chat app.
- The Omarchy pill against this daemon, including two quick dictations showing two cards.

Not confirmed on hardware yet (checklist step in brackets):

- Picking a microphone by node name from the bar menu [5]. It works in Docker, below.
- The API key from `secret-tool` rather than `ANTHROPIC_API_KEY`.
- `install.sh`, the Hyprland bindings and autostart [13].
- The pill reconnecting after a daemon restart [8], taps pasting nothing [11], the first word not being clipped [10], and `notify-send` notices without the pill [12].

Checked in Docker (Rust 1.99, Debian bookworm, x86_64):

- The whole workspace builds, and its tests, clippy and rustfmt pass. The build needs no GTK or WebKitGTK.
- `murmur transcribe <clip> --no-cleanup` gives a correct transcript with the model folder the Mac app downloaded.
- The daemon: `murmur status`, a second daemon refusing to start, `murmur start` with no microphone (a clean error reply, and a notice to watchers), a socket watcher seeing the state and notices, `murmur doctor`, `--help`, and shutdown on SIGTERM (the socket file is removed).
- Picking a microphone by PipeWire node name (PipeWire 0.3.65 with WirePlumber and pipewire-alsa in the container, arm64): with two virtual sources, a tone and silence, the node name decides which one is recorded and `pw-link` shows the stream linked to it; no setting records the default source, and a name that doesn't exist falls back to the default.

**T2 MacBooks:** the built-in mic records about 30 dB too quietly and with a DC offset, so the level meter stays flat and every recording looks like speech (a tap gets transcribed). There's no hardware gain to raise; fix it system-wide with a PipeWire filter that removes the offset and boosts the mic.

## Setup

### 1. Rust

```bash
sudo pacman -S --needed rustup base-devel
rustup default stable
```

The build also needs the ALSA headers (`alsa-lib`, on every Omarchy install) and `pkg-config` (in `base-devel`). sherpa-onnx's static libraries are downloaded by its build script the first time, so the first build needs the network.

### 2. System packages

```bash
sudo pacman -S --needed pipewire-alsa wl-clipboard wtype libnotify libsecret
```

| Tool | Package | Used for |
|---|---|---|
| ALSA `pipewire` device | pipewire-alsa | recording the mic and playing the start/stop sounds through PipeWire |
| `wl-copy` | wl-clipboard | putting the text on the clipboard |
| `wtype` | wtype | sending Ctrl+V (Ctrl+Shift+V in terminals) |
| `hyprctl` | hyprland | finding the focused window, to pick the cleanup style |
| `notify-send` | libnotify | showing notices when the on-screen pill isn't running |
| `secret-tool` | libsecret | reading the API key from the keyring |

Omarchy already ships most of these. `pw-record` and `pw-play` are no longer used.

### 3. Build and install

From the repository root:

```bash
rust/linux/install.sh
```

This builds a release binary and installs `~/.local/bin/murmur`. On Omarchy it also installs the pill and bar menu plugin to `~/.config/omarchy/plugins/plyhdrn.murmur/` and puts it on the bar, left of the tray, the first time (`omarchy plugin disable plyhdrn.murmur` turns it off; drag the icon to move it). Run it again after pulling changes; it restarts the daemon if one is running.

The speech model (~480 MB) is downloaded into `~/.local/share/murmur/models/` the first time the daemon starts; `murmur status` says `loading` until it's ready. If the Swift build already downloaded it there, it's used as is.

### 4. API key

For Claude cleanup, store your Anthropic API key in the keyring (it prompts for the key, so it doesn't land in shell history):

```bash
secret-tool store --label='Murmur' service com.plyhdrn.murmur account anthropic-api-key
```

This needs a Secret Service provider such as gnome-keyring, which Omarchy uses. Without one, set `ANTHROPIC_API_KEY` for the daemon instead, e.g. `env = ANTHROPIC_API_KEY,…` in Hyprland's config. To use a local model or no cleanup, see [Settings](#settings).

### 5. Hyprland

Start the daemon with your session, and bind push-to-talk. A key with no modifiers is the safest choice: by the time text is pasted you've let go of it, so the paste can't turn into some other shortcut. Right Alt, for example. When a modifier key is pressed on its own, Hyprland sees that modifier as held when it's released, hence `ALT` on the release binding.

Current Omarchy configures Hyprland in Lua. In `~/.config/hypr/autostart.lua`:

```lua
o.launch_on_start(os.getenv("HOME") .. "/.local/bin/murmur daemon")
```

and in `~/.config/hypr/bindings.lua`:

```lua
local murmur = os.getenv("HOME") .. "/.local/bin/murmur"
o.bind("Alt_R", "Start dictation (Murmur)", murmur .. " start")
o.bind("ALT + Alt_R", "Stop dictation (Murmur)", murmur .. " stop", { release = true })
```

With an older `.conf` config, use `exec-once = ~/.local/bin/murmur daemon` in `autostart.conf`, and in `bindings.conf`:

```ini
bind = , Alt_R, exec, ~/.local/bin/murmur start
bindr = ALT, Alt_R, exec, ~/.local/bin/murmur stop
```

**On a MacBook, Fn works too** (like the Mac app's default). Linux's Apple keyboard driver reports Fn as its own key, code 472 to Hyprland, and it isn't a modifier, so neither binding needs one:

```lua
o.bind("code:472", "Start dictation (Murmur)", murmur .. " start")
o.bind("code:472", "Stop dictation (Murmur)", murmur .. " stop", { release = true })
```

Holding Fn for media keys (Fn+F1 and so on, with `hid_apple`'s `fnmode=2`) also starts a recording; it's discarded as `no speech` when you let go.

If the release doesn't fire, drop the `ALT` from the release binding, or use toggle mode, where one press starts and the next press finishes (`murmur toggle` on one key).

Check what's free first with `hyprctl binds`. Reload with `hyprctl reload`, then check `hyprctl configerrors`. Start the daemon once by hand instead of logging out: `hyprctl dispatch 'hl.dsp.exec_cmd("~/.local/bin/murmur daemon")'` (Lua) or `hyprctl dispatch exec "~/.local/bin/murmur daemon"` (`.conf`).

### 6. Check

```bash
murmur doctor
```

It checks each tool, pipewire-alsa, the model, the API key (or the Ollama model), whether the daemon is running and whether the pill plugin is installed, and prints the fix for anything missing.

## Settings

`~/.config/murmur/config.json`, the same file the Swift build used (optional; every field has a default). The bar menu changes `cleanup`, `ollamaModel`, `microphone` and `sounds` with `murmur set <key> <value>`, which keeps the rest of the file as it is; you can also edit it by hand. It's re-read at the start of each dictation (and at once after `murmur set`), except `model` and `threads`, which apply when the daemon starts.

```json
{
  "cleanup": "claude",
  "ollamaModel": "qwen3.5:9b",
  "sounds": "wood",
  "microphone": "alsa_input.usb-Blue_Yeti-00.analog-stereo",
  "threads": 4
}
```

- **cleanup**: `claude` (default), `ollama` (runs `ollamaModel`; `ollama pull` it first) or `off` (paste Parakeet's transcript as-is).
- **sounds**: `wood` (default), `soft`, `chime`, `bubble` or `off`.
- **microphone**: a PipeWire node name from `pw-record --list-targets` (or `wpctl status`); leave it out to use the default input. It's opened through pipewire-alsa as the ALSA device `pipewire:NODE=<name>`. If that can't be opened (no pipewire-alsa, PipeWire not running), Murmur logs it and records from the default input instead.
- **threads**: CPU threads for speech-to-text (default: the number of cores, up to 8; the Swift build defaulted to 4).
- **model**: a different sherpa-onnx Parakeet folder (default `~/.local/share/murmur/models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8`). A folder set here is never downloaded into.

**Dictionary:** `~/.config/murmur/dictionary.json`, the same format as on the Mac (`~/Library/Application Support/Murmur/dictionary.json`), so you can copy that file over. Changes apply on the next dictation. The bar menu's Dictionary button opens it in a card over the screen, a port of the Mac Dictionary window ([`Dictionary.qml`](omarchy-plugin/Dictionary.qml)): Words and Replacements tabs, search, Return to add, double-click or Return to edit a row, Delete to remove, Ctrl+Z to undo, Esc to back out. It saves the file after each change and picks up edits made to it by hand. To open it from a key binding, run `omarchy-shell shell call plyhdrn.murmur openDictionary ""`.

**Cleanup style per app:** the focused window's class picks chat, email, notes or code style (see `TargetContext::from_window_class` in [`context.rs`](../crates/murmur-core/src/context.rs)). Find a window's class with `hyprctl activewindow`. Terminals get Ctrl+Shift+V instead of Ctrl+V (`is_terminal` in [`murmur-platform`'s `linux.rs`](../crates/murmur-platform/src/linux.rs)).

## Testing checklist

Work through these on the Linux machine, roughly in order. Each one isolates one piece.

1. `murmur doctor` reports nothing missing (the model may still be missing before the first daemon start).
2. **Speech model:** `murmur transcribe ~/.local/share/murmur/models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/test_wavs/en.wav --no-cleanup` prints a sensible sentence. Note the load and speech times. (If the model isn't there yet, this downloads it.)
3. **Cleanup and API key:** the same without `--no-cleanup` prints a `Cleanup [claude-… plain]` line rather than "Cleanup failed".
4. **Daemon by hand:** `murmur daemon` in one terminal (it logs `Ready` once the model has loaded); in another, run `murmur start`, speak, then `murmur stop`. The text should paste into that second terminal (with Ctrl+Shift+V). The daemon's terminal shows the timing and `Dictated:` lines.
5. **Microphone:** with no `microphone` setting, step 4 records from the default input. Then set `microphone` to a node name from `pw-record --list-targets` and repeat: the daemon log should not say "unavailable … using the default". Check the pill's level meter moves while you speak.
6. **Sounds:** start and stop each play a short tone, and they don't go to the microphone node.
7. **Key binding:** hold the key, speak, release. Check that it pastes into a browser text field (Ctrl+V), a terminal (Ctrl+Shift+V), and a chat app (chat style).
8. **Pill:** while recording the pill shows the level, then "Murmuring…", and two quick dictations show two cards. Stop the daemon (`pkill -x murmur`) and start it again: the pill reconnects.
9. **Bar menu:** the mic icon turns red while recording. In its menu, pick another sound (it plays as a preview), switch cleanup to Local and back, pick a microphone, and copy a recent dictation; `config.json` keeps any `model` or `threads` you set. Quit dims the icon; Start brings the daemon back.
10. **First word:** say a short sentence right as you press the key. If the first word is often missing, the microphone is starting too slowly (see Ideas).
11. **Taps:** tap the key without speaking. Nothing should paste (`murmur stop` replies `no speech`).
12. **Notices without the pill:** `omarchy plugin disable plyhdrn.murmur`, then dictate with cleanup set to `ollama` and Ollama stopped: a `notify-send` notification says "Cleanup failed, pasted raw text". Re-enable it with `omarchy plugin enable plyhdrn.murmur --section right`.
13. **Autostart:** log out and in; `murmur status` says `ready`.

When something fails, check `~/.local/state/murmur/murmur.log` first.

## Development

```bash
cd rust
cargo build -p murmur
cargo run -p murmur -- doctor
cargo test --workspace
```

The Linux front end is [`crates/murmur/src/linux`](../crates/murmur/src/linux); `main.rs` picks it on Linux and the tray app elsewhere.

| File | What |
|---|---|
| `mod.rs` | The CLI: `--help` and dispatch; `start`/`stop`/`toggle`/`cancel`/`status` send one socket command. |
| `daemon.rs` | Owns the engine, serves socket commands into it, broadcasts its events to watchers, and stops on SIGINT/SIGTERM. |
| `config.rs` | `config.json`'s `model` and `threads` (the rest is `murmur_core::Settings`). |
| `settings.rs` | `murmur settings` (one JSON object for the bar menu: status, settings, PipeWire sources from `pw-dump`, Ollama models, recent dictations) and `murmur set`. |
| `transcribe.rs`, `bench.rs`, `doctor.rs` | The one-shot commands. |

The rest is shared: [`murmur-engine`](../crates/murmur-engine) (recording, queue, cleanup, paste), [`murmur-control`](../crates/murmur-control) (the socket and its line protocol), [`murmur-platform`](../crates/murmur-platform/src/linux.rs) (`hyprctl`, `wl-copy` + `wtype`, `notify-send`) and [`murmur-audio`](../crates/murmur-audio) (ALSA capture, including [`pipewire.rs`](../crates/murmur-audio/src/pipewire.rs) for node names).

The on-screen pill is [`omarchy-plugin/Murmur.qml`](omarchy-plugin/Murmur.qml), a `panel` plugin for the Omarchy shell (Quickshell). It connects to the daemon's socket and sends `watch`; the daemon keeps that connection open and writes one line per event: `state ready|recording|processing`, `queue 2` (dictations waiting to paste, including the one being processed), `level 0.42` (about every 50 ms while recording) and `notice <message>`. While anything is watching, notices go to the pill instead of `notify-send`.

The bar menu is [`omarchy-plugin/MurmurBar.qml`](omarchy-plugin/MurmurBar.qml), the same plugin's `bar-widget`. It watches the socket for the icon, runs `murmur settings` when it opens and `murmur set` when something is picked; `set` sends the daemon `reload`, which applies the change at once (loading a newly picked Ollama model, and playing a newly picked sound). The daemon also answers `recent` with its last five dictations as a JSON array.

The shell reloads a plugin's changed files by itself, but not files added, renamed or deleted and made again; `omarchy restart shell` picks those up (`install.sh` does this when the plugin's file list changes). To work on the plugin, link it instead of copying it, and the shell reloads it on save (`install.sh` leaves a link alone):

```bash
rm -rf ~/.config/omarchy/plugins/plyhdrn.murmur
ln -s "$PWD/linux/omarchy-plugin" ~/.config/omarchy/plugins/plyhdrn.murmur
```

`quickshell log -p /usr/share/omarchy/shell` shows QML errors.

**Checking the Linux build from a Mac,** with Docker: `scripts/linux-docker-test.sh` runs the workspace's tests, clippy and rustfmt in a Debian container (`MURMUR_DOCKER_ARCH=arm64` is much faster on Apple silicon). To run the binary there without downloading the model, mount the Mac's copy:

```bash
MURMUR_MODELS="$HOME/Library/Application Support/Murmur/models" scripts/linux-docker-test.sh bash
cargo build --release -p murmur
target/linux-docker-amd64/release/murmur transcribe <clip.wav> --no-cleanup
```

## Ideas

- **Faster start:** if the first word gets clipped, keep one input stream open and only keep the audio between start and stop.
- **Streaming:** transcribe while you're still talking, as on the Mac roadmap.
- **Packaging:** an AUR `PKGBUILD`, and a systemd user service as an alternative to `exec-once`.
