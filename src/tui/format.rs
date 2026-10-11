//! Display helpers shared by the views: sizes, times, links, and job summaries.

use chrono::{DateTime, Local};

use crate::domain::{AudioFormat, AudioQuality, ClipOptions, DownloadJob, JobStatus, MediaMode};

/// Decimal units, as download managers and file browsers report sizes.
pub fn format_bytes(bytes: Option<f64>) -> String {
    let Some(bytes) = bytes.filter(|bytes| bytes.is_finite() && *bytes >= 0.0) else {
        return "unknown".into();
    };
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes;
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if value >= 100.0 || unit == 0 {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

pub fn format_duration(total_seconds: f64) -> String {
    let total = total_seconds.max(0.0) as u64;
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub fn format_eta(seconds: Option<f64>) -> String {
    match seconds {
        None => "Estimating…".into(),
        Some(seconds) if seconds < 60.0 => format!("{} sec left", seconds.max(0.0).round()),
        Some(seconds) => format!("{} min left", (seconds / 60.0).ceil()),
    }
}

pub fn hostname(link: &str) -> String {
    url::Url::parse(link)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.trim_start_matches("www.").to_owned())
        })
        .unwrap_or_else(|| link.to_owned())
}

/// "14:05" for today, "Oct 7" otherwise.
pub fn format_when(iso: Option<&str>) -> String {
    let Some(time) = iso.and_then(|iso| DateTime::parse_from_rfc3339(iso).ok()) else {
        return String::new();
    };
    let local = time.with_timezone(&Local);
    if local.date_naive() == Local::now().date_naive() {
        local.format("%H:%M").to_string()
    } else {
        local.format("%b %-d").to_string()
    }
}

pub fn format_timestamp(iso: &str) -> String {
    DateTime::parse_from_rfc3339(iso)
        .map(|time| {
            time.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| iso.to_owned())
}

/// Extracts the first web link from pasted text, if any.
pub fn link_from_text(text: &str) -> Option<String> {
    let start = text.find("https://").or_else(|| text.find("http://"))?;
    let link = text[start..]
        .split(|character: char| character.is_whitespace() || "<>\"'".contains(character))
        .next()?;
    (link.len() > "https://".len()).then(|| link.to_owned())
}

/// A link like `watch?v=ID&list=ID` points at one video inside a playlist.
pub fn is_video_in_playlist(link: &str) -> bool {
    let Ok(url) = url::Url::parse(link) else {
        return false;
    };
    let has = |key: &str| url.query_pairs().any(|(name, _)| name == key);
    has("list")
        && (has("v")
            || url
                .host_str()
                .is_some_and(|host| host.ends_with("youtu.be")))
}

/// Parses seconds, `MM:SS`, or `HH:MM:SS`, each with up to three decimals.
pub fn parse_timecode(value: &str) -> Option<f64> {
    let input = value.trim();
    let parts = input.split(':').collect::<Vec<_>>();
    if input.is_empty() || parts.len() > 3 {
        return None;
    }
    let last = parts.len() - 1;
    for (index, part) in parts.iter().enumerate() {
        let (whole, fraction) = part.split_once('.').unwrap_or((part, ""));
        let valid = !whole.is_empty()
            && whole.chars().all(|character| character.is_ascii_digit())
            && (fraction.is_empty() && !part.ends_with('.')
                || index == last
                    && (1..=3).contains(&fraction.len())
                    && fraction.chars().all(|character| character.is_ascii_digit()));
        if !valid {
            return None;
        }
    }
    let values = parts
        .iter()
        .map(|part| part.parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let seconds = values[last];
    let minutes = if parts.len() > 1 {
        values[last - 1]
    } else {
        0.0
    };
    let hours = if parts.len() > 2 { values[0] } else { 0.0 };
    if (parts.len() > 1 && seconds >= 60.0) || (parts.len() > 2 && minutes >= 60.0) {
        return None;
    }
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

pub fn format_timecode(seconds: f64) -> String {
    let total_milliseconds = (seconds.max(0.0) * 1000.0).round() as u64;
    let hours = total_milliseconds / 3_600_000;
    let minutes = (total_milliseconds % 3_600_000) / 60_000;
    let whole_seconds = (total_milliseconds % 60_000) / 1000;
    let milliseconds = total_milliseconds % 1000;
    let fractional = if milliseconds > 0 {
        format!(".{:03}", milliseconds)
            .trim_end_matches('0')
            .to_owned()
    } else {
        String::new()
    };
    if hours > 0 {
        format!("{hours}:{minutes:02}:{whole_seconds:02}{fractional}")
    } else {
        format!("{minutes}:{whole_seconds:02}{fractional}")
    }
}

/// Validates a clip's start and end against the media length.
pub fn validate_clip(
    start: &str,
    end: &str,
    precise: bool,
    media_duration: Option<f64>,
) -> Result<ClipOptions, String> {
    let start =
        parse_timecode(start).ok_or("Enter the start time as seconds, MM:SS, or HH:MM:SS.")?;
    let end = parse_timecode(end).ok_or("Enter the end time as seconds, MM:SS, or HH:MM:SS.")?;
    if end <= start {
        return Err("End time must be after start time.".into());
    }
    if let Some(duration) = media_duration.filter(|duration| duration.is_finite())
        && end > duration + 0.001
    {
        return Err(format!(
            "End time must be within {}.",
            format_timecode(duration)
        ));
    }
    Ok(ClipOptions {
        start_seconds: start,
        duration_seconds: end - start,
        precise,
    })
}

/// The list a filter shows; `All` holds everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadFilter {
    All,
    Downloading,
    Completed,
    Stopped,
}

impl DownloadFilter {
    pub const ALL: [Self; 4] = [Self::All, Self::Downloading, Self::Completed, Self::Stopped];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All downloads",
            Self::Downloading => "Downloading",
            Self::Completed => "Completed",
            Self::Stopped => "Stopped",
        }
    }

    pub fn of(status: JobStatus) -> Self {
        match status {
            JobStatus::Completed => Self::Completed,
            status if status.is_stopped() => Self::Stopped,
            _ => Self::Downloading,
        }
    }

    pub fn includes(self, status: JobStatus) -> bool {
        self == Self::All || self == Self::of(status)
    }
}

/// One downloads list built from the persisted queue and history. Running and
/// waiting jobs keep their queue order; finished ones follow, newest first.
pub fn merge_downloads<'a>(
    queue: &'a [DownloadJob],
    history: &'a [DownloadJob],
) -> Vec<&'a DownloadJob> {
    let mut in_progress = Vec::new();
    let mut finished: Vec<&DownloadJob> = Vec::new();
    for job in queue {
        if DownloadFilter::of(job.status) == DownloadFilter::Downloading {
            in_progress.push(job);
        } else {
            finished.push(job);
        }
    }
    for job in history {
        let known = finished.iter().any(|seen| seen.id == job.id)
            || in_progress.iter().any(|seen| seen.id == job.id);
        if !known {
            finished.push(job);
        }
    }
    finished.sort_by(|a, b| {
        let a = a.finished_at.as_deref().unwrap_or(&a.created_at);
        let b = b.finished_at.as_deref().unwrap_or(&b.created_at);
        b.cmp(a)
    });
    in_progress.extend(finished);
    in_progress
}

/// "Video, up to 1080p", "MP3, 320 kbps", "Exact format 137+140".
pub fn format_summary(job: &DownloadJob) -> String {
    let options = &job.request.options;
    match options.mode {
        MediaMode::Audio if options.audio_format == AudioFormat::Best => {
            "Audio, original format".into()
        }
        MediaMode::Audio => {
            let bitrate = match options.audio_quality {
                AudioQuality::Best => String::new(),
                quality => format!(", {} kbps", quality.as_yt_dlp_value().trim_end_matches('K')),
            };
            format!("{}{bitrate}", options.audio_format.as_str().to_uppercase())
        }
        MediaMode::Custom => match options.custom_format.as_deref() {
            Some(format) => format!("Exact format {format}"),
            None => "Exact format".into(),
        },
        MediaMode::Video => match options.quality.as_str() {
            "best" => "Video, best quality".into(),
            "single" => "Video, single file".into(),
            height => format!("Video, up to {height}p"),
        },
    }
}

pub fn clip_summary(job: &DownloadJob) -> Option<String> {
    job.request.options.clip.as_ref().map(|clip| {
        format!(
            "{} to {}",
            format_timecode(clip.start_seconds),
            format_timecode(clip.start_seconds + clip.duration_seconds)
        )
    })
}

pub fn job_meta(job: &DownloadJob) -> String {
    let format = match clip_summary(job) {
        Some(clip) => format!("{}, clip {clip}", format_summary(job)),
        None => format_summary(job),
    };
    format!("{format} · {}", hostname(&job.request.url))
}

pub fn job_title(job: &DownloadJob) -> String {
    job.title
        .clone()
        .unwrap_or_else(|| hostname(&job.request.url))
}

/// Everything known about a download as plain text, for pasting into a bug
/// report: what was asked for, what happened, and every diagnostic line.
pub fn technical_report(job: &DownloadJob) -> String {
    let mut lines = vec![
        format!(
            "yt-dlp TUI {} ({} {})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        format!("Title: {}", job_title(job)),
        format!("Link: {}", job.request.url),
        format!("Status: {}", status_label(job.status)),
    ];
    if let Some(stage) = &job.progress.stage {
        lines.push(format!("Stage: {stage}"));
    }
    if let Some(message) = &job.error_message {
        match &job.error_category {
            Some(category) => lines.push(format!("Error ({category}): {message}")),
            None => lines.push(format!("Error: {message}")),
        }
    }
    lines.push(format!("Format: {}", format_summary(job)));
    if let Some(clip) = clip_summary(job) {
        lines.push(format!("Clip: {clip}"));
    }
    match &job.output_path {
        Some(path) => lines.push(format!("File: {path}")),
        None => lines.push(format!("Folder: {}", job.request.destination)),
    }
    lines.push(format!("Added: {}", job.created_at));
    if let Some(started) = &job.started_at {
        lines.push(format!("Started: {started}"));
    }
    if let Some(finished) = &job.finished_at {
        lines.push(format!("Finished: {finished}"));
    }
    if let Ok(options) = serde_json::to_string(&job.request.options) {
        lines.push(format!("Options: {options}"));
    }
    if !job.diagnostics.is_empty() {
        lines.push(String::new());
        lines.push("Technical details:".into());
        lines.extend(job.diagnostics.iter().cloned());
    }
    let mut report = lines.join("\n");
    report.push('\n');
    report
}

pub fn status_label(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Queued => "Waiting",
        JobStatus::Analyzing => "Starting",
        JobStatus::Downloading => "Downloading",
        JobStatus::PostProcessing => "Finishing",
        JobStatus::Completed => "Completed",
        JobStatus::Failed => "Needs attention",
        JobStatus::Cancelled => "Cancelled",
        JobStatus::Interrupted => "Interrupted",
    }
}

/// Progress as shown in the list: finishing and finished jobs read as full.
pub fn percent_of(job: &DownloadJob) -> f64 {
    match job.status {
        JobStatus::Completed | JobStatus::PostProcessing => 100.0,
        _ => job.progress.percent.unwrap_or(0.0).clamp(0.0, 100.0),
    }
}

/// "Japanese (ja)" for known codes; anything else is shown as-is.
pub fn language_name(code: &str) -> String {
    let base = code
        .split("-orig")
        .next()
        .unwrap_or(code)
        .split(['-', '_'])
        .next()
        .unwrap_or(code)
        .to_ascii_lowercase();
    let name = match base.as_str() {
        "af" => "Afrikaans",
        "ar" => "Arabic",
        "bg" => "Bulgarian",
        "bn" => "Bengali",
        "ca" => "Catalan",
        "cs" => "Czech",
        "da" => "Danish",
        "de" => "German",
        "el" => "Greek",
        "en" => "English",
        "es" => "Spanish",
        "et" => "Estonian",
        "fa" => "Persian",
        "fi" => "Finnish",
        "fil" => "Filipino",
        "fr" => "French",
        "he" | "iw" => "Hebrew",
        "hi" => "Hindi",
        "hr" => "Croatian",
        "hu" => "Hungarian",
        "id" => "Indonesian",
        "it" => "Italian",
        "ja" => "Japanese",
        "ko" => "Korean",
        "lt" => "Lithuanian",
        "lv" => "Latvian",
        "ms" => "Malay",
        "nl" => "Dutch",
        "no" | "nb" => "Norwegian",
        "pl" => "Polish",
        "pt" => "Portuguese",
        "ro" => "Romanian",
        "ru" => "Russian",
        "sk" => "Slovak",
        "sl" => "Slovenian",
        "sr" => "Serbian",
        "sv" => "Swedish",
        "sw" => "Swahili",
        "ta" => "Tamil",
        "te" => "Telugu",
        "th" => "Thai",
        "tl" => "Tagalog",
        "tr" => "Turkish",
        "uk" => "Ukrainian",
        "ur" => "Urdu",
        "vi" => "Vietnamese",
        "zh" => "Chinese",
        _ => return code.to_owned(),
    };
    format!("{name} ({code})")
}

/// Truncates to `width` terminal columns, ending with an ellipsis when cut.
pub fn truncate(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let total: usize = text.chars().map(|c| c.width().unwrap_or(0)).sum();
    if total <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 0;
    let mut output = String::new();
    for character in text.chars() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > width - 1 {
            break;
        }
        used += character_width;
        output.push(character);
    }
    output.push('…');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes_like_a_download_manager() {
        assert_eq!(format_bytes(Some(999.0)), "999 B");
        assert_eq!(format_bytes(Some(1_500.0)), "1.5 KB");
        assert_eq!(format_bytes(Some(158_000_000.0)), "158 MB");
        assert_eq!(format_bytes(None), "unknown");
    }

    #[test]
    fn parses_and_formats_timecodes() {
        assert_eq!(parse_timecode("90"), Some(90.0));
        assert_eq!(parse_timecode("1:30"), Some(90.0));
        assert_eq!(parse_timecode("1:02:03.5"), Some(3723.5));
        assert_eq!(parse_timecode("0:60"), None);
        assert_eq!(parse_timecode("1:60:00"), None);
        assert_eq!(parse_timecode("1:2:3:4"), None);
        assert_eq!(parse_timecode("1.5:00"), None);
        assert_eq!(parse_timecode("12."), None);
        assert_eq!(parse_timecode("abc"), None);
        assert_eq!(format_timecode(3723.5), "1:02:03.5");
        assert_eq!(format_timecode(65.0), "1:05");
        assert_eq!(format_timecode(0.25), "0:00.25");
    }

    #[test]
    fn validates_clips_against_the_media_length() {
        let clip = validate_clip("0:10", "0:40", true, Some(60.0)).unwrap();
        assert_eq!(clip.start_seconds, 10.0);
        assert_eq!(clip.duration_seconds, 30.0);
        assert!(validate_clip("0:40", "0:10", true, None).is_err());
        assert!(validate_clip("0:10", "2:00", true, Some(60.0)).is_err());
        assert!(validate_clip("x", "0:10", true, None).is_err());
    }

    #[test]
    fn finds_links_in_pasted_text() {
        assert_eq!(
            link_from_text("watch this: https://youtu.be/abc?t=4 now").as_deref(),
            Some("https://youtu.be/abc?t=4")
        );
        assert_eq!(link_from_text("no link here"), None);
        assert_eq!(link_from_text("https://"), None);
    }

    #[test]
    fn recognises_a_video_inside_a_playlist() {
        assert!(is_video_in_playlist(
            "https://www.youtube.com/watch?v=abc&list=PL123"
        ));
        assert!(is_video_in_playlist("https://youtu.be/abc?list=PL123"));
        assert!(!is_video_in_playlist(
            "https://www.youtube.com/playlist?list=PL123"
        ));
        assert!(!is_video_in_playlist("https://www.youtube.com/watch?v=abc"));
    }

    #[test]
    fn names_languages_and_keeps_unknown_codes() {
        assert_eq!(language_name("ja"), "Japanese (ja)");
        assert_eq!(language_name("en-US"), "English (en-US)");
        assert_eq!(language_name("de-orig"), "German (de-orig)");
        assert_eq!(language_name("xx"), "xx");
    }

    #[test]
    fn truncates_by_display_width() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("a longer title", 8), "a longe…");
        assert_eq!(truncate("日本語のタイトル", 7), "日本語…");
    }
}
