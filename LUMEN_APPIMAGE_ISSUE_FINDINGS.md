# Lumen AppImage Issue - Diagnostic Findings

**Date:** December 15, 2025
**Issue:** Lumen AppImage won't execute - downloads as "Not Found" text instead of binary
**Status:** ✅ RESOLVED - December 17, 2025

## Problem Summary

The Lumen AppImage at `/media/anekdelpugu/Idanileko/cardano/Lumen-x86_64.AppImage` contains only the text "Not Found" (9 bytes) instead of being a proper executable binary.

## Root Cause Analysis

### 1. Download Issues
- **Command used:** `curl -LO https://github.com/Oclivion/Lumen/releases/latest/download/Lumen-x86_64.AppImage`
- **Result:** HTTP 404 - "Not Found" (9 bytes)
- **Expected:** Large binary file (typically 50MB+ for a Cardano node AppImage)

### 2. Repository Investigation
- **GitHub Repo:** https://github.com/Oclivion/Lumen
- **Contributors:** Claude Sonnet 4 (top contributor), anekdelpugu
- **Latest Release:** v0.1.5 (December 11, 2025)
- **Release Count:** 6 releases (v0.1.0 through v0.1.5)
- **Assets Status:** GitHub releases page shows loading errors for asset sections

### 3. Technical Details
```bash
# File analysis of corrupted download:
$ ls -la Lumen-x86_64.AppImage
-rwxrwxr-x 1 anekdelpugu anekdelpugu 9 Dec 15 07:37 Lumen-x86_64.AppImage

$ file Lumen-x86_64.AppImage
Lumen-x86_64.AppImage: ASCII text, with no line terminators

$ cat Lumen-x86_64.AppImage
Not Found

$ hexdump -C Lumen-x86_64.AppImage
00000000  4e 6f 74 20 46 6f 75 6e  64                       |Not Found|
```

## Release Infrastructure Problems

### Missing Release Assets
- GitHub releases exist but contain no actual binary files
- Download URLs return 404 errors
- Release asset sections show loading errors on GitHub web interface

### Documentation vs Reality Gap
- README.md instructs users to download from releases
- Actual releases don't contain the promised AppImage files
- Build instructions exist but releases aren't built/uploaded

## What Needs to be Fixed

### 1. GitHub Actions / Release Pipeline
- **Missing:** Automated build pipeline to create AppImage on release
- **Needed:** CI/CD workflow that builds and uploads AppImage to releases
- **File:** Likely need `.github/workflows/release.yml` or similar

### 2. AppImage Build Process
- **Build script exists:** `./packaging/linux/build-appimage.sh`
- **Status:** Unknown if script works correctly
- **Dependencies:** Requires Rust toolchain, AppImage tools

### 3. Release Asset Upload
- **Current:** Releases created but assets not uploaded
- **Needed:** Automated asset upload to GitHub releases
- **Authentication:** May need GitHub token/permissions setup

## Immediate Action Items (For Next Session)

### 1. Verify Build Process
```bash
cd /path/to/Lumen
cargo build --release
./packaging/linux/build-appimage.sh
```

### 2. Fix Release Pipeline
- Check `.github/workflows/` directory
- Add/fix release workflow to build and upload AppImage
- Test release process

### 3. Upload Missing Assets
- Manually upload AppImage to existing releases, or
- Re-trigger release process with proper asset upload

### 4. Test Download Process
```bash
curl -LO https://github.com/Oclivion/Lumen/releases/latest/download/Lumen-x86_64.AppImage
chmod +x Lumen-x86_64.AppImage
./Lumen-x86_64.AppImage --help
```

## Repository Context

- **Type:** Cardano node wrapper/manager
- **Language:** Rust
- **Target:** Self-contained AppImage for Linux
- **Features:** Auto-updating, Mithril integration, 20-minute sync
- **License:** MIT OR Apache-2.0

## Files to Investigate Next Session

1. `.github/workflows/` - Release automation
2. `packaging/linux/build-appimage.sh` - Build script
3. `Cargo.toml` - Version and metadata
4. Release process documentation

## Current Working Directory
```
/media/anekdelpugu/Idanileko/cardano/
```

**Note:** This is our own project that we built in a previous session. The issue is in our release infrastructure, not a third-party problem.

---

## Resolution (December 17, 2025)

### Root Cause Identified
The GitHub releases workflow was uploading AppImages with versioned filenames (`Lumen-0.1.5-x86_64.AppImage`) but the README and users expected a generic filename (`Lumen-x86_64.AppImage`) to work with the `/latest/download/` URL pattern.

### Changes Made

1. **Updated `.github/workflows/release.yml`**
   - Added step to create generic filename copies before release upload
   - Now uploads both `Lumen-VERSION-x86_64.AppImage` (versioned) and `Lumen-x86_64.AppImage` (generic)
   - This allows `/latest/download/Lumen-x86_64.AppImage` to work correctly

2. **Fixed v0.1.5 Release**
   - Manually downloaded the v0.1.5 AppImage
   - Created generic filename copies
   - Uploaded to the existing v0.1.5 release
   - Verified download works: 47 MB ELF executable

### Verification

```bash
# Download now works correctly:
$ curl -LO https://github.com/Oclivion/Lumen/releases/latest/download/Lumen-x86_64.AppImage
$ ls -lh Lumen-x86_64.AppImage
-rw-rw-r-- 1 user user 47M Dec 17 17:28 Lumen-x86_64.AppImage

$ file Lumen-x86_64.AppImage
Lumen-x86_64.AppImage: ELF 64-bit LSB executable, x86-64, version 1 (SYSV)...
```

### Future Releases
All future releases triggered by the updated workflow will automatically include both:
- `Lumen-VERSION-x86_64.AppImage` (versioned for archival)
- `Lumen-x86_64.AppImage` (generic for /latest/download/)

This ensures the download instructions in the README work correctly for all users.