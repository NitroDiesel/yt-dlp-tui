# Architecture

## Decision record

yt-dlp TUI is a single Rust binary: a ratatui interface on top of the same application core design as yt-dlp Desktop's Rust backend. The interface owns screen state and short-lived form drafts. The core owns validation, process execution, cancellation, persistence, tool discovery, and platform actions. SQLite is the durable source of truth.

```text
ratatui views + App state  ◀── terminal reader thread (keys, bracketed paste)
          │ async calls                ▲ Message / ServiceEvent channels
          ▼                            │
AppService ─────────── queue scheduler (1–4 at a time)
  ├── SQLite             ├── typed argv builder
  ├── tool resolver      ├── child-process runner
  ├── tool installer     └── sentinel protocol parser (stdout and stderr)
  └── open / reveal               │
                                  ▼
                      yt-dlp + Deno + FFmpeg
```

## Modules

- `domain`: serialized contracts, the job state machine, settings, tool and GPU metadata.
- `application`: startup, the persistent queue scheduler, recovery, shutdown, and job events.
- `integration/yt_dlp`: typed argument construction, analysis, the progress protocol, redaction, and the child-process lifecycle.
- `integration/ffmpeg`: runtime NVENC/AMF probes and the non-destructive GPU conversion stage.
- `integration/dependencies`: resolves a custom executable first, then the managed tools folder. `PATH` is never searched.
- `integration/installer`: downloads `packaging/components.json` pins, verifies SHA-256 while streaming, extracts only the named archive entries, and swaps a component in only after all of its files are ready.
- `persistence`: migrations and SQLite queries.
- `platform`: opens a validated completed file or its folder.
- `tui`: the event loop, `App` state, a shared keyboard form model used by New download and Settings, and the views.

## Interface model

The event loop draws, then waits on whichever arrives first: terminal input (read on a dedicated thread that polls and exits when its channel closes), results of background work (`Message`), job updates from the service (`ServiceEvent`), or a half-second tick. Nothing that starts a process runs on the loop itself.

Forms rebuild their rows from state on every frame, so controls appear exactly when their options apply, and focus is tracked by a stable field id rather than a row index. Service events and background results travel on separate channels, so a result that describes an older state of a job (such as the one returned by enqueue) never overwrites a newer event.

## Process lifecycle

1. Analysis runs `yt-dlp --dump-single-json --flat-playlist` with the network settings. Results for a link the user has since changed are ignored.
2. Enqueue validates the request, creates the destination folder, stores the job, and wakes the scheduler.
3. The scheduler claims queued jobs up to the configured concurrency and builds argv as an array ending in `-- <link>`.
4. yt-dlp reports through an app-owned sentinel prefix. Because `--print after_move:…` implies quiet mode, the app passes `--progress` so download records still appear, and it parses records on stderr too, where yt-dlp writes post-processing progress. Updates reach the interface at most four times a second and SQLite every two seconds; status changes are written immediately.
5. Cancellation interrupts the process group, then kills it after two seconds.
6. Output paths are accepted only from yt-dlp's `after_move` record and only inside the chosen destination.
7. Optional GPU conversion writes a temporary MKV beside the source and removes the source only after the new file is finalized.

Quitting asks first when downloads are running, then stops them through the same cancellation path but records them as *interrupted*, so Retry resumes from the partial file.

## Persistence and recovery

The database lives in the platform data folder (or `YT_DLP_TUI_DATA_DIR`). An exclusive lock file stops a second copy from scheduling the same queue. At startup, jobs left in an active state become *interrupted* and queued work is scheduled again. Settings are one JSON document whose later fields carry serde defaults. Migrations are append-only once released.

## Security boundaries

- No shell command strings are built; every child gets typed argv elements, and links follow `--`.
- Links must be http(s) without embedded credentials. Proxy URLs with credentials are rejected so secrets are not stored.
- Cookie files are referenced by path and never copied. Browser cookies are read by yt-dlp itself.
- Diagnostics from both output streams are bounded and redact URL credentials, query secrets, authorization and cookie headers, and the home folder.
- Open and reveal accept a job id, re-validate that its file is inside the job's destination, and only then call the platform opener.
- Tool downloads are pinned to exact URLs and SHA-256 digests compiled into the app. A mismatch installs nothing. Only named archive entries are extracted, to fixed names, so archive paths never choose where files land. Nothing downloaded is executed during installation.
- There is no free-text argument box. Every yt-dlp feature is a typed control with validation.
