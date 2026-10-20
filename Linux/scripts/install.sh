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

if pkill -x murmur; then
  sleep 0.5
  if command -v hyprctl >/dev/null && [[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]]; then
    hyprctl dispatch exec "~/.local/bin/murmur daemon" >/dev/null
    echo "Restarted the daemon"
  else
    echo "Stopped the daemon; start it again with: murmur daemon"
  fi
fi
