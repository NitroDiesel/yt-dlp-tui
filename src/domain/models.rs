use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaFormat {
    pub format_id: String,
    pub extension: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub bitrate_kbps: Option<f64>,
    pub file_size: Option<u64>,
    pub note: Option<String>,
    pub hdr: bool,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTrack {
    pub language: String,
    pub name: Option<String>,
    pub extensions: Vec<String>,
    pub automatic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaProbe {
    pub id: String,
    pub url: String,
    pub title: String,
    pub creator: Option<String>,
    pub duration_seconds: Option<f64>,
    pub is_playlist: bool,
    pub playlist_count: Option<u32>,
    pub is_live: bool,
    pub formats: Vec<MediaFormat>,
    pub subtitles: Vec<SubtitleTrack>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaMode {
    Video,
    Audio,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HardwareCodec {
    H264,
    Hevc,
    Av1,
}

impl HardwareCodec {
    pub const ALL: [Self; 3] = [Self::H264, Self::Hevc, Self::Av1];

    pub fn label(&self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::Hevc => "HEVC",
            Self::Av1 => "AV1",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HardwareEncoderProvider {
    Nvenc,
    Amf,
}

impl HardwareEncoderProvider {
    pub fn encoder_name(&self, codec: &HardwareCodec) -> &'static str {
        match (self, codec) {
            (Self::Nvenc, HardwareCodec::H264) => "h264_nvenc",
            (Self::Nvenc, HardwareCodec::Hevc) => "hevc_nvenc",
            (Self::Nvenc, HardwareCodec::Av1) => "av1_nvenc",
            (Self::Amf, HardwareCodec::H264) => "h264_amf",
            (Self::Amf, HardwareCodec::Hevc) => "hevc_amf",
            (Self::Amf, HardwareCodec::Av1) => "av1_amf",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Nvenc => "NVIDIA NVENC",
            Self::Amf => "AMD AMF",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoConversionOptions {
    pub codec: HardwareCodec,
    pub quality: u8,
    #[serde(default)]
    pub use_hardware_decode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClipOptions {
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub precise: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    #[default]
    Best,
    Mp3,
    M4a,
    Opus,
    Flac,
    Wav,
}

impl AudioFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Best => "best",
            Self::Mp3 => "mp3",
            Self::M4a => "m4a",
            Self::Opus => "opus",
            Self::Flac => "flac",
            Self::Wav => "wav",
        }
    }

    pub fn supports_bitrate(self) -> bool {
        matches!(self, Self::Mp3 | Self::M4a | Self::Opus)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum AudioQuality {
    #[default]
    #[serde(rename = "best")]
    Best,
    #[serde(rename = "320K")]
    Kbps320,
    #[serde(rename = "256K")]
    Kbps256,
    #[serde(rename = "192K")]
    Kbps192,
    #[serde(rename = "128K")]
    Kbps128,
}

impl AudioQuality {
    pub fn as_yt_dlp_value(self) -> &'static str {
        match self {
            Self::Best => "0",
            Self::Kbps320 => "320K",
            Self::Kbps256 => "256K",
            Self::Kbps192 => "192K",
            Self::Kbps128 => "128K",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadOptions {
    pub mode: MediaMode,
    pub quality: String,
    pub audio_format: AudioFormat,
    #[serde(default)]
    pub audio_quality: AudioQuality,
    pub subtitle_languages: Vec<String>,
    pub write_subtitles: bool,
    pub write_automatic_subtitles: bool,
    pub embed_subtitles: bool,
    pub embed_metadata: bool,
    pub embed_thumbnail: bool,
    pub playlist_items: Option<String>,
    pub custom_format: Option<String>,
    // Options added after the first release carry serde defaults so stored
    // jobs keep deserializing.
    #[serde(default)]
    pub clip: Option<ClipOptions>,
    #[serde(default)]
    pub video_conversion: Option<VideoConversionOptions>,
    #[serde(default)]
    pub container: Option<VideoContainer>,
    #[serde(default)]
    pub codec_preference: Option<CodecPreference>,
    #[serde(default)]
    pub subtitle_format: Option<SubtitleFormat>,
    #[serde(default)]
    pub embed_chapters: bool,
    #[serde(default)]
    pub split_chapters: bool,
    #[serde(default)]
    pub sponsorblock: Option<SponsorBlockOptions>,
    #[serde(default)]
    pub write_thumbnail: bool,
    #[serde(default)]
    pub write_description: bool,
    #[serde(default)]
    pub write_info_json: bool,
    #[serde(default)]
    pub restrict_filenames: bool,
    #[serde(default)]
    pub concurrent_fragments: Option<u8>,
    /// Upload-date window for playlists and channels, as `YYYYMMDD`.
    #[serde(default)]
    pub date_after: Option<String>,
    #[serde(default)]
    pub date_before: Option<String>,
    #[serde(default)]
    pub max_downloads: Option<u32>,
    #[serde(default)]
    pub sleep_interval: Option<u32>,
    /// Preferred audio track language code, for videos with several dubs.
    #[serde(default)]
    pub audio_language: Option<String>,
    /// For a video link that also names a playlist: download only the video.
    #[serde(default)]
    pub no_playlist: bool,
    #[serde(default)]
    pub live_from_start: bool,
    #[serde(default)]
    pub min_filesize_mb: Option<u32>,
    #[serde(default)]
    pub max_filesize_mb: Option<u32>,
    #[serde(default)]
    pub skip_live: bool,
    #[serde(default)]
    pub min_duration_seconds: Option<u32>,
    #[serde(default)]
    pub playlist_order: Option<PlaylistOrder>,
    #[serde(default)]
    pub referer: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub windows_filenames: bool,
    #[serde(default)]
    pub trim_filenames: Option<u32>,
    #[serde(default)]
    pub force_overwrites: bool,
    #[serde(default)]
    pub write_comments: bool,
    #[serde(default)]
    pub write_link: bool,
    #[serde(default)]
    pub thumbnail_format: Option<ThumbnailFormat>,
    #[serde(default)]
    pub keep_video: bool,
    /// Plain text; chapters whose title contains it are cut out.
    #[serde(default)]
    pub remove_chapters: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlaylistOrder {
    Reverse,
    Random,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThumbnailFormat {
    Jpg,
    Png,
    Webp,
}

impl ThumbnailFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jpg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IpVersion {
    Ipv4,
    Ipv6,
}

impl Default for DownloadOptions {
    /// Best-quality video with metadata embedded and every optional feature off.
    fn default() -> Self {
        Self {
            mode: MediaMode::Video,
            quality: "best".into(),
            audio_format: AudioFormat::Best,
            audio_quality: AudioQuality::Best,
            subtitle_languages: vec![],
            write_subtitles: false,
            write_automatic_subtitles: false,
            embed_subtitles: false,
            embed_metadata: true,
            embed_thumbnail: false,
            playlist_items: None,
            custom_format: None,
            clip: None,
            video_conversion: None,
            container: None,
            codec_preference: None,
            subtitle_format: None,
            embed_chapters: false,
            split_chapters: false,
            sponsorblock: None,
            write_thumbnail: false,
            write_description: false,
            write_info_json: false,
            restrict_filenames: false,
            concurrent_fragments: None,
            date_after: None,
            date_before: None,
            max_downloads: None,
            sleep_interval: None,
            audio_language: None,
            no_playlist: false,
            live_from_start: false,
            min_filesize_mb: None,
            max_filesize_mb: None,
            skip_live: false,
            min_duration_seconds: None,
            playlist_order: None,
            referer: None,
            user_agent: None,
            windows_filenames: false,
            trim_filenames: None,
            force_overwrites: false,
            write_comments: false,
            write_link: false,
            thumbnail_format: None,
            keep_video: false,
            remove_chapters: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VideoContainer {
    Mp4,
    Mkv,
    Webm,
    Mov,
    Avi,
}

impl VideoContainer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Mov => "mov",
            Self::Avi => "avi",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4",
            Self::Mkv => "MKV",
            Self::Webm => "WebM",
            Self::Mov => "MOV",
            Self::Avi => "AVI",
        }
    }

    /// MP4 and MKV hold every codec yt-dlp downloads, so yt-dlp remuxes into
    /// them. WebM, MOV, and AVI each accept only some codecs, so the app
    /// converts the finished download itself, re-encoding only what doesn't fit.
    pub fn converted_by_app(self) -> bool {
        matches!(self, Self::Webm | Self::Mov | Self::Avi)
    }

    /// Whether video the GPU encoded as `codec` can be copied into this
    /// container. AVI always re-encodes to Xvid, so GPU encoding is wasted.
    pub fn holds(self, codec: &HardwareCodec) -> bool {
        match self {
            Self::Mp4 | Self::Mkv => true,
            Self::Webm => *codec == HardwareCodec::Av1,
            Self::Mov => *codec != HardwareCodec::Av1,
            Self::Avi => false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CodecPreference {
    H264,
    Vp9,
    Av1,
}

impl CodecPreference {
    /// yt-dlp `--format-sort` key; `avc` is yt-dlp's name for H.264.
    pub fn sort_key(self) -> &'static str {
        match self {
            Self::H264 => "vcodec:avc",
            Self::Vp9 => "vcodec:vp9",
            Self::Av1 => "vcodec:av01",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleFormat {
    Srt,
    Vtt,
    Ass,
}

impl SubtitleFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::Vtt => "vtt",
            Self::Ass => "ass",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SponsorBlockMode {
    /// Adds the segments as chapters; the media is untouched.
    Mark,
    /// Cuts the segments out of the file with FFmpeg.
    Remove,
}

/// SponsorBlock segment kinds, matching `SponsorBlockPP.CATEGORIES` in
/// yt-dlp 2026.08.19.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SponsorCategory {
    Sponsor,
    Selfpromo,
    Interaction,
    Intro,
    Outro,
    Preview,
    Hook,
    Filler,
    MusicOfftopic,
    PoiHighlight,
    Chapter,
}

impl SponsorCategory {
    pub const ALL: [Self; 11] = [
        Self::Sponsor,
        Self::Selfpromo,
        Self::Interaction,
        Self::Intro,
        Self::Outro,
        Self::Preview,
        Self::Hook,
        Self::Filler,
        Self::MusicOfftopic,
        Self::PoiHighlight,
        Self::Chapter,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sponsor => "sponsor",
            Self::Selfpromo => "selfpromo",
            Self::Interaction => "interaction",
            Self::Intro => "intro",
            Self::Outro => "outro",
            Self::Preview => "preview",
            Self::Hook => "hook",
            Self::Filler => "filler",
            Self::MusicOfftopic => "music_offtopic",
            Self::PoiHighlight => "poi_highlight",
            Self::Chapter => "chapter",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Sponsor => "Sponsors",
            Self::Selfpromo => "Self promotion",
            Self::Interaction => "Like and subscribe reminders",
            Self::Intro => "Intros",
            Self::Outro => "Outros and end cards",
            Self::Preview => "Previews and recaps",
            Self::Hook => "Hooks and greetings",
            Self::Filler => "Off-topic filler",
            Self::MusicOfftopic => "Non-music in music videos",
            Self::PoiHighlight => "Highlight (mark only)",
            Self::Chapter => "Community chapters (mark only)",
        }
    }

    /// yt-dlp refuses to cut highlights and community chapters out of a file.
    pub fn removable(self) -> bool {
        !matches!(self, Self::PoiHighlight | Self::Chapter)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SponsorBlockOptions {
    pub mode: SponsorBlockMode,
    pub categories: Vec<SponsorCategory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub url: String,
    pub destination: String,
    pub filename_template: String,
    #[serde(default)]
    pub is_playlist: bool,
    pub options: DownloadOptions,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub percent: Option<f64>,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub speed_bytes_per_second: Option<f64>,
    pub eta_seconds: Option<f64>,
    pub playlist_index: Option<u32>,
    pub playlist_count: Option<u32>,
    pub filename: Option<String>,
    pub stage: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Analyzing,
    Downloading,
    PostProcessing,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Analyzing => "analyzing",
            Self::Downloading => "downloading",
            Self::PostProcessing => "post_processing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn is_running(self) -> bool {
        matches!(
            self,
            Self::Analyzing | Self::Downloading | Self::PostProcessing
        )
    }

    pub fn is_stopped(self) -> bool {
        matches!(self, Self::Failed | Self::Cancelled | Self::Interrupted)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJob {
    pub id: String,
    pub request: DownloadRequest,
    pub title: Option<String>,
    pub status: JobStatus,
    pub progress: DownloadProgress,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub output_path: Option<String>,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub download_directory: String,
    #[serde(default)]
    pub last_download_directory: Option<String>,
    pub filename_template: String,
    pub default_mode: MediaMode,
    pub default_quality: String,
    pub queue_concurrency: u8,
    /// "blue", "violet", "pink", or "graphite".
    #[serde(default = "default_accent")]
    pub accent: String,
    /// Clicks, scrolling, and hover. Off leaves the mouse to the terminal.
    #[serde(default = "default_mouse")]
    pub mouse: bool,
    pub yt_dlp_path: Option<String>,
    pub ffmpeg_path: Option<String>,
    pub deno_path: Option<String>,
    pub cookie_browser: Option<String>,
    pub cookie_file: Option<String>,
    pub proxy: Option<String>,
    pub rate_limit: Option<String>,
    pub retries: u8,
    pub fragment_retries: u8,
    #[serde(default)]
    pub ip_version: Option<IpVersion>,
    #[serde(default)]
    pub socket_timeout: Option<u32>,
    /// yt-dlp `--xff`: "default", "never", or a two-letter country code.
    #[serde(default)]
    pub geo_bypass: Option<String>,
    #[serde(default)]
    pub impersonate: Option<String>,
    #[serde(default)]
    pub sleep_requests: Option<u32>,
    #[serde(default)]
    pub http_chunk_size_mb: Option<u32>,
    #[serde(default)]
    pub extractor_retries: Option<u32>,
    #[serde(default)]
    pub legacy_server_connect: bool,
    /// Stamp files with the download time instead of the upload date.
    #[serde(default)]
    pub use_download_time: bool,
}

fn default_accent() -> String {
    "blue".into()
}

fn default_mouse() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        let download_directory = dirs::download_dir()
            .or_else(|| dirs::home_dir().map(|p| p.join("Downloads")))
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        Self {
            download_directory,
            last_download_directory: None,
            filename_template: "%(title).200B [%(id)s].%(ext)s".into(),
            default_mode: MediaMode::Video,
            default_quality: "best".into(),
            queue_concurrency: 1,
            accent: default_accent(),
            mouse: default_mouse(),
            yt_dlp_path: None,
            ffmpeg_path: None,
            deno_path: None,
            cookie_browser: None,
            cookie_file: None,
            proxy: None,
            rate_limit: None,
            retries: 10,
            fragment_retries: 10,
            ip_version: None,
            socket_timeout: None,
            geo_bypass: None,
            impersonate: None,
            sleep_requests: None,
            http_chunk_size_mb: None,
            extractor_retries: None,
            legacy_server_connect: false,
            use_download_time: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    YtDlp,
    Ffmpeg,
    Ffprobe,
    JavascriptRuntime,
}

impl DependencyKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::YtDlp => "yt-dlp",
            Self::Ffmpeg => "FFmpeg",
            Self::Ffprobe => "FFprobe",
            Self::JavascriptRuntime => "Deno",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DependencyInfo {
    pub kind: DependencyKind,
    /// "available", "missing", or "invalid".
    pub status: String,
    /// "custom", "bundled", "managed", or "not_found".
    pub source: String,
    pub path: Option<String>,
    pub version: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HardwareEncoderInfo {
    pub provider: HardwareEncoderProvider,
    pub codec: HardwareCodec,
    pub encoder: String,
    pub compiled: bool,
    pub available: bool,
    pub decode_backend: Option<String>,
    pub decode_available: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HardwareAccelerationInfo {
    pub status: String,
    pub encoders: Vec<HardwareEncoderInfo>,
    pub message: String,
}

impl HardwareAccelerationInfo {
    pub fn unavailable(status: &str, message: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            encoders: [HardwareEncoderProvider::Nvenc, HardwareEncoderProvider::Amf]
                .into_iter()
                .flat_map(|provider| {
                    HardwareCodec::ALL
                        .into_iter()
                        .map(move |codec| HardwareEncoderInfo {
                            encoder: provider.encoder_name(&codec).into(),
                            provider: provider.clone(),
                            codec,
                            compiled: false,
                            available: false,
                            decode_backend: None,
                            decode_available: false,
                            message: None,
                        })
                })
                .collect(),
            message: message.into(),
        }
    }

    /// Placeholder returned while the runtime GPU probe runs in the background.
    pub fn checking() -> Self {
        Self::unavailable(
            "checking",
            "Checking the installed GPU and driver for NVENC or AMF…",
        )
    }

    pub fn encoder_for(&self, codec: &HardwareCodec) -> Option<&HardwareEncoderInfo> {
        self.encoders
            .iter()
            .find(|encoder| &encoder.codec == codec && encoder.available)
    }
}

#[derive(Debug, Clone)]
pub struct AppSnapshot {
    pub settings: AppSettings,
    pub queue: Vec<DownloadJob>,
    pub history: Vec<DownloadJob>,
    pub dependencies: Vec<DependencyInfo>,
    pub hardware_acceleration: HardwareAccelerationInfo,
    pub queue_paused: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_stored_options_default_every_later_feature_to_off() {
        let value = serde_json::json!({
            "mode": "video",
            "quality": "best",
            "audioFormat": "best",
            "subtitleLanguages": [],
            "writeSubtitles": false,
            "writeAutomaticSubtitles": false,
            "embedSubtitles": false,
            "embedMetadata": true,
            "embedThumbnail": false,
            "playlistItems": null,
            "customFormat": null
        });
        let options: DownloadOptions = serde_json::from_value(value).unwrap();
        assert_eq!(options.audio_quality, AudioQuality::Best);
        assert!(options.clip.is_none());
        assert!(options.video_conversion.is_none());
        assert!(options.sponsorblock.is_none());
    }

    #[test]
    fn minimal_stored_settings_default_the_accent() {
        let defaults = AppSettings::default();
        let value = serde_json::json!({
            "downloadDirectory": defaults.download_directory,
            "filenameTemplate": defaults.filename_template,
            "defaultMode": "video",
            "defaultQuality": "best",
            "queueConcurrency": 1,
            "ytDlpPath": null,
            "ffmpegPath": null,
            "denoPath": null,
            "cookieBrowser": null,
            "cookieFile": null,
            "proxy": null,
            "rateLimit": null,
            "retries": 10,
            "fragmentRetries": 10
        });

        let settings: AppSettings = serde_json::from_value(value).unwrap();
        assert!(settings.last_download_directory.is_none());
        assert_eq!(settings.accent, "blue");
        assert!(settings.mouse, "mouse support starts on");
    }

    #[test]
    fn sponsor_categories_use_yt_dlp_names() {
        let names = SponsorCategory::ALL.map(SponsorCategory::as_str);
        assert_eq!(
            names,
            [
                "sponsor",
                "selfpromo",
                "interaction",
                "intro",
                "outro",
                "preview",
                "hook",
                "filler",
                "music_offtopic",
                "poi_highlight",
                "chapter",
            ]
        );
        for category in SponsorCategory::ALL {
            let json = serde_json::to_value(category).unwrap();
            assert_eq!(json, category.as_str());
        }
        assert!(!SponsorCategory::Chapter.removable());
        assert!(SponsorCategory::Hook.removable());
    }
}
