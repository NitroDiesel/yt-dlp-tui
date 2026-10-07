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
- Installs pinned builds of yt-dlp 2026.08.19, Deno 2.9.7, and FFmpeg 8.1.2, each verified against a SHA-256 built into the app. Programs on `PATH` are never run automatically.

## Install

Requires Rust 1.94 or newer.

```sh
cargo install --path .
yt-dlp-tui install-tools   # or press I inside the app
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
| `K` / `J` | Move a waiting download up or down |
| `p` | Pause or resume the queue |
| `C` | Clear completed downloads from the list |
| `I` | Install or update the pinned tools |
| `s` / `?` / `q` | Settings / all keys / quit |

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

`cargo test` includes end-to-end tests that drive the real binary in a pseudo-terminal against a fake engine (`src/bin/fake-ytdlp.rs`) and read the screen through a terminal emulator. Two more tests download a short public video with the real pinned tools; they need the network and run on request:

```sh
cargo run -- install-tools
export YT_DLP_TUI_LIVE_TOOLS="$(cargo run -q -- paths | sed -n 's/^Tools: //p')"
cargo test --test live_download --test tui_pty -- --ignored --nocapture
```

- [Architecture and security decisions](docs/ARCHITECTURE.md)
- [Audited yt-dlp capability mapping](docs/CAPABILITY_AUDIT.md)
- [Agent guide](AGENTS.md)

## License

Licensed under the GNU General Public License, version 3 or later, like yt-dlp Desktop. See [LICENSE](LICENSE). The installed tools keep their own licenses: the official yt-dlp executables and the FFmpeg builds are GPL-3.0-or-later combined works, and Deno is MIT. The app downloads them from their publishers; it does not redistribute them.
