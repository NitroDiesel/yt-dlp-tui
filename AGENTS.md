# Agent guide

Start here for coding-agent work in this repository.

## Load the authoritative context

- `docs/PROJECT_CONTEXT.md`: read first in every session, before searching the code. It covers the module map, how a download flows, recipes for common changes, and the gotchas behind past decisions.
- `README.md`: capabilities, keys, supported platforms, and the checks to run.
- `docs/ARCHITECTURE.md`: before changing the event loop, process execution, persistence, tool resolution, the installer, mouse handling, or open/reveal.
- `docs/CAPABILITY_AUDIT.md`: before changing yt-dlp option coverage or the pinned versions.
- `docs/RELEASING.md`: before changing versions, tags, the changelog, packaging, or the release workflow.

Update the relevant document when a change makes it inaccurate. When you move a responsibility or learn a gotcha the code can't show, record it in `docs/PROJECT_CONTEXT.md`.

## Product direction

yt-dlp TUI is the terminal counterpart of [yt-dlp Desktop](https://github.com/NitroDiesel/yt-dlp-desktop): a conventional download manager (filters, downloads table, details, status bar), not a command builder. Keep the primary flow obvious: paste or type a link, choose video/audio and quality, then download or queue. Expert controls stay under Advanced. Every state is shown in words; color only reinforces it, and `NO_COLOR` is honored. Every action works from both the keyboard and the mouse. A fresh install needs nothing preinstalled: the app sets up its own pinned tools.

## Engineering boundaries

- Build child-process arguments as typed argv elements ending in `-- <link>`. Resolve tools only from a custom path or the managed tools folder. Keep shell strings, `PATH` lookups, and free-text argument boxes out of the app.
- The interface never waits on a process: slow work is spawned and reports back as a `Message`. Results that may describe an older job state must not overwrite a newer `ServiceEvent`.
- yt-dlp output carries the sentinel protocol on both stdout and stderr; keep `--progress` with `--print`.
- Clicks replay the key that does the same action (`Click::Key`), so mouse and keyboard behave identically.
- Tool pins live in `packaging/components.json`, with sizes and SHA-256 digests independently verified against each publisher's checksum file. Downloaded tools stay out of the repository.
- SQLite migrations are append-only once released. New stored fields need serde defaults.
- Open and reveal take a job id and re-validate its file inside the destination.

## Completion criteria

Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets`, and add a regression test for changed behavior. Interface changes need a PTY scenario in `tests/tui_pty.rs`. Changes to arguments, the runner, or the pins also need the live tests (see the README). Packaging or workflow changes need a green manual run of the release workflow before tagging.
