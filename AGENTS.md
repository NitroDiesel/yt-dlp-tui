# Agent guide

Start here for coding-agent work in this repository.

## Load the authoritative context

- `README.md`: capabilities, keys, supported platforms, and the checks to run.
- `docs/ARCHITECTURE.md`: before changing the event loop, process execution, persistence, tool resolution, the installer, or open/reveal.
- `docs/CAPABILITY_AUDIT.md`: before changing yt-dlp option coverage or the pinned versions.
- `docs/RELEASING.md`: before changing versions, tags, the changelog, or the release workflow.

Update the relevant document when a change makes it inaccurate.

## Product direction

yt-dlp TUI is the terminal counterpart of [yt-dlp Desktop](https://github.com/NitroDiesel/yt-dlp-desktop): a conventional download manager (filters, downloads table, details, status bar), not a command builder. Keep the primary flow obvious: paste or type a link, choose video/audio and quality, then download or queue. Expert controls stay under Advanced. Every state is shown in words; color only reinforces it, and `NO_COLOR` is honored.

## Engineering boundaries

- Build child-process arguments as typed argv elements ending in `-- <link>`. Never construct shell strings, search `PATH`, or add a free-text argument box.
- The interface never waits on a process: slow work is spawned and reports back as a `Message`. Results that may describe an older job state must not overwrite a newer `ServiceEvent`.
- yt-dlp output carries the sentinel protocol on both stdout and stderr; keep `--progress` with `--print`.
- Tool pins live in `packaging/components.json` with SHA-256 digests independently verified against each publisher's checksum file. Never commit downloaded tools.
- SQLite migrations are append-only once released. New stored fields need serde defaults.
- Open and reveal take a job id and re-validate its file inside the destination.

## Completion criteria

Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets`, and add a regression test for changed behavior. Interface changes need a PTY scenario in `tests/tui_pty.rs`. Changes to arguments, the runner, or the pins also need the live tests (see the README).
