# Releasing

Releases are built and published by `.github/workflows/release.yml`.

1. Bump `version` in `Cargo.toml` and run `cargo build` so `Cargo.lock` follows.
2. Add a `## vX.Y.Z` section to `CHANGELOG.md`. It becomes the release notes.
3. If the tool pins changed, re-run the live tests and update `docs/CAPABILITY_AUDIT.md`.
4. Merge to `main`, then tag that commit and push the tag:

   ```sh
   git tag vX.Y.Z && git push origin vX.Y.Z
   ```

The workflow refuses a tag whose version differs from `Cargo.toml`, has no changelog section, or is not on `main`. It runs the formatting, lint, and test checks on Rust 1.94.0, builds Linux (x86_64, aarch64), Windows x64, and macOS (Apple silicon, Intel), and version-checks every native build. For each target it publishes an archive, a standalone executable, and an installer: an MSI (WiX 5, `packaging/windows/yt-dlp-tui.wxs`), a `.deb`, or a `.pkg`. A `SHA256SUMS.txt` covers every asset. Every installer is installed, run, and removed in CI before anything is published; the aarch64 `.deb` is cross-built, so it is only built, not installed.

Run the workflow by hand from the Actions tab to build the same archives without publishing.
