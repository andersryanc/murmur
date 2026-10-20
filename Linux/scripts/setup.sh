#!/usr/bin/env bash
# Downloads what the Linux build needs and doesn't keep in git: sherpa-onnx's libraries (into
# Linux/vendor/sherpa-onnx) and the Parakeet speech model (into ~/.local/share/murmur/models,
# ~480 MB). Safe to run again; it skips anything already there.
set -euo pipefail
cd "$(dirname "$0")/.."

SHERPA_VERSION=1.13.8
MODEL=sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8

case "$(uname -m)" in
  x86_64) build=linux-x64-shared ;;
  aarch64) build=linux-aarch64-shared-cpu ;;
  *) echo "No sherpa-onnx build for $(uname -m)" >&2; exit 1 ;;
esac

vendor=vendor/sherpa-onnx
if [[ "$(cat "$vendor/VERSION" 2>/dev/null)" != "$SHERPA_VERSION" ]]; then
  echo "Downloading sherpa-onnx $SHERPA_VERSION ($build)…"
  rm -rf "$vendor" && mkdir -p "$vendor"
  curl -fL --progress-bar "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$SHERPA_VERSION/sherpa-onnx-v$SHERPA_VERSION-$build.tar.bz2" \
    | tar xj -C "$vendor" --strip-components=1
  rm -rf "$vendor/bin"
  # Some builds ship without headers; take the one from the same release.
  header="$vendor/include/sherpa-onnx/c-api/c-api.h"
  if [[ ! -f "$header" ]]; then
    mkdir -p "$(dirname "$header")"
    curl -fsSL "https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v$SHERPA_VERSION/sherpa-onnx/c-api/c-api.h" -o "$header"
  fi
  echo "$SHERPA_VERSION" > "$vendor/VERSION"
fi

models="${XDG_DATA_HOME:-$HOME/.local/share}/murmur/models"
if [[ ! -f "$models/$MODEL/encoder.int8.onnx" ]]; then
  echo "Downloading the Parakeet speech model (~480 MB)…"
  mkdir -p "$models"
  curl -fL --progress-bar "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/$MODEL.tar.bz2" \
    | tar xj -C "$models"
fi

echo "sherpa-onnx: $PWD/$vendor"
echo "Model: $models/$MODEL"
