#!/usr/bin/env bash
# Builds murmur (release) and installs it to ~/.local/bin, installs and enables the Omarchy pill
# plugin, and restarts the daemon if it's running. sherpa-onnx is linked statically on Linux, so
# the binary is all there is to install. The speech model (~480 MB) downloads into
# ~/.local/share/murmur/models the first time the daemon starts, if it isn't there already.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/.."

cargo build --release -p murmur
mkdir -p ~/.local/bin
install -m 755 target/release/murmur ~/.local/bin/murmur
echo "Installed ~/.local/bin/murmur"

# The on-screen pill is an Omarchy shell plugin. A symlink there (for working on the plugin)
# is left alone. It's enabled on the first install only, so disabling it sticks.
plugin=~/.config/omarchy/plugins/plyhdrn.murmur
if command -v omarchy-shell >/dev/null && [[ ! -L $plugin ]]; then
  first=$([[ -d $plugin ]] || echo yes)
  rm -rf "$plugin" && mkdir -p "$plugin"
  cp linux/omarchy-plugin/* "$plugin/"
  omarchy-shell shell rescanPlugins >/dev/null 2>&1 || true
  if [[ -n $first ]]; then
    sleep 1
    omarchy plugin enable plyhdrn.murmur >/dev/null 2>&1 \
      || echo "Enable the on-screen pill with: omarchy plugin enable plyhdrn.murmur"
  fi
  echo "Installed the Omarchy plugin to $plugin"
fi

# A running daemon is still the old binary. SIGTERM lets it remove its socket.
if pkill -x murmur; then
  for _ in $(seq 20); do pgrep -x murmur >/dev/null || break; sleep 0.1; done
  if command -v hyprctl >/dev/null && [[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]]; then
    # A Lua Hyprland config takes a Lua expression here; older configs take the plain form.
    hyprctl dispatch 'hl.dsp.exec_cmd("~/.local/bin/murmur daemon")' 2>/dev/null | grep -qx ok \
      || hyprctl dispatch exec "~/.local/bin/murmur daemon" >/dev/null
    echo "Restarted the daemon"
  else
    echo "Stopped the daemon; start it again with: murmur daemon"
  fi
fi
