#!/usr/bin/env bash
# Installs the PS5 payload SDK that builds the Kite helper, pinned and checksum-checked.
#
#   PS5_PAYLOAD_SDK=/opt/ps5-payload-sdk bash scripts/install-ps5-sdk.sh
#
# It also needs LLVM (clang, lld and llvm-config); CI uses Ubuntu's LLVM 19 and points the SDK
# at it with LLVM_CONFIG=/usr/bin/llvm-config-19.
set -euo pipefail

TAG=v0.45
SHA256=5fac64093d0e3e69f2584ce7935f24f703f622d9681f1495c85b9f00d1291a2c
URL="https://github.com/ps5-payload-dev/sdk/releases/download/${TAG}/ps5-payload-sdk.zip"
DEST="${PS5_PAYLOAD_SDK:-/opt/ps5-payload-sdk}"

work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

echo "Downloading the PS5 payload SDK ${TAG}"
curl -fsSL --retry 3 -o "$work/sdk.zip" "$URL"
echo "${SHA256}  $work/sdk.zip" | sha256sum -c -

unzip -q "$work/sdk.zip" -d "$work/unpacked"
rm -rf -- "$DEST"
mkdir -p "$(dirname "$DEST")"
mv "$work/unpacked/ps5-payload-sdk" "$DEST"
chmod +x "$DEST"/bin/*
echo "Installed in $DEST"
