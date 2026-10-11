# yt-dlp TUI

A keyboard-driven terminal interface for [yt-dlp](https://github.com/yt-dlp/yt-dlp), and the terminal counterpart of [yt-dlp Desktop](https://github.com/NitroDiesel/yt-dlp-desktop). It is written in Rust with [ratatui](https://ratatui.rs) and SQLite, and shares the desktop app's download engine design: typed arguments instead of shell strings, a persistent queue, and pinned, checksum-verified tools.

> Download only media you are authorized to access. This project does not bypass DRM and is not affiliated with yt-dlp or supported media services.

## What it does

- Analyzes a video, playlist, or channel link before downloading, and starts a new download when you paste a link anywhere.
- Offers video or audio, quality, container, codec preference, audio format and bitrate, exact stream picking, and audio track language.
- Exposes yt-dlp features as form controls: subtitles and automatic captions, subtitle conversion, chapters, SponsorBlock (including the `hook`, highlight, and community chapter segments in yt-dlp 2026.08.19), thumbnails and side files, filename presets, parallel connections, playlist ranges, date windows, size and length filters, item limits, Referer and User-Agent headers.
- Downloads only a selected timeframe, with an accurate or a fast keyframe cut. Clips are named after their range so they never shadow the full video.
- Detects NVIDIA NVENC and AMD AMF at runtime and can re-encode a finished video on the GPU.
- Shows every download in one list, filtered by Downloading, Completed, and Stopped, with live progress, speed, ETA, post-processing stages, errors, and diagnostics.
- Supports pause, reorder, retry, cancel, remove, open file, and show in folder. Removing a download never deletes its file.
- Keeps the queue, history, and settings in SQLite. Quitting stops running downloads as *interrupted*; Retry resumes from the partial file.
- Needs nothing else installed: no yt-dlp, Python, FFmpeg, or Deno. On first launch it offers to set up its own copies of yt-dlp 2026.08.19, Deno 2.9.7, and FFmpeg 8.1.2 (about 200 MB, depending on the platform), each checked against a size and SHA-256 built into the app. A yt-dlp or FFmpeg already on your system is never used unless you choose it in Settings.

## Install

Download from the [Releases page](https://github.com/NitroDiesel/yt-dlp-tui/releases) and check the file against `SHA256SUMS.txt`:

| Platform | Installer | Standalone |
|---|---|---|
| Windows x64 | `.msi`: installs to Program Files, adds `yt-dlp-tui` to `PATH`, and adds a Start Menu entry | `.exe`, or the `.zip` |
| Linux x86_64 / aarch64 | `.deb`: `sudo apt install ./yt-dlp-tui-….deb` | the executable (`chmod +x`), or the `.tar.gz` |
| macOS Apple silicon / Intel | `.pkg`: installs to `/usr/local/bin` | the executable (`chmod +x`), or the `.tar.gz` |

Then run `yt-dlp-tui` (or `./yt-dlp-tui` for a standalone file). You don't need to install yt-dlp or anything else first: on first launch the app asks to set up its own download engine, and a link you started it with is read as soon as setup finishes. To set up ahead of time, for example on a server, run `./yt-dlp-tui install-tools`.

To build from source instead (Rust 1.94 or newer):

```sh
cargo install --path .
yt-dlp-tui
```

Pinned tool builds exist for Linux (x86_64, aarch64, glibc), Windows x64, and macOS (Apple silicon and Intel). On other systems, choose your own executables under **Settings → Download engine**. GPU conversion appears only when FFmpeg completes a real test encode on an installed NVIDIA or AMD driver; it is unavailable on macOS.

## Use

Copy a link and paste it into the terminal, or press `n`. The form reads the link, then offers what that media supports. `Ctrl+D` downloads now and `Ctrl+Q` adds to the queue.

| Key | Action |
|---|---|
| `n` or paste | New download |
| `↑` `↓` / `j` `k` | Select a download |
| `Enter` / `Esc` | Show or hide details |
| `1`–`4`, `Tab` | All, Downloading, Completed, Stopped |
| `c` / `r` / `d` | Cancel / retry / remove from the list |
| `o` / `f` | Open the file / show it in its folder |
| `y` | Copy the source link (OSC 52, works over SSH) |
| `Y` | Copy the technical details: the request, the error, and every diagnostic line, ready for a bug report |
| `K` / `J` | Move a waiting download up or down |
| `p` | Pause or resume the queue |
| `C` | Clear completed downloads from the list |
| `I` | Set up, update, or cancel installing the app's own tools |
| `s` / `?` / `q` | Settings / all keys / quit |

The mouse works too: click sidebar entries, downloads, buttons, form fields, picker options, and the key hints along the bottom; double-click a download for its details; scroll with the wheel. Hold Shift while dragging to select text, or turn the mouse off under **Settings → Appearance** to leave it to the terminal.

In forms, `↑` `↓` and `Tab` move between fields, `←` `→` change a choice, `Enter` opens a list or presses a button, and `→` at the end of a folder accepts the suggested completion. Settings save with `Ctrl+S`.

```text
yt-dlp-tui [LINK]                       Open the app, optionally reading LINK
yt-dlp-tui install-tools [--reinstall]  Download the pinned tools
yt-dlp-tui paths                        Show the data and tools folders
yt-dlp-tui --version                    Show the version and pinned tool versions
```

The database and tools live in the platform data folder (`~/.local/share/yt-dlp-tui` on Linux). Set `YT_DLP_TUI_DATA_DIR` or `YT_DLP_TUI_TOOLS_DIR` to move them, and `NO_COLOR` to turn colors off. Only one copy of the app runs per data folder at a time.

## Develop

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

`cargo test` includes end-to-end tests that drive the real binary in a pseudo-terminal against a fake engine (`src/bin/fake-ytdlp.rs`) and read the screen through a terminal emulator. They also check that a yt-dlp on `PATH` is ignored. Three more tests use the network and run on request: one starts from an empty tools folder, lets the app set up its own engine, and downloads a short public video; the other two download with tools you installed:

```sh
cargo test --test tui_pty a_fresh_machine -- --ignored --nocapture
cargo run -- install-tools
export YT_DLP_TUI_LIVE_TOOLS="$(cargo run -q -- paths | sed -n 's/^Tools: //p')"
cargo test --test live_download --test tui_pty -- --ignored --skip a_fresh_machine --nocapture
```

On a machine with an NVIDIA or AMD GPU, `cargo test --test gpu_conversion -- --ignored --nocapture` (with the same variable) checks GPU conversion of an AV1 video.

- [Architecture and security decisions](docs/ARCHITECTURE.md)
- [Audited yt-dlp capability mapping](docs/CAPABILITY_AUDIT.md)
- [Releasing](docs/RELEASING.md) and the [changelog](CHANGELOG.md)
- [Agent guide](AGENTS.md)

## License

Licensed under the GNU General Public License, version 3 or later, like yt-dlp Desktop. See [LICENSE](LICENSE). The installed tools keep their own licenses: the official yt-dlp executables and the FFmpeg builds are GPL-3.0-or-later combined works, and Deno is MIT. The app downloads them from their publishers; it does not redistribute them.
