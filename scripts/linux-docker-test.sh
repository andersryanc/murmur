#!/usr/bin/env bash
# Builds and tests the Linux side of the workspace in a Docker container, so it can be checked
# from a Mac. The whole repository is mounted, and the build goes to target/linux-docker-<arch>
# so it never touches the macOS build.
#
#   scripts/linux-docker-test.sh                 # cargo test, clippy and fmt on the whole workspace
#   scripts/linux-docker-test.sh cargo build -p squawk-control   # any other command
#
# SQUAWK_DOCKER_ARCH=amd64 (default, what the Omarchy machine runs; emulated on Apple silicon)
# or arm64 (native on Apple silicon, much faster, fine for compile checks).
set -euo pipefail

arch="${SQUAWK_DOCKER_ARCH:-amd64}"
rust_image="rust:1.99.0-bookworm"
image="squawk-linux-test:$arch"
repo_dir="$(cd "$(dirname "$0")/.." && pwd)"

# libasound2-dev and pkg-config for cpal (ALSA). sherpa-onnx's static Linux build is downloaded
# by its build script and links against libstdc++, which the image already has.
docker build --quiet --platform "linux/$arch" -t "$image" - >/dev/null <<EOF
FROM $rust_image
RUN apt-get update \
 && apt-get install -y --no-install-recommends libasound2-dev pkg-config \
 && rm -rf /var/lib/apt/lists/*
RUN rustup component add clippy rustfmt
EOF

# The tray app's GUI dependencies are left out of the Linux build, so the whole workspace builds
# here without GTK or WebKitGTK.
if [ $# -eq 0 ]; then
    set -- bash -c "cargo test --workspace \
        && cargo clippy --workspace --all-targets -- -D warnings \
        && cargo fmt --check"
fi

# SQUAWK_MODELS=<folder> mounts a models folder read-only where the Linux build looks for it,
# so running the binary here skips the slow download, e.g. the Mac app's:
#   SQUAWK_MODELS="$HOME/Library/Application Support/Squawk/models" scripts/linux-docker-test.sh bash
extra=()
if [ -n "${SQUAWK_MODELS:-}" ]; then
    extra+=(-v "$SQUAWK_MODELS":/root/.local/share/squawk/models:ro)
fi

# Named volumes keep the crate downloads and sherpa-onnx's archive between runs.
exec docker run --rm --platform "linux/$arch" ${extra[@]+"${extra[@]}"} \
    -v "$repo_dir":/squawk \
    -v "squawk-cargo-registry-$arch":/usr/local/cargo/registry \
    -w /squawk \
    -e CARGO_TARGET_DIR="/squawk/target/linux-docker-$arch" \
    -e CARGO_TERM_COLOR=always \
    "$image" "$@"
