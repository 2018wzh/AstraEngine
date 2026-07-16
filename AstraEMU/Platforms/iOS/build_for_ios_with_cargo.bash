#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# The Xcode executable handoff follows Slint's documented Rust/iOS build shape.

set -euo pipefail

export PATH="/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:${PATH:-}:$HOME/.cargo/bin"

if [[ $# -lt 1 ]]; then
  echo "ASTRA_EMU_IOS_BINARY_NAME_MISSING" >&2
  exit 2
fi

for name in SRCROOT CONFIGURATION DERIVED_FILE_DIR TARGET_BUILD_DIR EXECUTABLE_PATH ARCHS; do
  if [[ -z "${!name:-}" ]]; then
    echo "ASTRA_EMU_IOS_XCODE_ENV_MISSING:$name" >&2
    exit 2
  fi
done
for name in ASTRA_EMU_FAMILY_SIGNING_KEY_HEX ASTRA_EMU_FAMILY_SIGNER_ID ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX; do
  if [[ -z "${!name:-}" ]]; then
    echo "ASTRA_EMU_IOS_SIGNING_ENV_MISSING:$name" >&2
    exit 2
  fi
done

BINARY_NAME="$1"
shift
REPOSITORY_ROOT="$(cd "$SRCROOT/../../.." && pwd -P)"
if [[ ! -f "$REPOSITORY_ROOT/Cargo.toml" ]]; then
  echo "ASTRA_EMU_IOS_WORKSPACE_ROOT_INVALID" >&2
  exit 2
fi

if [[ "$CONFIGURATION" == "Debug" ]]; then
  CARGO_PROFILE=debug
  PROFILE_ARGS=()
else
  CARGO_PROFILE=release
  PROFILE_ARGS=(--release)
fi
export CARGO_PROFILE_RELEASE_DEBUG="${CARGO_PROFILE_RELEASE_DEBUG:-1}"
export CARGO_TARGET_DIR="$DERIVED_FILE_DIR/cargo"

IS_SIMULATOR=0
if [[ "${LLVM_TARGET_TRIPLE_SUFFIX:-}" == "-simulator" ]]; then
  IS_SIMULATOR=1
fi

executables=()
for arch in $ARCHS; do
  case "$arch:$IS_SIMULATOR" in
    arm64:0) CARGO_TARGET=aarch64-apple-ios ;;
    arm64:1) CARGO_TARGET=aarch64-apple-ios-sim ;;
    x86_64:1) CARGO_TARGET=x86_64-apple-ios ;;
    *)
      echo "ASTRA_EMU_IOS_ARCH_UNSUPPORTED:$arch" >&2
      exit 2
      ;;
  esac

  cargo build --manifest-path "$REPOSITORY_ROOT/Cargo.toml" \
    "${PROFILE_ARGS[@]}" --target "$CARGO_TARGET" -p astra-emu-fvp --lib

  archive="$CARGO_TARGET_DIR/$CARGO_TARGET/$CARGO_PROFILE/libastra_emu_fvp.rlib"
  if [[ ! -s "$archive" ]]; then
    echo "ASTRA_EMU_IOS_FVP_ARCHIVE_MISSING:$CARGO_TARGET" >&2
    exit 2
  fi
  descriptors=()
  while IFS= read -r descriptor; do
    descriptors+=("$descriptor")
  done < <(find "$CARGO_TARGET_DIR/$CARGO_TARGET/$CARGO_PROFILE/build" \
    -path '*/out/astra-fvp-descriptor.json' -type f -print)
  if [[ ${#descriptors[@]} -ne 1 ]]; then
    echo "ASTRA_EMU_IOS_FVP_DESCRIPTOR_IDENTITY:${#descriptors[@]}" >&2
    exit 2
  fi

  metadata_dir="$DERIVED_FILE_DIR/astraemu-static/$CARGO_TARGET"
  mkdir -p "$metadata_dir"
  manifest="$metadata_dir/manifest.json"
  rm -f "$manifest" "$metadata_dir/.manifest.json.tmp"
  cargo run --quiet --manifest-path "$REPOSITORY_ROOT/Cargo.toml" \
    -p astra-emu-family-package -- static-sign \
    --archive "$archive" \
    --descriptor "${descriptors[0]}" \
    --output "$manifest" \
    --target "$CARGO_TARGET" \
    --signer-identity "$ASTRA_EMU_FAMILY_SIGNER_ID"

  export ASTRA_EMU_FVP_STATIC_ARCHIVE="$archive"
  export ASTRA_EMU_FVP_STATIC_MANIFEST="$manifest"
  cargo build --manifest-path "$REPOSITORY_ROOT/Cargo.toml" \
    "${PROFILE_ARGS[@]}" --target "$CARGO_TARGET" --bin "$BINARY_NAME" "$@"

  executable="$CARGO_TARGET_DIR/$CARGO_TARGET/$CARGO_PROFILE/$BINARY_NAME"
  if [[ ! -x "$executable" ]]; then
    echo "ASTRA_EMU_IOS_EXECUTABLE_MISSING:$CARGO_TARGET" >&2
    exit 2
  fi
  executables+=("$executable")
done

mkdir -p "$(dirname "$TARGET_BUILD_DIR/$EXECUTABLE_PATH")"
lipo -create -output "$TARGET_BUILD_DIR/$EXECUTABLE_PATH" "${executables[@]}"

if [[ -n "${DWARF_DSYM_FOLDER_PATH:-}" && -n "${DWARF_DSYM_FILE_NAME:-}" ]]; then
  mkdir -p "$DWARF_DSYM_FOLDER_PATH"
  dsymutil "$TARGET_BUILD_DIR/$EXECUTABLE_PATH" \
    -o "$DWARF_DSYM_FOLDER_PATH/$DWARF_DSYM_FILE_NAME"
fi

if [[ $IS_SIMULATOR -eq 0 && "${CODE_SIGNING_ALLOWED:-YES}" != "NO" ]]; then
  if [[ -z "${EXPANDED_CODE_SIGN_IDENTITY:-}" ]]; then
    echo "ASTRA_EMU_IOS_CODE_SIGN_IDENTITY_MISSING" >&2
    exit 2
  fi
  entitlements="${TARGET_TEMP_DIR:-}/${PRODUCT_NAME:-AstraEMU}.app.xcent"
  if [[ -s "$entitlements" ]]; then
    codesign --force --sign "$EXPANDED_CODE_SIGN_IDENTITY" \
      --entitlements "$entitlements" "$TARGET_BUILD_DIR/$EXECUTABLE_PATH"
  else
    codesign --force --sign "$EXPANDED_CODE_SIGN_IDENTITY" "$TARGET_BUILD_DIR/$EXECUTABLE_PATH"
  fi
fi
