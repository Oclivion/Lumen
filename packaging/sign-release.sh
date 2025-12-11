#!/bin/bash
# Release signing tool for Lumen
# Signs release artifacts and generates version.json manifest

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

VERSION="${1:-}"
PRIVATE_KEY="${LUMEN_SIGNING_KEY:-}"

if [ -z "$VERSION" ]; then
    echo "Usage: $0 <version>"
    echo ""
    echo "Environment variables:"
    echo "  LUMEN_SIGNING_KEY - Ed25519 private key (base64, optional)"
    exit 1
fi

RELEASES_DIR="$PROJECT_ROOT/releases"
APPIMAGE="$RELEASES_DIR/Lumen-${VERSION}-x86_64.AppImage"

if [ ! -f "$APPIMAGE" ]; then
    echo "ERROR: AppImage not found: $APPIMAGE"
    echo "Run build-appimage.sh first"
    exit 1
fi

echo "Processing release v${VERSION}..."

# Compute SHA256
SHA256=$(sha256sum "$APPIMAGE" | cut -d' ' -f1)
echo "SHA256: $SHA256"

# Sign if key is available
SIGNATURE=""
if [ -n "$PRIVATE_KEY" ]; then
    echo "Signing release..."
    # Create a temporary key file from the base64 private key
    TMPKEY=$(mktemp)
    trap "rm -f $TMPKEY" EXIT

    # Decode the base64 private key to a PEM file
    echo "-----BEGIN PRIVATE KEY-----" > "$TMPKEY"
    echo "$PRIVATE_KEY" >> "$TMPKEY"
    echo "-----END PRIVATE KEY-----" >> "$TMPKEY"

    # Sign the hash (use temp file because -rawin doesn't work with stdin)
    TMPHASH=$(mktemp)
    echo -n "$SHA256" > "$TMPHASH"
    SIGNATURE=$(openssl pkeyutl -sign -inkey "$TMPKEY" -rawin -in "$TMPHASH" 2>/dev/null | base64 -w0) || true
    rm -f "$TMPHASH"

    if [ -n "$SIGNATURE" ]; then
        echo "Signature: ${SIGNATURE:0:32}..."
    else
        echo "WARNING: Failed to sign. Continuing without signature."
    fi
else
    echo "WARNING: LUMEN_SIGNING_KEY not set. Release will not be signed."
    echo "To sign releases, set LUMEN_SIGNING_KEY to a base64-encoded Ed25519 private key."
fi

# Get file size
SIZE=$(stat -c%s "$APPIMAGE" 2>/dev/null || stat -f%z "$APPIMAGE")

# Generate version.json
cat > "$RELEASES_DIR/version.json" << EOF
{
  "version": "${VERSION}",
  "sha256": "${SHA256}",
  "signature": ${SIGNATURE:+\"$SIGNATURE\"}${SIGNATURE:-null},
  "min_version": null,
  "release_notes": "Lumen v${VERSION}",
  "released_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "downloads": {
    "linux_x86_64": "https://github.com/Oclivion/Lumen/releases/download/v${VERSION}/Lumen-${VERSION}-x86_64.AppImage",
    "linux_aarch64": null,
    "darwin_x86_64": null,
    "darwin_aarch64": null,
    "windows_x86_64": null
  },
  "size": ${SIZE}
}
EOF

echo ""
echo "Generated: $RELEASES_DIR/version.json"
echo ""
echo "Files ready for release:"
echo "  - Lumen-${VERSION}-x86_64.AppImage"
echo "  - Lumen-${VERSION}-x86_64.AppImage.sha256"
echo "  - version.json"
