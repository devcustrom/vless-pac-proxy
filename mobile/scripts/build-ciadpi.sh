#!/usr/bin/env bash
# Rebuilds the ByeDPI (ciadpi) executables packaged as libciadpi.so.
# Requirements: Android NDK 27, git, curl, unzip.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NDK="${ANDROID_NDK_HOME:-${ANDROID_NDK:-}}"
if [ -z "$NDK" ]; then
  echo "Set ANDROID_NDK_HOME to the NDK 27 directory" >&2
  exit 1
fi

case "$(uname -s)" in
  Linux*) HOST=linux-x86_64 ;;
  Darwin*) HOST=darwin-x86_64 ;;
  MINGW*|MSYS*|CYGWIN*) HOST=windows-x86_64 ;;
  *) echo "Unsupported host" >&2; exit 1 ;;
esac
BIN="$NDK/toolchains/llvm/prebuilt/$HOST/bin"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
git clone --depth 1 https://github.com/hufrea/byedpi "$WORK/byedpi"

build() {
  local abi="$1" target="$2"
  local cc="$BIN/${target}26-clang"
  [ -x "$cc" ] || cc="$cc.cmd"
  mkdir -p "$ROOT/android/app/src/main/jniLibs/$abi"
  "$cc" -D_DEFAULT_SOURCE -I"$WORK/byedpi" -std=c99 -O2 -Wall \
    -Wno-unused -Wextra -Wno-unused-parameter -pedantic -fPIE -pie \
    -o "$ROOT/android/app/src/main/jniLibs/$abi/libciadpi.so" \
    "$WORK/byedpi/packets.c" "$WORK/byedpi/main.c" "$WORK/byedpi/conev.c" \
    "$WORK/byedpi/proxy.c" "$WORK/byedpi/desync.c" "$WORK/byedpi/mpool.c" \
    "$WORK/byedpi/extend.c"
  echo "built $abi"
}

build arm64-v8a aarch64-linux-android
build x86_64 x86_64-linux-android
