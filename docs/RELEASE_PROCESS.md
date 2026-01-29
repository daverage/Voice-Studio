# Release Process (Local gh)

This repo uses a local, `gh`-driven release flow. Releases are tagged as `vX.Y.Z` and built for macOS, Windows, and Linux.

## Preconditions

- Clean working tree (`git status` is empty)
- GitHub CLI installed and authenticated (`gh auth status`)
- Docker installed (for Linux cross builds via `cross`)
- `cross` installed (`cargo install cross`)
- `xwin` installed (`cargo install xwin`)
- LLVM `lld-link` available in PATH (for Windows cross build)
- Version set to the intended release (e.g., `0.1.0`)

## Version Sources

- `Cargo.toml` version
- `Cargo.lock` entry for `vxcleaner`
- `src/lib.rs` VERSION constant (UI footer text)

All three must match before releasing.

## Release Script

Run the script and follow the prompt for the commit message:

```bash
tools/release.sh
```

The script:
- Builds and bundles macOS/Windows/Linux artifacts
- Uses `cross` for Linux builds (Docker)
- Uses `xwin` + `lld-link` for Windows builds on macOS
- Packages `help.html` at the root of each zip
- Produces per-OS/per-format zips in `dist/vX.Y.Z/`
- Creates a git tag `vX.Y.Z`
- Creates a GitHub release and uploads assets

## Asset Naming

- `vxcleaner-macos-vst3.zip`
- `vxcleaner-macos-clap.zip`
- `vxcleaner-windows-vst3.zip`
- `vxcleaner-windows-clap.zip`
- `vxcleaner-linux-vst3.zip`
- `vxcleaner-linux-clap.zip`
