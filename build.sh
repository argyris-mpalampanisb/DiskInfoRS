#!/usr/bin/env bash
# Build DiskInfoRS.
#   ./build.sh            Linux: release AppImage -> dist/DiskInfoRS-<version>-<arch>.AppImage
#                         macOS: universal .dmg    -> dist/DiskInfoRS-<version>-macos-universal.dmg
#   ./build.sh test       unit tests, svelte-check, clippy
#   ./build.sh dev        debug build, then run with demo data
#   ./build.sh icons      re-render all icons from app/src-tauri/icons/icon.svg
#
# Environment:
#   MAC_TARGET=aarch64-apple-darwin   build a single-arch macOS bundle instead of universal
#
# Requirements (both): rustup, Node.js >= 18, tauri-cli 2 (`cargo install tauri-cli --version '^2'`).
# Linux also: <arch>-unknown-linux-musl target, WebKitGTK 4.1 + GTK3 dev packages, rsvg-convert.
# macOS also: Xcode command line tools.
set -euo pipefail
cd "$(dirname "$0")"
ROOT=$PWD
: "${CARGO_TARGET_DIR:=$ROOT/target}"
export CARGO_TARGET_DIR
TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
OS=$(uname -s)
MODE=${1:-release}
VERSION=$(tr -d '\r' < Cargo.toml | sed -n 's/^version = "\(.*\)"/\1/p' | head -1)

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }

# Linux only: the privileged helper is a static musl binary bundled as a Tauri sidecar.
helper() {
  [[ $OS == Linux ]] || return 0
  step "Building static diskinfo-helper (musl)"
  local musl="${TRIPLE/-gnu/-musl}"
  rustup target list --installed | grep -qx "$musl" || rustup target add "$musl"
  cargo build --release -p diskinfo-helper --target "$musl"
  mkdir -p app/src-tauri/binaries
  install -m 0755 "$CARGO_TARGET_DIR/$musl/release/diskinfo-helper" "app/src-tauri/binaries/diskinfo-helper-$TRIPLE"
}

# Icons are committed. They are rendered only when missing (git checkouts don't keep
# mtimes, so "svg newer than png" is unreliable); `./build.sh icons` forces a re-render.
icons() {
  if [[ ! -f app/src-tauri/icons/icon.png || ${1:-} == force ]]; then
    step "Rendering icons"
    local tmp; tmp=$(mktemp -d)
    rsvg-convert -w 1024 -h 1024 app/src-tauri/icons/icon.svg -o "$tmp/icon.png"
    (cd app/src-tauri && cargo tauri icon "$tmp/icon.png" -o icons >/dev/null && rm -rf icons/ios icons/android)
    rm -rf "$tmp"
  fi
}

ui() {
  step "Building UI"
  (cd app/ui && { [[ -d node_modules ]] || if [[ -f package-lock.json ]]; then npm ci --no-audit --no-fund; else npm install --no-audit --no-fund; fi; } && npm run build)
}

release_linux() {
  step "Bundling AppImage"
  (cd app/src-tauri && cargo tauri build --bundles appimage)
  mkdir -p dist
  local src out
  src=$(ls -t "$CARGO_TARGET_DIR"/release/bundle/appimage/*.AppImage | head -1)
  out="dist/DiskInfoRS-$VERSION-${TRIPLE%%-*}.AppImage"
  cp "$src" "$out"
  chmod +x "$out"
  step "Done: $out"
}

release_macos() {
  local target=${MAC_TARGET:-universal-apple-darwin} label
  if [[ $target == universal-apple-darwin ]]; then
    for t in aarch64-apple-darwin x86_64-apple-darwin; do
      rustup target list --installed | grep -qx "$t" || rustup target add "$t"
    done
    label=universal
  else
    rustup target list --installed | grep -qx "$target" || rustup target add "$target"
    label=${target%%-*}
  fi
  step "Bundling .app / .dmg ($target)"
  (cd app/src-tauri && cargo tauri build --target "$target" --bundles app,dmg)
  mkdir -p dist
  local src out
  src=$(ls -t "$CARGO_TARGET_DIR/$target"/release/bundle/dmg/*.dmg | head -1)
  out="dist/DiskInfoRS-$VERSION-macos-$label.dmg"
  cp "$src" "$out"
  step "Done: $out"
}

case "$MODE" in
  test)
    step "Unit tests"
    cargo test -p smart-core -p diskinfo-helper
    helper; icons; ui
    step "Svelte type check"
    (cd app/ui && npx svelte-check --tsconfig ./tsconfig.json --threshold warning)
    cargo test -p diskinfors
    step "Clippy"
    cargo clippy --workspace --all-targets -- -D warnings
    ;;
  dev)
    helper; icons; ui
    step "Debug build"
    cargo build -p diskinfors
    if [[ $OS == Linux ]]; then
      install -m 0755 "app/src-tauri/binaries/diskinfo-helper-$TRIPLE" "$CARGO_TARGET_DIR/debug/diskinfo-helper"
    fi
    "$CARGO_TARGET_DIR/debug/diskinfors" --demo
    ;;
  release)
    helper; icons; ui
    case "$OS" in
      Linux) release_linux ;;
      Darwin) release_macos ;;
      *) echo "unsupported OS: $OS" >&2; exit 1 ;;
    esac
    ;;
  icons) icons force ;;
  *) echo "usage: $0 [release|test|dev|icons]"; exit 2 ;;
esac
