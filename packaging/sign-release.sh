#!/bin/bash
# Release signing tool for Lumen
# Signs release artifacts and generates version.json manifest

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

VERSION="${1:-}"
PRIVATE_KEY="${LUMEN_SIGNING_KEY:-}"

if [ -z "$VERSION" ]; then
    echo "Usage: $0 <version> [--generate-key]"
    echo ""
    echo "Environment variables:"
    echo "  LUMEN_SIGNING_KEY - Ed25519 private key (hex)"
    echo ""
    echo "Options:"
    echo "  --generate-key    Generate a new signing keypair"
    exit 1
fi

if [ "$VERSION" = "--generate-key" ]; then
    echo "Generating new Ed25519 signing keypair..."
    echo ""

    # Use the orchestrator to generate keys
    cd "$PROJECT_ROOT"
    cargo build --release -p lumen 2>/dev/null

    # Generate keys via Rust (using a simple inline program)
    KEYS=$(cat << 'RUSTCODE' | cargo script --
//! ```cargo
//! [dependencies]
//! ed25519-dalek = { version = "2.1", features = ["rand_core"] }
//! rand = "0.8"
//! hex = "0.4"
//! ```
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;

fn main() {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();

    println!("PRIVATE_KEY={}", hex::encode(signing_key.to_bytes()));
    println!("PUBLIC_KEY={}", hex::encode(verifying_key.to_bytes()));
}
RUSTCODE
    ) || {
        # Fallback: use openssl if cargo-script not available
        echo "Note: Using OpenSSL for key generation"
        PRIVKEY=$(openssl genpkey -algorithm ed25519 2>/dev/null | openssl pkey -outform DER 2>/dev/null | tail -c 32 | xxd -p -c 64)
        # This won't work perfectly - ed25519-dalek uses different format
        echo "WARNING: OpenSSL keys may not be compatible. Install cargo-script for proper key generation."
        echo ""
        echo "To install cargo-script:"
        echo "  cargo install cargo-script"
        exit 1
    }

    echo "$KEYS"
    echo ""
    echo "IMPORTANT: Store the PRIVATE_KEY securely! Add it to GitHub Secrets as LUMEN_SIGNING_KEY"
    echo "The PUBLIC_KEY should be added to orchestrator/src/config.rs in UpdateConfig::public_key"
    exit 0
fi

if [ -z "$PRIVATE_KEY" ]; then
    echo "ERROR: LUMEN_SIGNING_KEY environment variable not set"
    echo ""
    echo "Set it with:"
    echo "  export LUMEN_SIGNING_KEY='your-hex-encoded-private-key'"
    echo ""
    echo "Or generate a new key with:"
    echo "  $0 --generate-key"
    exit 1
fi

RELEASES_DIR="$PROJECT_ROOT/releases"
APPIMAGE="$RELEASES_DIR/Lumen-${VERSION}-x86_64.AppImage"

if [ ! -f "$APPIMAGE" ]; then
    echo "ERROR: AppImage not found: $APPIMAGE"
    echo "Run build-appimage.sh first"
    exit 1
fi

echo "Signing release v${VERSION}..."

# Compute SHA256
SHA256=$(sha256sum "$APPIMAGE" | cut -d' ' -f1)
echo "SHA256: $SHA256"

# Sign the hash using ed25519-dalek via Rust
SIGNATURE=$(cat << RUSTCODE | cargo script -- "$PRIVATE_KEY" "$SHA256"
//! \`\`\`cargo
//! [dependencies]
//! ed25519-dalek = "2.1"
//! hex = "0.4"
//! \`\`\`
use ed25519_dalek::{Signer, SigningKey};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let private_key_hex = &args[1];
    let hash_hex = &args[2];

    let private_bytes = hex::decode(private_key_hex).expect("Invalid private key hex");
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&private_bytes);
    let signing_key = SigningKey::from_bytes(&key_bytes);

    let hash_bytes = hex::decode(hash_hex).expect("Invalid hash hex");
    let signature = signing_key.sign(&hash_bytes);

    println!("{}", hex::encode(signature.to_bytes()));
}
RUSTCODE
) || {
    echo "ERROR: Could not sign release. Install cargo-script:"
    echo "  cargo install cargo-script"
    exit 1
}

echo "Signature: ${SIGNATURE:0:32}..."

# Get file size
SIZE=$(stat -f%z "$APPIMAGE" 2>/dev/null || stat -c%s "$APPIMAGE")

# Generate version.json
cat > "$RELEASES_DIR/version.json" << EOF
{
  "version": "${VERSION}",
  "sha256": "${SHA256}",
  "signature": "${SIGNATURE}",
  "min_version": null,
  "release_notes": "Lumen v${VERSION}",
  "released_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "downloads": {
    "linux_x86_64": "https://github.com/user/lumen/releases/download/v${VERSION}/Lumen-${VERSION}-x86_64.AppImage",
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
echo ""
echo "To create GitHub release:"
echo "  gh release create v${VERSION} \\"
echo "    releases/Lumen-${VERSION}-x86_64.AppImage \\"
echo "    releases/version.json \\"
echo "    --title 'Lumen v${VERSION}' \\"
echo "    --notes 'Auto-updating Cardano node'"
