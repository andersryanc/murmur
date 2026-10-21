# Murmur for Linux

Push-to-talk dictation on Linux with Hyprland (built for [Omarchy](https://omarchy.org)). Hold a key, speak, and polished text is pasted wherever your cursor is. It uses the same speech model, cleanup prompt and Dictionary as the Mac app.

```
Hyprland key bind ─▶ murmur start/stop ─▶ daemon ─▶ pw-record ─▶ Parakeet (sherpa-onnx, CPU)
                                                       ─▶ Claude / Ollama cleanup ─▶ wl-copy + wtype
```

- **`murmur daemon`** runs in the background. It loads Parakeet v3 once and listens on a Unix socket (`$XDG_RUNTIME_DIR/murmur/murmur.sock`).
- **`murmur start`** and **`murmur stop`** are bound to a key's press and release in Hyprland. Wayland doesn't let apps watch keys globally, so the compositor does it.
- Cleanup, the Dictionary, the start/stop sounds and the per-app style all come from the shared [`MurmurKit`](../Core/Sources/MurmurKit). Your Mac Dictionary file works here unchanged.
- **The on-screen pill** (like the Mac HUD) is an [Omarchy shell](https://omarchy.org) plugin in [`omarchy-plugin/`](omarchy-plugin). It shows the mic level while you speak, "Murmuring…" while it works, and any problem for a moment afterwards.

## Status

> **For the agent picking this up on the Linux machine:** this was built on a Mac, checked in a `swift:6.2` arm64 Docker container, then set up and used on an Omarchy machine. What's left is in the last list below; work through [Setup](#setup), then [Testing checklist](#testing-checklist), and fix what breaks.

Checked in Docker (Swift 6.2.4, Ubuntu, arm64):

- `scripts/setup.sh` downloads sherpa-onnx 1.13.8 and the model, and the release build links against them.
- `murmur transcribe` gives a correct transcript (about 0.18 s for a 6 s clip after the model is loaded) and the Claude cleanup call works from Linux.
- The daemon's socket commands (start, stop, toggle, cancel, status), the "already running" check, `murmur doctor` and shutdown on SIGTERM.
- MurmurKit's tests pass on Linux.

Checked on the Omarchy machine (x86_64, Swift 6.4.1, Hyprland 0.56 with a Lua config):

- `install.sh` builds and installs; `murmur transcribe` on the test clip is correct (model load 1.6 s, speech 0.24 s).
- The on-screen pill: recording with live levels, processing and notices, in the Omarchy theme, and it reconnects when the daemon restarts.
- Push-to-talk on Right Alt with the Lua bindings below: recording, the sounds, cleanup with Ollama and pasting into a terminal.
- Latency on an i9 MacBook Pro (2019, 8-core i9-9980HK, CPU only): speech 0.3–0.9 s. Ollama cleanup with `qwen3.5:4b` takes ~0.5 s when the previous dictation used the same style, but ~10 s when the style changes (each style's prompt is processed again), and ~25 s when the model has to load; `qwen3.5:9b` is about twice as slow.

Not tested yet:

- Pasting into a browser and a chat app, `notify-send`, reading the API key with `secret-tool`.
- Whether `pw-record`'s start-up clips the first word.

**T2 MacBooks:** the built-in mic records about 30 dB too quietly and with a DC offset, so the level meter stays flat and every recording looks like speech (a tap gets transcribed). There's no hardware gain to raise; fix it system-wide with a PipeWire filter that removes the offset and boosts the mic.

## Setup

### 1. Swift

Murmur needs Swift 6.2 or newer. On Arch/Omarchy, install the prebuilt toolchain from the AUR:

```bash
yay -S swift-bin
```

```bash
swift --version
```

If `swift-bin` is out of date or broken, the alternatives are the official tarball from [swift.org/install/linux](https://www.swift.org/install/linux/) (pick the newest "Universal" or RHEL/Fedora build and add its `usr/bin` to `PATH`) or the `swift-language` AUR package (builds from source; slow).

### 2. System packages

```bash
sudo pacman -S --needed pipewire wl-clipboard wtype libnotify libsecret curl bzip2
```

| Tool | Package | Used for |
|---|---|---|
| `pw-record`, `pw-play` | pipewire | recording the mic, playing the start/stop sounds |
| `wl-copy` | wl-clipboard | putting the text on the clipboard |
| `wtype` | wtype | sending Ctrl+V (Ctrl+Shift+V in terminals) |
| `hyprctl` | hyprland | finding the focused window, to pick the cleanup style |
| `notify-send` | libnotify | showing errors when the on-screen pill isn't running |
| `secret-tool` | libsecret | reading the API key from the keyring |

Omarchy already ships most of these.

### 3. Build and install

From the repository root:

```bash
Linux/scripts/install.sh
```

This runs `scripts/setup.sh` first, which downloads sherpa-onnx's libraries into `Linux/vendor/` (not in git) and the Parakeet model (~480 MB) into `~/.local/share/murmur/models/`. Then it builds a release binary and installs `~/.local/bin/murmur`, with sherpa-onnx's libraries in `~/.local/lib/murmur/`. On Omarchy it also installs the on-screen pill plugin to `~/.config/omarchy/plugins/plyhdrn.murmur/` and enables it the first time (`omarchy plugin disable plyhdrn.murmur` turns it off). Run it again after pulling changes; it restarts the daemon if one is running.

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

If the release doesn't fire, drop the `ALT` from the release binding, or use toggle mode, where one press starts and the next press finishes (`murmur toggle` on one key).

Check what's free first with `hyprctl binds`. Reload with `hyprctl reload`, then check `hyprctl configerrors`. Start the daemon once by hand instead of logging out: `hyprctl dispatch 'hl.dsp.exec_cmd("~/.local/bin/murmur daemon")'` (Lua) or `hyprctl dispatch exec "~/.local/bin/murmur daemon"` (`.conf`).

### 6. Check

```bash
murmur doctor
```

It checks each tool, the model, the API key (or the Ollama model) and whether the daemon is running, and prints the fix for anything missing.

## Settings

`~/.config/murmur/config.json` (optional; every field has a default, and it's re-read at the start of each dictation):

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
- **microphone**: a PipeWire node name from `pw-record --list-targets`; leave it out to use the default input.
- **threads**: CPU threads for speech-to-text.
- **model**: a different sherpa-onnx Parakeet folder (default `~/.local/share/murmur/models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8`).

**Dictionary:** `~/.config/murmur/dictionary.json`, the same format as on the Mac (`~/Library/Application Support/Murmur/dictionary.json`), so you can copy that file over. Changes apply on the next dictation.

**Cleanup style per app:** the focused window's class picks chat, email, notes or code style (see `TargetContext(windowClass:)` in [`TargetContext.swift`](../Core/Sources/MurmurKit/TargetContext.swift)). Find a window's class with `hyprctl activewindow`. Terminals get Ctrl+Shift+V instead of Ctrl+V (`Desktop.isTerminal`).

## Testing checklist

Work through these on the Linux machine, roughly in order. Each one isolates one piece.

1. `murmur doctor` reports nothing missing.
2. **Speech model:** `murmur transcribe ~/.local/share/murmur/models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/test_wavs/en.wav --no-cleanup` prints a sensible sentence. Note the load and speech times.
3. **Microphone:** `pw-record --rate 16000 --channels 1 --format f32 /tmp/t.wav`, speak, Ctrl+C, then `murmur transcribe /tmp/t.wav`. This checks recording, the WAV reader and cleanup together.
4. **API key:** step 3 prints a `Cleanup [claude-haiku-5-5 …]` line rather than "Cleanup failed".
5. **Daemon by hand:** `murmur daemon` in one terminal; in another, run `murmur start`, speak, then `murmur stop`. The text should paste into that second terminal (with Ctrl+Shift+V). The daemon's terminal shows the log line.
6. **Sounds:** start and stop each play a short tone.
7. **Key binding:** hold the key, speak, release. Check that it pastes into a browser text field (Ctrl+V), a terminal (Ctrl+Shift+V), and a chat app (chat style).
8. **First word:** say a short sentence right as you press the key. If the first word is often missing, `pw-record` is starting too slowly (see Ideas).
9. **Taps:** tap the key without speaking. Nothing should paste (the log says "No speech").
10. **Autostart:** log out and in; `murmur status` says `ready`.

When something fails, check `~/.local/state/murmur/murmur.log` first.

## Development

```bash
cd Linux
scripts/setup.sh            # once: sherpa-onnx into vendor/, model into ~/.local/share
swift build
swift run murmur doctor
```

The pieces, all in [`Sources/murmur`](Sources/murmur):

| File | What |
|---|---|
| `main.swift` | The CLI: `daemon`, `start`/`stop`/`toggle`/`cancel`/`status`, `transcribe`, `doctor`. |
| `Daemon.swift` | The push-to-talk loop: record → transcribe → Dictionary → cleanup → paste. |
| `ControlSocket.swift` | The Unix socket between the CLI and the daemon. |
| `Transcriber.swift` | Parakeet through sherpa-onnx's C API (`Sources/CSherpaOnnx`). |
| `Audio.swift` | Recording with `pw-record`, reading WAV files, playing sounds with `pw-play`. |
| `Desktop.swift` | `hyprctl`, `wl-copy` + `wtype`, `notify-send`. |
| `Config.swift` | `config.json` and choosing the cleaner. |
| `Support.swift` | XDG paths, the log, running command-line tools. |

The on-screen pill is [`omarchy-plugin/Murmur.qml`](omarchy-plugin/Murmur.qml), a `panel` plugin for the Omarchy shell (Quickshell). It connects to the daemon's socket and sends `watch`; the daemon keeps that connection open and writes one line per event: `state ready|recording|processing`, `queue 2` (dictations waiting to paste, including the one being processed), `level 0.42` (about every 50 ms while recording, measured from the WAV file `pw-record` is writing), and `notice <message>`. While anything is watching, notices go to the pill instead of `notify-send`. To work on the plugin, link it instead of copying it, and the shell reloads it on save (`install.sh` leaves a link alone):

```bash
rm -rf ~/.config/omarchy/plugins/plyhdrn.murmur
ln -s "$PWD/omarchy-plugin" ~/.config/omarchy/plugins/plyhdrn.murmur
```

`quickshell log -p /usr/share/omarchy/shell` shows QML errors.

`Package.swift` finds sherpa-onnx in `vendor/sherpa-onnx` (or `$SHERPA_ONNX_DIR`). The binary looks for the libraries next to the installed binary first (`~/.local/lib/murmur`), then in `vendor/`. Shared logic belongs in `Core/Sources/MurmurKit`, which must stay plain Foundation so it builds on both systems (`cd Core && swift test` runs its tests on either).

**Checking the Linux build from a Mac,** with Docker:

```bash
docker run --rm -v "$PWD":/src:ro swift:6.2 bash -c 'apt-get update -qq && apt-get install -y -qq curl bzip2 >/dev/null && cp -r /src/Core /src/Linux /tmp/ && cd /tmp/Linux && rm -rf .build vendor && scripts/setup.sh && swift build'
```

(This downloads the model too; mount an existing copy at `/root/.local/share/murmur/models` to skip that.)

## Ideas

- **Faster start:** if `pw-record` clips the first word, keep one recording stream open and only keep the audio between start and stop.
- **Streaming:** transcribe while you're still talking, as on the Mac roadmap.
- **Packaging:** an AUR `PKGBUILD`, and a systemd user service as an alternative to `exec-once`.
