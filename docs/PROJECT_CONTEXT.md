# Project context

The map of this codebase: where each responsibility lives, how a download flows through it, recipes for the common changes, and the gotchas behind decisions that the code alone doesn't explain. Read it at the start of a session instead of scanning the tree. Symbols are named so `grep -n` finds them; update this file when a change moves a responsibility or retires a gotcha.

## What this is

yt-dlp TUI is the terminal counterpart of the owner's [yt-dlp Desktop](https://github.com/NitroDiesel/yt-dlp-desktop) (Tauri). Its Rust core (argv builder, analysis, runner, queue, SQLite) was ported from the desktop's `src-tauri`; Tauri commands became `AppService` methods that the TUI calls directly. The repository is private at the owner's request. Released versions, assets, and notes are in `CHANGELOG.md`; the pinned tools are in `packaging/components.json`.

The owner's standing requirements: nothing needs to be preinstalled (the app sets up its own pinned yt-dlp, Deno, and FFmpeg), the mouse works everywhere the keyboard does, and every release ships archives, standalone executables, and installers (MSI, .deb, .pkg) for all five targets.

## Module map

| Path | Owns | Start at |
|---|---|---|
| `src/main.rs` | CLI parsing, data and tools folders, the single-instance lock, the tokio runtime | `main`, `run_interface`, `lock_data_dir` |
| `src/domain/models.rs` | Serialized contracts: `DownloadOptions`, `DownloadRequest`, `DownloadJob`, `JobStatus`, `AppSettings`, tool and GPU info | the struct you're changing |
| `src/application/service.rs` | `AppService`: queue scheduler, job lifecycle, analysis, settings, tool checks, installs, shutdown; `validate_settings` | `enqueue`, `schedule`, `run_job` |
| `src/integration/yt_dlp/args.rs` | Typed argv for downloads, request validation, playlist-item grammar, clip naming | `build_download_args`, `validate_typed_options` |
| `src/integration/yt_dlp/probe.rs` | Link analysis, error explanations, `redact` | `probe`, `classify_probe_error` |
| `src/integration/yt_dlp/{parser,runner}.rs` | The `__YTDLP_TUI__` sentinel protocol; the child process, read on both streams | `parse_protocol_line`, `run_download`, `read_lines` |
| `src/integration/installer.rs` | Pinned tool download, size and SHA-256 checks, extraction, `installed.json` | `install`, `outdated_components` |
| `src/integration/dependencies.rs` | Tool resolution (custom path, then managed folder; never `PATH`) and version checks | `DependencyManager` |
| `src/integration/ffmpeg.rs` | NVENC/AMF runtime probes and GPU re-encoding | `inspect_hardware_acceleration`, `run_hardware_conversion` |
| `src/persistence/database.rs` | SQLite queries; migrations live in `migrations/` | `Database` |
| `src/tui/mod.rs` | Event loop, terminal setup and teardown, mouse capture, the input reader thread | `run` |
| `src/tui/app.rs` | `App` state, key/paste/mouse routing, background work (`Message`), dialogs, engine setup | `handle_key`, `handle_message`, `click` |
| `src/tui/view.rs` | All drawing, plus recording clickable areas into `Hits` | `render` |
| `src/tui/form.rs` | The shared form engine: rows rebuilt each frame, focus by id, pickers, keys, clicks, wheel, hover | `FormModel`, `handle_key`, `click`, `render` |
| `src/tui/new_download.rs` | The New download form and request building | `rows_themed`, `choose`, `build_request` |
| `src/tui/settings.rs` | The Settings form and draft handling | `SettingsForm::rows`, `build` |
| `src/tui/mouse.rs` | `Click`, `Layer`, `Hits` | `Click::for_hint` |
| `src/tui/{format,text_input,theme}.rs` | Display helpers, the line editor, semantic colors | — |
| `src/bin/fake-ytdlp.rs` | The test engine; the link picks the scenario | its header comment |
| `tests/` | `tui_pty.rs` (real binary in a PTY), `runner_integration.rs`, `cli.rs`, `live_download.rs` | — |
| `.github/workflows/release.yml` | Verify, five builds, packaging, install tests, publish | see `docs/RELEASING.md` |

## How a download flows

1. A paste or `n` opens `NewDownload` (`App::open_new_download`). The link is read only when the engine is `Engine::Ready`; earlier links wait in `pending` and are read on the next `Message::Dependencies`.
2. Analysis is spawned (`AppService::probe_media`) and returns as `Message::Probed`; results for an older `revision` are dropped.
3. `NewDownload::build_request` turns form state into a `DownloadRequest`; `App::enqueue` creates the folder and calls `AppService::enqueue`, which validates through `build_download_args` before storing.
4. `schedule` claims queued jobs up to the concurrency; `run_job` builds argv, runs `run_download`, and relays `RunnerEvent`s through `relay_events` (UI at most every 250 ms, SQLite every 2 s, status changes immediately).
5. Each update reaches the UI as `ServiceEvent::JobChanged`; `App::update_job` merges it into `queue` and `history`, which `merge_downloads` turns into the single list.

## Recipes

**Add a yt-dlp option.** Add the field to `DownloadOptions` with `#[serde(default)]` and to its `Default`. Validate it in `validate_typed_options`, emit it in `build_download_args`, and add an `args.rs` test. In `new_download.rs`, add a `Field`, a row (main form or `advanced_rows`), its `choose`/`toggle`/`input` arm, and the `build_request` mapping. Add a row to `docs/CAPABILITY_AUDIT.md`. Confirm the flag exists in the pinned yt-dlp's `options.py`, then run the live tests.

**Add a setting.** Add the field to `AppSettings` with a serde default, validate it in `validate_settings`, and add a `settings::Field`, row, handler arm, and `SettingsForm::build` mapping. Settings apply through `AppService::save_settings`; the event loop reads `app.settings` each iteration (as it does for `mouse`).

**Make something clickable.** In `view.rs`, push a `Hit` with the right `Layer`: `Base` for screens, `Bar` for key hints, `Modal`, or `Confirm`. Prefer `Click::Key(...)` replaying the key that already does the job, so mouse and keyboard cannot diverge. Forms need nothing extra: `form::click` uses the geometry `form::render` records.

**Bump the pinned tools.** For each artifact in `packaging/components.json`, update `url`, `sha256` (checked against the publisher's own checksum file, never only GitHub's API digest), and `size` (from an HTTP HEAD). Then run `cargo test` (the manifest tests), both live tests, and `yt-dlp-tui install-tools --reinstall`. Re-audit `options.py` against the previous pin, update `docs/CAPABILITY_AUDIT.md` and `CHANGELOG.md`, and fix any test that names the old version.

**Release.** Follow `docs/RELEASING.md`. Run a manual (build-only) workflow first when packaging or the workflow changed.

## Tests

- Unit tests sit beside the code. The App-level tests in `src/tui/app.rs` build a real `AppService` on a temp folder.
- `tests/tui_pty.rs` drives the real binary through `Session`: `start`/`launch` (with `PATH` and arguments), `wait_for`, `wait_until_gone`, `click` (SGR mouse), `snapshot`. The fake engine's scenario comes from the link: `slow`, `cancel`, `failure` (analysis fails), `refused` (the download fails after a warning), `playlist`, anything else succeeds. `Session::clipboard` decodes the last OSC 52 copy. Each scenario gets its own `HOME`, data, and tools folders.
- `tests/gpu_conversion.rs` is `#[ignore]` and needs an NVENC or AMF GPU: it converts an AV1 clip with GPU decoding requested.
- Live tests are `#[ignore]` and need the network; the README lists the commands. Use `https://www.youtube.com/watch?v=jNQXAC9IVRw`: yt-dlp's own test video `BaW_jenozKc` is unavailable.

## Gotchas

- **`--print` implies quiet mode.** Without `--progress`, yt-dlp emits no download records, and post-processor records go to stderr, so `read_lines` parses both streams. yt-dlp Desktop has both bugs; the fixes are not backported there yet.
- **`after_move` fires for files that already exist**, so a repeat download completes. A clip with the same name as the full video would make yt-dlp skip the full one, which is why `output_template` adds the clip range.
- **Two channels reach `App`:** `Message` (background results) and `ServiceEvent` (job updates). A `Message` can describe an older job state, so enqueue and retry results go through `add_job`, which never overwrites.
- **Escape merges in PTY tests.** Terminals send Alt+key as Escape followed by the key, so after sending Escape, wait for a screen state before the next key. After submitting New download, the job's `ServiceEvent` can update the list before `Message::Enqueued` closes the form, so wait for the form to be gone before sending keys to the list. Also wait for the exact text before `position_of`, since a frame can arrive half-drawn, and search for phrases that don't wrap.
- **The notice bar replaces the key hints** for 4–8 s after a toast, so hint clicks and waits for hint text pause until it expires.
- **GPU decode support depends on the codec and the card.** The startup check proves decoding only for H.264; an RTX 20-series card has no AV1 decoder, and FFmpeg with `-hwaccel_output_format cuda` then fails on every frame and never finishes. `decodes_on_gpu` checks the real input on one frame (`-xerror`) before each conversion and falls back to CPU decoding.
- **sqlx 0.9 accepts only `'static` SQL strings**, so queries are literals.
- **ureq readers are unlimited**; the installer enforces each artifact's pinned `size` itself.
- **`PATH` is never searched for tools.** A system yt-dlp or FFmpeg is ignored unless chosen in Settings; a PTY test guards this.
- **CI pins Rust 1.94.0** (the MSRV). Builds use `ubuntu-22.04`, which sets glibc 2.34 as the minimum. The aarch64 cross build needs `libc6-dev-arm64-cross` alongside the compiler, and WiX is pinned to 5.0.2 for reproducible MSIs. Each target's installer is installed, run, and removed in CI.
- **The macOS FFmpeg host rejects Python's default User-Agent**; check URLs with `curl`, not `urllib`.
- **On the owner's machine `/tmp` is tmpfs** and is wiped on reboot. Keep scratch tool installs in `/var/tmp`.

## Where state lives

The platform data folder (`yt-dlp-tui paths` prints it; `YT_DLP_TUI_DATA_DIR` overrides it) holds `workspace.sqlite3` and `instance.lock`. The tools folder (`<data>/tools`, overridden by `YT_DLP_TUI_TOOLS_DIR`) holds the executables and `installed.json`. Downloads go wherever each request says. `NO_COLOR` turns colors off.
