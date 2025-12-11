#!/bin/bash
# Build script for Lumen AppImage
# Creates a self-contained AppImage with all dependencies

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$PROJECT_ROOT/build/appimage"
VERSION="${1:-$(grep '^version' "$PROJECT_ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')}"

echo "=========================================="
echo "Building Lumen AppImage v${VERSION}"
echo "=========================================="

# Clean previous build
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR/AppDir/usr/bin"
mkdir -p "$BUILD_DIR/AppDir/usr/lib"
mkdir -p "$BUILD_DIR/AppDir/usr/share/lumen/config"
mkdir -p "$BUILD_DIR/AppDir/usr/share/icons/hicolor/256x256/apps"

# Build the orchestrator
echo "Building orchestrator..."
cd "$PROJECT_ROOT"
cargo build --release -p lumen

# Copy orchestrator binary
cp "$PROJECT_ROOT/target/release/lumen" "$BUILD_DIR/AppDir/usr/bin/"
strip "$BUILD_DIR/AppDir/usr/bin/lumen"

# Download cardano-node and cardano-cli if not cached
CARDANO_VERSION="9.2.1"
CARDANO_CACHE="$PROJECT_ROOT/.cache/cardano"
mkdir -p "$CARDANO_CACHE"

if [ ! -f "$CARDANO_CACHE/cardano-node-$CARDANO_VERSION/cardano-node" ]; then
    echo "Downloading cardano-node v${CARDANO_VERSION}..."
    cd "$CARDANO_CACHE"

    # Download from IOG releases
    CARDANO_URL="https://github.com/IntersectMBO/cardano-node/releases/download/${CARDANO_VERSION}/cardano-node-${CARDANO_VERSION}-linux.tar.gz"

    if ! curl -L -o "cardano-node.tar.gz" "$CARDANO_URL"; then
        echo "WARNING: Could not download cardano-node from GitHub."
        echo "You may need to download it manually and place it in:"
        echo "  $CARDANO_CACHE/cardano-node-$CARDANO_VERSION/"
    else
        mkdir -p "cardano-node-$CARDANO_VERSION"
        tar xzf "cardano-node.tar.gz" -C "cardano-node-$CARDANO_VERSION" --strip-components=1
        rm "cardano-node.tar.gz"
    fi
fi

# Copy cardano binaries if available (check both flat and bin/ subdirectory structures)
CARDANO_BIN_DIR="$CARDANO_CACHE/cardano-node-$CARDANO_VERSION"
if [ -f "$CARDANO_BIN_DIR/bin/cardano-node" ]; then
    CARDANO_BIN_DIR="$CARDANO_CACHE/cardano-node-$CARDANO_VERSION/bin"
fi

if [ -f "$CARDANO_BIN_DIR/cardano-node" ]; then
    echo "Bundling cardano-node..."
    cp "$CARDANO_BIN_DIR/cardano-node" "$BUILD_DIR/AppDir/usr/bin/"
    cp "$CARDANO_BIN_DIR/cardano-cli" "$BUILD_DIR/AppDir/usr/bin/"
    strip "$BUILD_DIR/AppDir/usr/bin/cardano-node" 2>/dev/null || true
    strip "$BUILD_DIR/AppDir/usr/bin/cardano-cli" 2>/dev/null || true
else
    echo "WARNING: cardano-node not found. AppImage will require system cardano-node."
fi

# Download mithril-client if not cached
MITHRIL_VERSION="2445.0"
if [ ! -f "$CARDANO_CACHE/mithril-client-$MITHRIL_VERSION" ]; then
    echo "Downloading mithril-client v${MITHRIL_VERSION}..."
    MITHRIL_URL="https://github.com/input-output-hk/mithril/releases/download/${MITHRIL_VERSION}/mithril-${MITHRIL_VERSION}-linux-x64.tar.gz"

    if curl -L -o "$CARDANO_CACHE/mithril.tar.gz" "$MITHRIL_URL" 2>/dev/null; then
        cd "$CARDANO_CACHE"
        tar xzf mithril.tar.gz
        mv mithril-client "mithril-client-$MITHRIL_VERSION" 2>/dev/null || true
        rm -f mithril.tar.gz
    else
        echo "WARNING: Could not download mithril-client."
    fi
fi

if [ -f "$CARDANO_CACHE/mithril-client-$MITHRIL_VERSION" ]; then
    echo "Bundling mithril-client..."
    cp "$CARDANO_CACHE/mithril-client-$MITHRIL_VERSION" "$BUILD_DIR/AppDir/usr/bin/mithril-client"
    strip "$BUILD_DIR/AppDir/usr/bin/mithril-client" 2>/dev/null || true
fi

# Bundle required libraries
echo "Bundling shared libraries..."
cd "$BUILD_DIR/AppDir"

# Function to copy library and its dependencies
copy_lib() {
    local lib="$1"
    if [ -f "$lib" ]; then
        local libname=$(basename "$lib")
        if [ ! -f "usr/lib/$libname" ]; then
            cp "$lib" "usr/lib/"
            # Recursively copy dependencies
            ldd "$lib" 2>/dev/null | grep "=> /" | awk '{print $3}' | while read dep; do
                copy_lib "$dep"
            done
        fi
    fi
}

# Copy critical libraries that might not be on all systems
for lib in libsodium libsecp256k1 libgmp; do
    libpath=$(ldconfig -p | grep "$lib" | head -1 | awk '{print $NF}')
    if [ -n "$libpath" ] && [ -f "$libpath" ]; then
        copy_lib "$libpath"
    fi
done

# Copy network configuration files
echo "Copying network configurations..."
if [ -d "$PROJECT_ROOT/configs/mainnet" ]; then
    cp -r "$PROJECT_ROOT/configs/mainnet" "$BUILD_DIR/AppDir/usr/share/lumen/config/"
fi
if [ -d "$PROJECT_ROOT/configs/preview" ]; then
    cp -r "$PROJECT_ROOT/configs/preview" "$BUILD_DIR/AppDir/usr/share/lumen/config/"
fi

# Copy AppRun and desktop file
cp "$SCRIPT_DIR/AppRun" "$BUILD_DIR/AppDir/"
chmod +x "$BUILD_DIR/AppDir/AppRun"
cp "$SCRIPT_DIR/lumen.desktop" "$BUILD_DIR/AppDir/"

# Create/copy icon
if [ -f "$PROJECT_ROOT/assets/icon.png" ]; then
    cp "$PROJECT_ROOT/assets/icon.png" "$BUILD_DIR/AppDir/lumen.png"
    cp "$PROJECT_ROOT/assets/icon.png" "$BUILD_DIR/AppDir/usr/share/icons/hicolor/256x256/apps/lumen.png"
else
    # Create placeholder icon
    echo "Creating placeholder icon..."
    convert -size 256x256 xc:'#0033AD' \
        -fill white -gravity center -pointsize 72 -annotate 0 'L' \
        "$BUILD_DIR/AppDir/lumen.png" 2>/dev/null || \
    echo "WARNING: Could not create icon. Install ImageMagick or provide assets/icon.png"
fi

# Create .DirIcon symlink
cd "$BUILD_DIR/AppDir"
ln -sf lumen.png .DirIcon

# Download appimagetool if not present
APPIMAGETOOL="$PROJECT_ROOT/.cache/appimagetool"
if [ ! -f "$APPIMAGETOOL" ]; then
    echo "Downloading appimagetool..."
    mkdir -p "$(dirname "$APPIMAGETOOL")"
    curl -L -o "$APPIMAGETOOL" \
        "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "$APPIMAGETOOL"
fi

# Build AppImage
echo "Creating AppImage..."
cd "$BUILD_DIR"
ARCH=x86_64 "$APPIMAGETOOL" AppDir "Lumen-${VERSION}-x86_64.AppImage"

# Create zsync file for delta updates (if zsyncmake is available)
if command -v zsyncmake &> /dev/null; then
    echo "Creating zsync file for delta updates..."
    zsyncmake -u "https://github.com/user/lumen/releases/download/v${VERSION}/Lumen-${VERSION}-x86_64.AppImage" \
        "Lumen-${VERSION}-x86_64.AppImage"
fi

# Move to releases directory
mkdir -p "$PROJECT_ROOT/releases"
mv "Lumen-${VERSION}-x86_64.AppImage" "$PROJECT_ROOT/releases/"
[ -f "Lumen-${VERSION}-x86_64.AppImage.zsync" ] && \
    mv "Lumen-${VERSION}-x86_64.AppImage.zsync" "$PROJECT_ROOT/releases/"

# Generate SHA256
cd "$PROJECT_ROOT/releases"
sha256sum "Lumen-${VERSION}-x86_64.AppImage" > "Lumen-${VERSION}-x86_64.AppImage.sha256"

echo ""
echo "=========================================="
echo "Build complete!"
echo "=========================================="
echo ""
echo "Output files:"
echo "  $PROJECT_ROOT/releases/Lumen-${VERSION}-x86_64.AppImage"
echo "  $PROJECT_ROOT/releases/Lumen-${VERSION}-x86_64.AppImage.sha256"
[ -f "$PROJECT_ROOT/releases/Lumen-${VERSION}-x86_64.AppImage.zsync" ] && \
    echo "  $PROJECT_ROOT/releases/Lumen-${VERSION}-x86_64.AppImage.zsync"
echo ""
echo "To test:"
echo "  chmod +x releases/Lumen-${VERSION}-x86_64.AppImage"
echo "  ./releases/Lumen-${VERSION}-x86_64.AppImage --help"
