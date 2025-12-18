# Lumen

**Self-contained, auto-updating Cardano node for everyone.**

Lumen packages the Cardano node into a user-friendly application that runs out-of-the-box, checks for updates on startup, and handles synchronization without requiring manual compilation or command-line expertise.

## Features

- **One-Click Start**: Double-click to run. No compilation, no dependencies to install.
- **Auto-Updates**: Checks for updates on startup with Ed25519 signature verification.
- **Fast Sync**: Mithril integration reduces initial sync from days to ~20 minutes.
- **Self-Contained**: All dependencies bundled (libsodium, libsecp256k1, etc.).
- **Cross-Platform**: Linux AppImage (macOS DMG and Windows EXE planned).
- **Secure**: Cryptographic verification of all updates and snapshots.

## Quick Start

### Linux (AppImage)

```bash
# Download
curl -LO https://github.com/Oclivion/Lumen/releases/latest/download/Lumen-x86_64.AppImage
chmod +x Lumen-x86_64.AppImage

# Run
./Lumen-x86_64.AppImage start

# Check status
./Lumen-x86_64.AppImage status

# Stop
./Lumen-x86_64.AppImage stop
```

### First Run

On first run, Lumen will:
1. Create configuration in `~/.local/share/lumen/`
2. Check for updates
3. Download a Mithril snapshot for fast sync (~40 GB, ~20 minutes)
4. Start the Cardano node

## Commands

```bash
lumen start              # Start the node (background)
lumen start --foreground # Start in foreground
lumen stop               # Stop the node gracefully
lumen stop --force       # Force kill
lumen status             # Show node status

lumen update --check     # Check for updates
lumen update             # Download and apply update

lumen mithril list       # List available snapshots
lumen mithril download   # Download latest snapshot
lumen mithril verify     # Verify existing snapshot

lumen init               # Initialize configuration
lumen config             # Show current configuration
lumen version            # Show version info
```

## Configuration

Configuration is stored in `~/.config/lumen/config.toml`:

```toml
network = "mainnet"  # or "preview", "preprod"
data_dir = "/home/user/.local/share/lumen"

[node]
host = "0.0.0.0"
port = 3001

[update]
auto_check = true
check_interval_hours = 24

[mithril]
enabled = true

[resources]
max_memory_mb = 8192
rts_threads = 0  # 0 = auto
```

## Networks

| Network | Description | Mithril Sync |
|---------|-------------|--------------|
| mainnet | Production network | ~40 GB, ~20 min |
| preview | Development testnet | ~5 GB, ~5 min |
| preprod | Pre-production testnet | ~15 GB, ~10 min |

## Requirements

- **Disk Space**: 150+ GB for mainnet, 20 GB for testnets
- **Memory**: 8 GB recommended, 4 GB minimum
- **OS**: Linux x86_64 (glibc 2.31+)

## Security

### Update Verification

All updates are verified using:
1. **SHA-256 hash** - Ensures download integrity
2. **Ed25519 signature** - Cryptographically verifies authenticity
3. **Minimum version check** - Forces update for critical security fixes

The public key is hardcoded in the binary and cannot be modified without rebuilding.

### Mithril Verification

Mithril snapshots are verified via:
1. **Certificate chain** - Traced back to genesis
2. **Stake-weighted multisig** - Signed by Cardano stake pool operators
3. **Hash verification** - Snapshot integrity check

## Building from Source

```bash
# Clone
git clone https://github.com/Oclivion/Lumen
cd Lumen

# Build
cargo build --release

# Run
./target/release/lumen --help

# Build AppImage
./packaging/linux/build-appimage.sh
```

## Release Signing

Releases are signed with Ed25519 to prove they come from the official source. Signing is **optional** - releases will still work without it, but won't have cryptographic verification.

### Step 1: Generate a Signing Key

Run this command to create a new key:

```bash
openssl genpkey -algorithm ed25519 -out lumen-signing.pem
```

This creates a file called `lumen-signing.pem`. **Keep this file secret and backed up!**

### Step 2: Get the Key Value for GitHub

Run this command to extract the key value:

```bash
grep -v "^-" lumen-signing.pem | tr -d '\n' && echo
```

This outputs a long string like `MC4CAQAwBQYDK2VwBCIEIxxxxxxxxx...`. Copy this entire string.

### Step 3: Add the Secret to GitHub

1. Go to https://github.com/Oclivion/Lumen/settings/secrets/actions
2. Click the green **"New repository secret"** button
3. In the **Name** field, type: `LUMEN_SIGNING_KEY`
4. In the **Secret** field, paste the string you copied in Step 2
5. Click **"Add secret"**

That's it! Future releases will now be automatically signed.

### Step 4: Get the Public Key (Optional)

If you want to verify signatures, get the public key with:

```bash
openssl pkey -in lumen-signing.pem -pubout
```

This public key should be added to `orchestrator/src/config.rs` for the app to verify updates.

## Project Structure

```
lumen/
├── orchestrator/           # Rust core
│   └── src/
│       ├── main.rs         # CLI entry point
│       ├── config.rs       # Configuration management
│       ├── node_manager.rs # Node process control
│       ├── updater.rs      # Auto-update with Ed25519
│       ├── mithril.rs      # Mithril snapshot client
│       └── error.rs        # Error types
├── gui/                    # Future: Tauri GUI
├── packaging/
│   ├── linux/
│   │   ├── build-appimage.sh
│   │   ├── AppRun
│   │   └── lumen.desktop
│   ├── macos/              # Future
│   └── windows/            # Future
├── configs/
│   ├── mainnet/
│   └── preview/
└── keys/
    └── update-pubkey.pem   # Update signing public key
```

## Roadmap

- [x] Linux AppImage with auto-updates
- [x] Mithril fast sync integration
- [x] Ed25519 signature verification
- [x] Tauri GUI with system tray
- [ ] macOS DMG with Sparkle updates
- [ ] Windows installer with Squirrel
- [ ] Stake pool operator mode
- [ ] Built-in wallet (light client)

## Contributing

Contributions welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) first.

## License

MIT OR Apache-2.0

## Acknowledgments

- [IOG](https://iohk.io/) for Cardano
- [Mithril](https://mithril.network/) for fast sync
- [AppImage](https://appimage.org/) for portable Linux packaging
