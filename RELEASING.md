# Releasing FileFlow

FileFlow ships as a native installer per platform. Building an installer
requires the target platform's toolchain — we don't cross-compile.

## Prerequisites

- Rust stable (1.77+).
- Node 20+ and npm.
- Platform-specific:
  - **Windows** — WebView2 runtime is bundled by Tauri; NSIS is fetched by
    `tauri build` automatically. Requires the MSVC C++ build tools.
  - **macOS** — Xcode command-line tools. For a signed & notarized build
    you also need an Apple Developer certificate and `APPLE_ID` /
    `APPLE_PASSWORD` env vars (Tauri handles notarization when present).
  - **Linux** — `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `librsvg2-dev`,
    plus `nsis` and `wix` are *not* needed. Building `.deb`/`.AppImage`
    needs `dpkg-deb` and `appimagetool` respectively.

## Build

```bash
npm install
npm run build                  # produces dist/ (frontend)
npm run tauri build            # builds native binary + installer
```

Tauri emits into `src-tauri/target/release/bundle/…`:

| Platform | Artifact(s)                                                          |
|----------|----------------------------------------------------------------------|
| Windows  | `nsis/FileFlow_0.1.0_x64-setup.exe`, `msi/FileFlow_0.1.0_x64_en-US.msi` |
| macOS    | `dmg/FileFlow_0.1.0_universal.dmg`, `macos/FileFlow.app`             |
| Linux    | `deb/fileflow_0.1.0_amd64.deb`, `appimage/fileflow_0.1.0_amd64.AppImage`, `rpm/fileflow-0.1.0.x86_64.rpm` |

Every artifact is a fully-self-contained bundle — WebView2 on Windows is
downloaded on first launch by the embedded bootstrapper.

## Checksums

Publish SHA-256 sums for every artifact next to the release entry:

```bash
# Windows PowerShell
Get-FileHash -Algorithm SHA256 dist-artifacts/*.exe > SHA256SUMS.txt
# macOS / Linux
shasum -a 256 dist-artifacts/* > SHA256SUMS.txt
```

Users can verify with:

```bash
shasum -a 256 -c SHA256SUMS.txt
```

## Version bump

Update the version in three places (they must match):

- `Cargo.toml` `[workspace.package].version`
- `package.json` `"version"`
- `src-tauri/tauri.conf.json` `"version"`

Update `CHANGELOG.md` and tag: `git tag -a v0.1.0 -m "FileFlow 0.1.0"`.

## Signing

- **Windows** — Tauri supports Authenticode signing via
  `TAURI_KEY_PASSWORD` + `TAURI_PRIVATE_KEY`. A self-signed cert works for
  development; ship a real EV cert for production.
- **macOS** — provide `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
  `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`.
- **Linux `.deb`/`.AppImage`** — sign with `gpg --detach-sign` after build.
