# yt-dlp capability audit

Audited against yt-dlp **2026.08.19**, the latest stable release, which `packaging/components.json` pins. `yt_dlp/options.py` is unchanged from 2026.07.04 (the version yt-dlp Desktop audited) and from `master` at `51bab8a` (2026-09-27), so the command-line surface is the same; every flag the app emits was checked to exist in the 2026.08.19 source, and the end-to-end tests download with the pinned build.

## Changes from the desktop audit

| Area | yt-dlp 2026.08.19 behavior | In this app |
|---|---|---|
| SponsorBlock categories | `SponsorBlockPP.CATEGORIES` includes `hook` (added 2025-11-01) and the non-skippable `poi_highlight` and `chapter`. | All three are offered. Highlights and community chapters can only be marked; `--sponsorblock-remove` rejects them, and so does the app. |
| Impersonation | curl_cffi targets are `chrome`, `edge`, `safari`, `firefox`, and `tor`. | Tor Browser is offered. |
| Browser cookies | `SUPPORTED_BROWSERS` adds `whale` to the Chromium family. | Whale is offered. |
| Progress under `--print` | `--print` implies `--quiet`, which hides download progress unless `--progress` is given, and post-processor progress goes to stderr. | The app passes `--progress` and parses protocol records on both streams. Without this, no live progress arrives with the real engine. |
| Clip file names | `--download-sections` output shares the full video's name, so `--no-overwrites` treats a later full download as already done. | Clip templates gain ` (clip %(section_start)d-%(section_end)ds)` before `.%(ext)s`. |
| Playlist items | `PlaylistEntries.PLAYLIST_ITEMS_RE`: indices and `[START][:STOP][:STEP]` or `START-STOP`, with `inf` and signed values; a zero step is invalid. | Validated with the same grammar; spaces are stripped. |
| JavaScript runtime | Deno 2.3.0 or newer is required for YouTube challenge solving. | Deno 2.9.7 is pinned and passed with `--js-runtimes deno:<path>`. |
| YouTube | `android_vr` left the default clients and `visionos` was added. | No app change; extraction is upstream behavior. |

## Control to command-line mapping

| Control | yt-dlp behavior |
|---|---|
| Analyze | `--dump-single-json --flat-playlist --yes-playlist\|--no-playlist --playlist-items :200` |
| Best available, with FFmpeg | yt-dlp's default `bv*+ba/b` (with a preferred audio language, `bv*+ba[language^=xx]/bv*+ba/b`) |
| Up to Np, with FFmpeg | `-f bv*[height<=N]+ba/b[height<=N] / wv*+ba/w` |
| Best single file, or no FFmpeg | `-f b` (capped: `b[height<=N] / b`) |
| Pick exact streams | `-f <video>+<audio>` from the analyzed formats |
| Container | `--merge-output-format <c> --remux-video <c>` (video only) |
| Video codec, Download as | `-S vcodec:avc\|vp9\|av01` |
| Video codec, Convert on GPU | FFmpeg after download with the first working NVENC or AMF encoder; quality 1–51; optional GPU decode, used only when the GPU decodes that input (checked on one frame), otherwise the CPU decodes |
| Audio, original | `-f ba` |
| Audio, converted | `-x --audio-format <f> --audio-quality <0\|bitrate>`, `--keep-video` |
| Audio track | `ba[language^=<code>]` in the selector |
| Live from start | `--live-from-start` |
| Clip | `--download-sections *start-end`, `--force-keyframes-at-cuts` for accurate cuts |
| Subtitles | `--write-subs`, `--write-auto-subs`, `--sub-langs`, `--convert-subs`, `--embed-subs` |
| Chapters | `--embed-chapters`, `--split-chapters`, `--remove-chapters (?i)<escaped text>` |
| SponsorBlock | `--sponsorblock-mark <cats>` or `--sponsorblock-remove <cats>` |
| Extra files | `--write-thumbnail --convert-thumbnails jpg\|png\|webp`, `--write-description`, `--write-info-json`, `--write-comments`, `--write-link` |
| File | `-o <preset or Settings template>`, `--embed-metadata`, `--embed-thumbnail`, `--restrict-filenames`, `--windows-filenames`, `--trim-filenames`, `--force-overwrites` (otherwise `--no-overwrites`) |
| Playlist | `--playlist-items`, reversed order as `--playlist-items ::-1`, `--playlist-random`, `--dateafter`, `--datebefore`, `--min-filesize`, `--max-filesize`, `--match-filters` for live and minimum length, `--max-downloads`, `--sleep-interval`; `--no-playlist` for a video inside a playlist |
| Connections | `--concurrent-fragments 1–16` |
| Referer, User agent | `--add-headers Referer:…`, `--add-headers User-Agent:…` |
| Settings, network | `--cookies-from-browser`, `--cookies`, `--proxy`, `--force-ipv4\|6`, `--socket-timeout`, `--xff`, `--impersonate`, `--sleep-requests`, `--extractor-retries`, `--legacy-server-connect` |
| Settings, transfer | `--retries`, `--fragment-retries`, `--limit-rate`, `--http-chunk-size`, `--no-mtime` |
| Managed by the app | `--ignore-config`, `--no-simulate`, `--newline`, `--color never`, `--progress`, `--progress-delta 0.25`, `--part`, `--continue`, `--progress-template`, `--print after_move:…`, `-P`, `--ffmpeg-location`, `--js-runtimes` |

## Deliberately not exposed

The same as yt-dlp Desktop: options that run programs or load code (`--exec`, `--use-postprocessor`, `--downloader`, `--plugin-dirs`, `--config-locations`, `--alias`), write or read outside the destination (`--batch-file`, `--load-info-json`, `--cache-dir`, `--print-to-file`, `--write-pages`), store secrets (usernames, passwords, two-factor codes, `.netrc`, client certificates), weaken transport security (`--no-check-certificates`, `--prefer-insecure`), self-update (`--update`; the app pins its own yt-dlp), and expression languages (`--parse-metadata`, `--replace-in-metadata`, free-form `--match-filters`). `--download-archive` remains deferred. There is no free-text argument box.

DRM circumvention is neither implemented nor supported. Extractor behavior is controlled by upstream yt-dlp and can change as services change; update by bumping the pins in `packaging/components.json` and re-running this audit.
