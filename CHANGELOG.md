# Changelog

Each release's section here becomes its GitHub release notes.

## v0.2.2

### Copy the technical details

- A download's details now have a **Copy details** button (or press `Y`) when it has an error or technical details. It copies the app version, the link, the status and error, the format, the file or folder, the times, the chosen options, and every diagnostic line, ready to paste into a bug report.

### Fixed

- GPU conversion no longer hangs when the GPU can't decode the video's format, such as AV1 on an NVIDIA RTX 20-series card or older. FFmpeg failed on every frame and never finished. Before converting, the app now checks one frame of the actual video; if the GPU can't decode it, the CPU decodes and the GPU still encodes. The details say when this happens.

## v0.2.1

### Installers and standalone executables

Every release now includes, for each platform:

- **Windows x64:** an `.msi` installer (installs to Program Files, adds `yt-dlp-tui` to `PATH`, adds a Start Menu entry, and uninstalls from Settings → Apps), a standalone `.exe`, and the `.zip`.
- **Linux x86_64 and aarch64:** a `.deb` package (`sudo apt install ./yt-dlp-tui-….deb`), a standalone executable, and the `.tar.gz`.
- **macOS Apple silicon and Intel:** a `.pkg` installer (installs to `/usr/local/bin`), a standalone executable, and the `.tar.gz`.

Standalone executables downloaded through a browser on Linux or macOS need `chmod +x` first. The installers and executables are not code-signed, so Windows SmartScreen or macOS Gatekeeper may ask before running them. Your downloads and the app's tools live in your user data folder, so uninstalling keeps them.

## v0.2.0

### Mouse support

- Click the sidebar, downloads, buttons, form fields, picker options, confirmation buttons, and the key hints along the bottom.
- Double-click a download to show its details. The details panel now has buttons for its actions: cancel, retry, move up or down, open the file, show it in its folder, copy the link, and remove.
- Scroll the downloads list, details, forms, and pickers with the wheel. Clickable items underline when the pointer is over them.
- Hold Shift while dragging to select text, or turn the mouse off under Settings → Appearance to leave it to the terminal.

### Fixed

- `yt-dlp-tui --help`, `--version`, and `paths` no longer panic when their output is piped into a command that stops reading early, such as `head`.

### Install

Download the archive for your platform, check it against `SHA256SUMS.txt`, extract it, and run `./yt-dlp-tui`. Nothing else needs to be installed; accept the setup prompt on first launch. Requirements and pinned tool versions are unchanged from v0.1.0.

## v0.1.0

The first release of yt-dlp TUI, the terminal counterpart of [yt-dlp Desktop](https://github.com/NitroDiesel/yt-dlp-desktop).

### Highlights

- A keyboard-driven download manager: filters, a downloads table with live progress, a details panel, and a status bar. Paste a link anywhere to start.
- Every desktop download option as a form control: quality, exact streams, container, codec, audio format and bitrate, audio track, clips, subtitles, chapters, SponsorBlock, side files, filename presets, playlist filters, connections, and headers.
- Runtime NVIDIA NVENC and AMD AMF detection with optional GPU re-encoding.
- A persistent queue and history in SQLite, with pause, reorder, retry, cancel, remove, open, and show in folder.

### Download engine

Nothing needs to be installed first: no yt-dlp, Python, FFmpeg, or Deno. On first launch the app offers to set up its own copies of the pinned builds below (about 200 MB, depending on the platform), and reads any link it was started with once setup finishes. Each file is checked against a size and SHA-256 built into the app before it is used. Tools already on your system are never used unless you choose them in Settings. `yt-dlp-tui install-tools` sets up ahead of time.

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

Download the archive for your platform, check it against `SHA256SUMS.txt`, extract it, and run `./yt-dlp-tui`. Accept the setup prompt on first launch.

Builds are provided for Linux (x86_64 and aarch64, glibc 2.34 or newer), Windows x64, and macOS (Apple silicon and Intel). The binaries are not code-signed. On macOS, if Gatekeeper blocks a browser-downloaded binary, run `xattr -d com.apple.quarantine yt-dlp-tui`.

### Known limitations

- Linux x86_64 is the tested platform. The other builds are produced in CI, where the Windows and macOS builds also pass a version check, but none of them has been tried interactively.
- GPU conversion, browser cookies, and proxies have not been tested end to end.
