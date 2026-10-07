# Changelog

Each release's section here becomes its GitHub release notes.

## v0.1.0

The first release of yt-dlp TUI, the terminal counterpart of [yt-dlp Desktop](https://github.com/NitroDiesel/yt-dlp-desktop).

### Highlights

- A keyboard-driven download manager: filters, a downloads table with live progress, a details panel, and a status bar. Paste a link anywhere to start.
- Every desktop download option as a form control: quality, exact streams, container, codec, audio format and bitrate, audio track, clips, subtitles, chapters, SponsorBlock, side files, filename presets, playlist filters, connections, and headers.
- Runtime NVIDIA NVENC and AMD AMF detection with optional GPU re-encoding.
- A persistent queue and history in SQLite, with pause, reorder, retry, cancel, remove, open, and show in folder.

### Download engine

Press `I` in the app, or run `yt-dlp-tui install-tools`, to install the pinned builds. Each is checked against a SHA-256 digest built into the app before it is used.

| Tool | Version |
|---|---|
| yt-dlp | 2026.08.19 |
| Deno | 2.9.7 |
| FFmpeg and FFprobe | 8.1.2 |

Up to date with yt-dlp 2026.08.19: SponsorBlock `hook`, highlight, and community chapter segments; Tor Browser impersonation; Whale browser cookies.

### Fixed compared with yt-dlp Desktop 0.1.13

- Live download progress: `--print` puts yt-dlp in quiet mode, so `--progress` is now passed.
- Post-processing stages (merging, extracting audio, metadata) are read from stderr, where yt-dlp writes them.
- Clips are named after their time range, so a clip no longer stands in for a later full download.
- yt-dlp's standard output is redacted before it is kept as diagnostics.

### Install

Download the archive for your platform, check it against `SHA256SUMS.txt`, extract it, and run:

```sh
./yt-dlp-tui install-tools
./yt-dlp-tui
```

Builds are provided for Linux (x86_64 and aarch64, glibc 2.35 or newer), Windows x64, and macOS (Apple silicon and Intel). The binaries are not code-signed. On macOS, if Gatekeeper blocks a browser-downloaded binary, run `xattr -d com.apple.quarantine yt-dlp-tui`.

### Known limitations

- Linux is the tested platform. The Windows and macOS builds compile and pass a version check in CI but have not been tried interactively.
- GPU conversion, browser cookies, and proxies have not been tested end to end.
