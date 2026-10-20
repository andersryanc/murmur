#!/usr/bin/env bash
# Builds murmur (release) and installs it to ~/.local/bin, with sherpa-onnx's libraries in
# ~/.local/lib/murmur. Restarts the daemon if it's running.
set -euo pipefail
cd "$(dirname "$0")/.."

scripts/setup.sh

flags=(-c release --static-swift-stdlib)
swift build "${flags[@]}"
bin="$(swift build "${flags[@]}" --show-bin-path)/murmur"

mkdir -p ~/.local/bin ~/.local/lib/murmur
install -m 755 "$bin" ~/.local/bin/murmur
cp -P vendor/sherpa-onnx/lib/*.so* ~/.local/lib/murmur/
echo "Installed ~/.local/bin/murmur"

# The on-screen pill is an Omarchy shell plugin. A symlink there (for working on the plugin)
# is left alone. It's enabled on the first install only, so disabling it sticks.
plugin=~/.config/omarchy/plugins/plyhdrn.murmur
if command -v omarchy-shell >/dev/null && [[ ! -L $plugin ]]; then
  first=$([[ -d $plugin ]] || echo yes)
  rm -rf "$plugin" && mkdir -p "$plugin"
  cp omarchy-plugin/* "$plugin/"
  omarchy-shell shell rescanPlugins >/dev/null 2>&1 || true
  if [[ -n $first ]]; then
    sleep 1
    omarchy plugin enable plyhdrn.murmur >/dev/null 2>&1 \
      || echo "Enable the on-screen pill with: omarchy plugin enable plyhdrn.murmur"
  fi
  echo "Installed the Omarchy plugin to $plugin"
fi

if pkill -x murmur; then
  sleep 0.5
  if command -v hyprctl >/dev/null && [[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]]; then
    # A Lua Hyprland config takes a Lua expression here; older configs take the plain form.
    hyprctl dispatch 'hl.dsp.exec_cmd("~/.local/bin/murmur daemon")' 2>/dev/null | grep -qx ok \
      || hyprctl dispatch exec "~/.local/bin/murmur daemon" >/dev/null
    echo "Restarted the daemon"
  else
    echo "Stopped the daemon; start it again with: murmur daemon"
  fi
fi
