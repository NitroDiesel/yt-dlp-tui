//! The "New download" form: read a link, choose what to save, then download
//! or queue it. Mirrors the desktop app's dialog, field for field.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::{Line, Span};

use super::{
    form::{
        self, ChoiceOption, Control, FormModel, FormOutcome, FormState, MultiItem, Row, button,
        choice, choices, text, toggle,
    },
    format::{
        format_bytes, format_duration, format_timecode, hostname, is_video_in_playlist,
        language_name, link_from_text, validate_clip,
    },
    text_input::TextInput,
    theme::Theme,
};
use crate::domain::{
    AppSettings, AudioFormat, AudioQuality, CodecPreference, DownloadOptions, DownloadRequest,
    HardwareAccelerationInfo, HardwareCodec, MediaFormat, MediaMode, MediaProbe, PlaylistOrder,
    SponsorBlockMode, SponsorBlockOptions, SponsorCategory, SubtitleFormat, ThumbnailFormat,
    VideoContainer, VideoConversionOptions,
};

pub const DEFAULT_TEMPLATE: &str = "%(title).200B [%(id)s].%(ext)s";

const QUALITIES: [(&str, &str); 6] = [
    ("best", "Best available"),
    ("2160", "Up to 2160p"),
    ("1440", "Up to 1440p"),
    ("1080", "Up to 1080p"),
    ("720", "Up to 720p"),
    ("single", "Best single file, no merging"),
];

const AUDIO_FORMATS: [(AudioFormat, &str); 6] = [
    (AudioFormat::Best, "Original format, no conversion"),
    (AudioFormat::Mp3, "MP3, widest compatibility"),
    (AudioFormat::M4a, "M4A, AAC audio"),
    (AudioFormat::Opus, "Opus, small and high quality"),
    (AudioFormat::Flac, "FLAC, lossless"),
    (AudioFormat::Wav, "WAV, uncompressed"),
];

const AUDIO_QUALITIES: [(AudioQuality, &str); 5] = [
    (AudioQuality::Best, "Best available"),
    (AudioQuality::Kbps320, "320 kbps"),
    (AudioQuality::Kbps256, "256 kbps"),
    (AudioQuality::Kbps192, "192 kbps"),
    (AudioQuality::Kbps128, "128 kbps"),
];

const CONTAINERS: [(Option<VideoContainer>, &str); 4] = [
    (None, "Automatic"),
    (Some(VideoContainer::Mp4), "MP4, plays on most devices"),
    (Some(VideoContainer::Mkv), "MKV, keeps every stream"),
    (Some(VideoContainer::Webm), "WebM"),
];

const CODECS: [(Option<CodecPreference>, &str); 4] = [
    (None, "Any, best quality first"),
    (Some(CodecPreference::H264), "H.264, most compatible"),
    (Some(CodecPreference::Vp9), "VP9"),
    (Some(CodecPreference::Av1), "AV1, smallest files"),
];

const SUBTITLE_FORMATS: [(Option<SubtitleFormat>, &str); 4] = [
    (None, "Keep original"),
    (Some(SubtitleFormat::Srt), "SRT"),
    (Some(SubtitleFormat::Vtt), "VTT"),
    (Some(SubtitleFormat::Ass), "ASS"),
];

const THUMBNAIL_FORMATS: [(ThumbnailFormat, &str); 3] = [
    (ThumbnailFormat::Jpg, "JPG"),
    (ThumbnailFormat::Png, "PNG"),
    (ThumbnailFormat::Webp, "WebP"),
];

/// (key, label, template); `None` uses the Settings template.
const FILENAME_PRESETS: [(&str, &str, Option<&str>); 6] = [
    ("default", "Use the Settings template", None),
    ("title", "Title", Some("%(title).200B.%(ext)s")),
    ("title-id", "Title [video ID]", Some(DEFAULT_TEMPLATE)),
    (
        "uploader",
        "Uploader - Title",
        Some("%(uploader).80B - %(title).150B.%(ext)s"),
    ),
    (
        "date",
        "Upload date Title",
        Some("%(upload_date>%Y-%m-%d)s %(title).180B.%(ext)s"),
    ),
    (
        "index",
        "Playlist number - Title",
        Some("%(playlist_index)03d - %(title).180B.%(ext)s"),
    ),
];

const TRIM_LENGTHS: [(Option<u32>, &str); 4] = [
    (None, "No limit"),
    (Some(150), "150 characters"),
    (Some(100), "100 characters"),
    (Some(60), "60 characters"),
];

const MIN_DURATIONS: [(Option<u32>, &str); 4] = [
    (None, "No minimum"),
    (Some(60), "1 minute"),
    (Some(180), "3 minutes, skips Shorts"),
    (Some(600), "10 minutes"),
];

const SLEEP_INTERVALS: [(Option<u32>, &str); 5] = [
    (None, "No pause"),
    (Some(5), "5 seconds"),
    (Some(15), "15 seconds"),
    (Some(30), "30 seconds"),
    (Some(60), "1 minute"),
];

const CONNECTIONS: [(Option<u8>, &str); 5] = [
    (None, "1, gentlest on the site"),
    (Some(2), "2"),
    (Some(4), "4, faster for streamed video"),
    (Some(8), "8"),
    (Some(16), "16"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Link,
    Scope,
    Kind,
    Quality,
    VideoStream,
    AudioStream,
    Container,
    VideoCodec,
    GpuQuality,
    GpuDecode,
    AudioFormat,
    Bitrate,
    AudioTrack,
    LiveFromStart,
    Clip,
    ClipStart,
    ClipEnd,
    ClipCut,
    Destination,
    Advanced,
    KeepVideo,
    WriteSubs,
    WriteAutoSubs,
    SubtitlePick,
    SubtitleLanguages,
    SubtitleFormat,
    EmbedSubs,
    EmbedChapters,
    SplitChapters,
    RemoveChapters,
    SponsorBlock,
    SponsorCategories,
    WriteThumbnail,
    ThumbnailFormat,
    WriteDescription,
    WriteInfoJson,
    WriteComments,
    WriteLink,
    FilenamePreset,
    EmbedMetadata,
    EmbedThumbnail,
    RestrictFilenames,
    WindowsFilenames,
    TrimFilenames,
    ForceOverwrites,
    PlaylistItems,
    PlaylistOrder,
    DateAfter,
    DateBefore,
    MinFilesize,
    MaxFilesize,
    MinDuration,
    SkipLive,
    MaxDownloads,
    SleepInterval,
    Connections,
    Referer,
    UserAgent,
    Analyze,
    Queue,
    Download,
}

/// What the app should do after a key in this form.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    None,
    Close,
    Analyze { url: String, no_playlist: bool },
    Submit { start_immediately: bool },
}

pub struct NewDownload {
    form: FormState<Field>,
    link: TextInput,
    whole_playlist: bool,
    pub probe: Option<MediaProbe>,
    pub analyzing: bool,
    pub revision: u64,
    pub analyze_error: Option<String>,
    options: DownloadOptions,
    video_stream: Option<String>,
    audio_stream: Option<String>,
    filename_preset: &'static str,
    clip_on: bool,
    clip_start: TextInput,
    clip_end: TextInput,
    clip_precise: bool,
    destination: TextInput,
    destination_suggestion: Option<String>,
    advanced: bool,
    subtitle_languages: TextInput,
    remove_chapters: TextInput,
    playlist_items: TextInput,
    date_after: TextInput,
    date_before: TextInput,
    min_filesize: TextInput,
    max_filesize: TextInput,
    max_downloads: TextInput,
    referer: TextInput,
    user_agent: TextInput,
    pub submitting: bool,
    pub error: Option<String>,
    pub yt_dlp_ready: bool,
    pub ffmpeg_ready: bool,
    pub hardware: HardwareAccelerationInfo,
    pub theme: Theme,
    settings_template: String,
}

fn has_codec(codec: Option<&String>) -> bool {
    codec.is_some_and(|codec| codec != "none")
}

/// "1080p60, MP4, avc1, 2.1 MB, HDR (ID 137)".
fn stream_label(format: &MediaFormat) -> String {
    let mut parts = Vec::new();
    if let Some(height) = format.height {
        let fps = format
            .fps
            .filter(|fps| *fps > 30.0)
            .map(|fps| format!("{}", fps.round()))
            .unwrap_or_default();
        parts.push(format!("{height}p{fps}"));
    }
    parts.push(format.extension.to_uppercase());
    if has_codec(format.video_codec.as_ref()) {
        parts.push(
            format
                .video_codec
                .clone()
                .unwrap_or_default()
                .split('.')
                .next()
                .unwrap_or_default()
                .to_owned(),
        );
    } else if has_codec(format.audio_codec.as_ref()) {
        parts.push(
            format
                .audio_codec
                .clone()
                .unwrap_or_default()
                .split('.')
                .next()
                .unwrap_or_default()
                .to_owned(),
        );
    }
    if let Some(bitrate) = format.bitrate_kbps {
        parts.push(format!("{} kbps", bitrate.round()));
    }
    if let Some(size) = format.file_size {
        parts.push(format_bytes(Some(size as f64)));
    }
    if format.hdr {
        parts.push("HDR".into());
    }
    if let Some(language) = format.language.as_deref() {
        parts.push(language_name(language));
    }
    format!("{} (ID {})", parts.join(", "), format.format_id)
}

/// Expands a leading `~` so typed paths work as they do in a shell.
pub fn expand_home(value: &str) -> PathBuf {
    let value = value.trim();
    if value == "~" {
        return dirs::home_dir().unwrap_or_default();
    }
    if let Some(rest) = value.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(value)
}

/// The rest of the first folder (or, with `files`, file) name that completes
/// `value`, fish-style. Folders end with a separator so typing can continue.
pub fn path_suggestion(value: &str, files: bool) -> Option<String> {
    if value.trim().is_empty() {
        return None;
    }
    let expanded = expand_home(value);
    let typed = expanded.to_string_lossy();
    let (parent, prefix) = if typed.ends_with(std::path::MAIN_SEPARATOR) {
        (expanded.clone(), String::new())
    } else {
        (
            expanded.parent()?.to_path_buf(),
            expanded.file_name()?.to_string_lossy().into_owned(),
        )
    };
    let mut names = std::fs::read_dir(&parent)
        .ok()?
        .take(2000)
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let folder = entry.path().is_dir();
            (folder || files).then(|| (entry.file_name().into_string().ok(), folder))
        })
        .filter_map(|(name, folder)| Some((name?, folder)))
        .filter(|(name, _)| {
            name.starts_with(&prefix)
                && name.len() > prefix.len()
                && (prefix.starts_with('.') || !name.starts_with('.'))
        })
        .collect::<Vec<_>>();
    names.sort_by_key(|(name, _)| name.to_lowercase());
    names.first().map(|(name, folder)| {
        let rest = &name[prefix.len()..];
        if *folder {
            format!("{rest}{}", std::path::MAIN_SEPARATOR)
        } else {
            rest.to_owned()
        }
    })
}

fn parse_count(input: &TextInput, label: &str) -> Result<Option<u32>, String> {
    input
        .trimmed()
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|_| format!("Enter {label} as a whole number."))
        })
        .transpose()
}

fn parse_date(input: &TextInput, label: &str) -> Result<Option<String>, String> {
    let Some(value) = input.trimmed() else {
        return Ok(None);
    };
    let compact = value.replace('-', "");
    if compact.len() == 8 && chrono::NaiveDate::parse_from_str(&compact, "%Y%m%d").is_ok() {
        Ok(Some(compact))
    } else {
        Err(format!("Enter the {label} date as YYYY-MM-DD."))
    }
}

impl NewDownload {
    pub fn new(
        settings: &AppSettings,
        yt_dlp_ready: bool,
        ffmpeg_ready: bool,
        hardware: HardwareAccelerationInfo,
        theme: Theme,
    ) -> Self {
        let destination = settings
            .last_download_directory
            .clone()
            .unwrap_or_else(|| settings.download_directory.clone());
        let mut new = Self {
            form: FormState::default(),
            link: TextInput::default(),
            whole_playlist: false,
            probe: None,
            analyzing: false,
            revision: 0,
            analyze_error: None,
            options: DownloadOptions {
                mode: if settings.default_mode == MediaMode::Audio {
                    MediaMode::Audio
                } else {
                    MediaMode::Video
                },
                quality: settings.default_quality.clone(),
                ..DownloadOptions::default()
            },
            video_stream: None,
            audio_stream: None,
            filename_preset: "default",
            clip_on: false,
            clip_start: TextInput::default(),
            clip_end: TextInput::default(),
            clip_precise: true,
            destination: TextInput::new(destination),
            destination_suggestion: None,
            advanced: false,
            subtitle_languages: TextInput::default(),
            remove_chapters: TextInput::default(),
            playlist_items: TextInput::default(),
            date_after: TextInput::default(),
            date_before: TextInput::default(),
            min_filesize: TextInput::default(),
            max_filesize: TextInput::default(),
            max_downloads: TextInput::default(),
            referer: TextInput::default(),
            user_agent: TextInput::default(),
            submitting: false,
            error: None,
            yt_dlp_ready,
            ffmpeg_ready,
            hardware,
            theme,
            settings_template: settings.filename_template.clone(),
        };
        new.form.focus(Field::Link);
        new
    }

    pub fn link(&self) -> &str {
        self.link.value()
    }

    /// Starts reading `url`, as pasting a link does.
    pub fn set_link(&mut self, url: &str) -> Effect {
        self.link.set(url);
        self.text_changed(Field::Link);
        self.analyze()
    }

    fn analyze(&mut self) -> Effect {
        let url = self.link.value().trim().to_owned();
        if url.is_empty() || !self.yt_dlp_ready || self.analyzing {
            return Effect::None;
        }
        self.revision += 1;
        self.analyzing = true;
        self.analyze_error = None;
        self.probe = None;
        self.clip_on = false;
        self.error = None;
        Effect::Analyze {
            no_playlist: is_video_in_playlist(&url) && !self.whole_playlist,
            url,
        }
    }

    /// Applies an analysis result if it still belongs to the current link.
    pub fn analyzed(&mut self, revision: u64, result: Result<MediaProbe, String>) {
        if revision != self.revision {
            return;
        }
        self.analyzing = false;
        match result {
            Ok(probe) => {
                self.probe = Some(probe);
                self.form.focus(Field::Kind);
            }
            Err(error) => self.analyze_error = Some(error),
        }
    }

    /// Stops waiting for the current analysis.
    pub fn cancel_analysis(&mut self) {
        self.revision += 1;
        self.analyzing = false;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Effect {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if !self.form.popup_open() {
            match key.code {
                KeyCode::Esc => return Effect::Close,
                KeyCode::Char('d') if control => return self.primary(true),
                KeyCode::Char('q') if control => return self.primary(false),
                _ => {}
            }
        }
        match form::handle_key(self, key) {
            FormOutcome::Activated(Field::Link | Field::Analyze) => self.analyze(),
            FormOutcome::Activated(Field::Queue) => self.primary(false),
            FormOutcome::Activated(Field::Download) => self.primary(true),
            FormOutcome::Activated(Field::Advanced) => {
                self.advanced = !self.advanced;
                Effect::None
            }
            FormOutcome::Activated(_) => {
                form::handle_key(self, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
                Effect::None
            }
            FormOutcome::Handled | FormOutcome::Ignored => Effect::None,
        }
    }

    /// Download (or queue) once the link was read; read it first otherwise.
    fn primary(&mut self, start_immediately: bool) -> Effect {
        if self.probe.is_none() {
            return self.analyze();
        }
        if self.submitting {
            return Effect::None;
        }
        Effect::Submit { start_immediately }
    }

    pub fn paste(&mut self, pasted: &str) -> Effect {
        let focus = self.form.focus;
        if focus == Some(Field::Link) || focus.is_none() {
            return match link_from_text(pasted) {
                Some(link) => self.set_link(&link),
                None => {
                    form::paste(self, pasted);
                    Effect::None
                }
            };
        }
        if form::paste(self, pasted).is_none()
            && let Some(link) = link_from_text(pasted)
        {
            self.form.focus(Field::Link);
            return self.set_link(&link);
        }
        Effect::None
    }

    fn picking(&self) -> bool {
        self.options.mode == MediaMode::Custom
    }

    fn is_playlist(&self) -> bool {
        self.probe.as_ref().is_some_and(|probe| probe.is_playlist)
    }

    fn streams(&self) -> (Vec<&MediaFormat>, Vec<&MediaFormat>) {
        let usable = self
            .probe
            .iter()
            .flat_map(|probe| &probe.formats)
            .filter(|format| format.extension != "mhtml")
            .collect::<Vec<_>>();
        let mut video = usable
            .iter()
            .copied()
            .filter(|format| has_codec(format.video_codec.as_ref()))
            .collect::<Vec<_>>();
        video.sort_by(|a, b| {
            b.height.cmp(&a.height).then(
                b.bitrate_kbps
                    .unwrap_or(0.0)
                    .total_cmp(&a.bitrate_kbps.unwrap_or(0.0)),
            )
        });
        let mut audio = usable
            .iter()
            .copied()
            .filter(|format| {
                has_codec(format.audio_codec.as_ref()) && !has_codec(format.video_codec.as_ref())
            })
            .collect::<Vec<_>>();
        audio.sort_by(|a, b| {
            b.bitrate_kbps
                .unwrap_or(0.0)
                .total_cmp(&a.bitrate_kbps.unwrap_or(0.0))
        });
        (video, audio)
    }

    fn max_height(&self) -> u32 {
        self.probe
            .iter()
            .flat_map(|probe| &probe.formats)
            .filter_map(|format| format.height)
            .max()
            .unwrap_or(0)
    }

    fn audio_languages(&self) -> Vec<String> {
        let mut languages = Vec::new();
        for format in self.probe.iter().flat_map(|probe| &probe.formats) {
            if has_codec(format.audio_codec.as_ref())
                && let Some(language) = &format.language
                && !languages.contains(language)
            {
                languages.push(language.clone());
            }
        }
        languages
    }

    /// Languages offered by the source, human subtitles first.
    fn subtitle_choices(&self) -> Vec<String> {
        let mut languages: Vec<(String, bool)> = Vec::new();
        for track in self.probe.iter().flat_map(|probe| &probe.subtitles) {
            match languages
                .iter_mut()
                .find(|(language, _)| *language == track.language)
            {
                Some(entry) => entry.1 &= track.automatic,
                None => languages.push((track.language.clone(), track.automatic)),
            }
        }
        languages
            .into_iter()
            .filter(|(_, automatic)| !automatic || self.options.write_automatic_subtitles)
            .map(|(language, _)| language)
            .take(200)
            .collect()
    }

    fn chosen_languages(&self) -> Vec<String> {
        self.subtitle_languages
            .value()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn clip_disabled_reason(&self) -> Option<&'static str> {
        let probe = self.probe.as_ref()?;
        if probe.is_playlist {
            Some("Clips work with one video at a time.")
        } else if probe.is_live {
            Some("Live streams can't be trimmed before downloading.")
        } else if !self.ffmpeg_ready {
            Some("Cutting a clip needs FFmpeg. Install the pinned tools in Settings.")
        } else {
            None
        }
    }

    fn gpu_encoder_ready(&self, codec: HardwareCodec) -> bool {
        self.hardware.encoder_for(&codec).is_some()
    }

    fn codec_options(&self) -> (Vec<ChoiceOption>, usize) {
        let mut options = CODECS
            .iter()
            .map(|(_, label)| ChoiceOption::new(*label).group("Download as"))
            .collect::<Vec<_>>();
        let vendors = {
            let mut vendors = Vec::new();
            for encoder in self
                .hardware
                .encoders
                .iter()
                .filter(|encoder| encoder.available)
            {
                let label = encoder.provider.label();
                if !vendors.contains(&label) {
                    vendors.push(label);
                }
            }
            vendors.join(", ")
        };
        let group = if vendors.is_empty() {
            "Convert on GPU".to_string()
        } else {
            format!("Convert on GPU ({vendors})")
        };
        let unavailable = if self.is_playlist() {
            "one video at a time"
        } else if self.hardware.status == "checking" {
            "checking the GPU"
        } else {
            "not supported here"
        };
        for codec in HardwareCodec::ALL {
            let name = match codec {
                HardwareCodec::H264 => "H.264 on GPU",
                HardwareCodec::Hevc => "HEVC on GPU, smaller files",
                HardwareCodec::Av1 => "AV1 on GPU",
            };
            let ready = self.gpu_encoder_ready(codec) && !self.is_playlist();
            let label = if ready {
                name.to_string()
            } else {
                format!("{name} ({unavailable})")
            };
            options.push(
                ChoiceOption::new(label)
                    .disabled(!ready)
                    .group(group.clone()),
            );
        }
        let selected = match (
            &self.options.video_conversion,
            self.options.codec_preference,
        ) {
            (Some(conversion), _) => {
                CODECS.len()
                    + HardwareCodec::ALL
                        .iter()
                        .position(|codec| *codec == conversion.codec)
                        .unwrap_or(0)
            }
            (None, preference) => CODECS
                .iter()
                .position(|(value, _)| *value == preference)
                .unwrap_or(0),
        };
        (options, selected)
    }

    fn filename_presets(&self) -> Vec<(&'static str, &'static str, Option<&'static str>)> {
        FILENAME_PRESETS
            .iter()
            .copied()
            .filter(|(key, _, _)| *key != "index" || self.is_playlist())
            .collect()
    }

    fn sponsor_categories(&self) -> Vec<SponsorCategory> {
        SponsorCategory::ALL.to_vec()
    }

    /// Asks before downloading a whole collection with no item limit.
    pub fn needs_playlist_confirmation(&self) -> Option<String> {
        let probe = self.probe.as_ref()?;
        if !probe.is_playlist
            || self.playlist_items.trimmed().is_some()
            || self.max_downloads.trimmed().is_some()
        {
            return None;
        }
        Some(match probe.playlist_count {
            Some(count) => format!("This will download all {count} items. Continue?"),
            None => "This will download the full collection. Continue?".into(),
        })
    }

    /// The download request for the current choices, or the first problem.
    pub fn build_request(&self) -> Result<DownloadRequest, String> {
        let probe = self.probe.as_ref().ok_or("Read the link first.")?;
        let destination = expand_home(self.destination.value());
        if self.destination.trimmed().is_none() || !destination.is_absolute() {
            return Err("Choose an absolute download folder, such as ~/Downloads.".into());
        }
        let clip = if self.clip_on {
            if let Some(reason) = self.clip_disabled_reason() {
                return Err(reason.into());
            }
            Some(validate_clip(
                self.clip_start.value(),
                self.clip_end.value(),
                self.clip_precise,
                probe.duration_seconds,
            )?)
        } else {
            None
        };
        let custom_format = if self.picking() {
            let parts = [self.video_stream.clone(), self.audio_stream.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            if parts.is_empty() {
                return Err("Choose a video or audio stream.".into());
            }
            Some(parts.join("+"))
        } else {
            None
        };
        if self
            .options
            .sponsorblock
            .as_ref()
            .is_some_and(|sponsorblock| sponsorblock.categories.is_empty())
        {
            return Err("Choose at least one kind of SponsorBlock segment.".into());
        }
        let template = self
            .filename_presets()
            .into_iter()
            .find(|(key, _, _)| *key == self.filename_preset)
            .and_then(|(_, _, template)| template)
            .map(str::to_owned)
            .unwrap_or_else(|| self.settings_template.clone());
        let mut options = DownloadOptions {
            custom_format,
            no_playlist: is_video_in_playlist(self.link.value().trim()) && !self.whole_playlist,
            video_conversion: if probe.is_playlist {
                None
            } else {
                self.options.video_conversion.clone()
            },
            clip,
            subtitle_languages: self.chosen_languages(),
            remove_chapters: self.remove_chapters.trimmed(),
            date_after: parse_date(&self.date_after, "earliest upload")?,
            date_before: parse_date(&self.date_before, "latest upload")?,
            min_filesize_mb: parse_count(&self.min_filesize, "the minimum size")?,
            max_filesize_mb: parse_count(&self.max_filesize, "the maximum size")?,
            max_downloads: parse_count(&self.max_downloads, "the item limit")?,
            referer: self.referer.trimmed(),
            user_agent: self.user_agent.trimmed(),
            ..self.options.clone()
        };
        if probe.is_playlist {
            options.playlist_items = self
                .playlist_items
                .trimmed()
                .map(|items| items.split_whitespace().collect::<String>());
        } else {
            options.playlist_items = None;
            options.playlist_order = None;
            options.date_after = None;
            options.date_before = None;
            options.min_filesize_mb = None;
            options.max_filesize_mb = None;
            options.min_duration_seconds = None;
            options.skip_live = false;
            options.max_downloads = None;
            options.sleep_interval = None;
        }
        if options.mode != MediaMode::Audio || options.audio_format == AudioFormat::Best {
            options.keep_video = false;
        }
        Ok(DownloadRequest {
            url: probe.url.clone(),
            destination: destination.to_string_lossy().into_owned(),
            filename_template: template,
            is_playlist: probe.is_playlist,
            options,
        })
    }

    fn header_rows(&self, rows: &mut Vec<Row<Field>>, theme: &Theme) {
        if !self.yt_dlp_ready {
            rows.push(Row::note(
                "",
                Line::from(Span::styled(
                    "yt-dlp isn't installed. Press Esc, then I to install the pinned tools.",
                    theme.warning(),
                )),
            ));
        }
        if self.analyzing {
            rows.push(Row::note(
                "",
                Line::from(Span::styled("Reading link…", theme.faint())),
            ));
        }
        if let Some(error) = &self.analyze_error {
            rows.push(Row::note(
                "",
                Line::from(Span::styled("Couldn't read that link", theme.error())),
            ));
            for line in form::wrap(error, 70) {
                rows.push(Row::note("", Line::from(Span::styled(line, theme.error()))));
            }
        }
        let Some(probe) = &self.probe else {
            return;
        };
        rows.push(Row::note(
            "",
            Line::from(Span::styled(probe.title.clone(), theme.strong())),
        ));
        let source = match &probe.creator {
            Some(creator) => format!("{creator} on {}", hostname(&probe.url)),
            None => hostname(&probe.url),
        };
        rows.push(Row::note(
            "",
            Line::from(Span::styled(source, theme.faint())),
        ));
        let mut tags = Vec::new();
        if let Some(duration) = probe.duration_seconds {
            tags.push(format_duration(duration));
        }
        if probe.is_playlist {
            tags.push(match probe.playlist_count {
                Some(count) => format!("Playlist of {count}"),
                None => "Playlist".into(),
            });
        }
        if probe.is_live {
            tags.push("Live".into());
        }
        let max_height = self.max_height();
        if max_height > 0 {
            tags.push(format!("{max_height}p max"));
        }
        if !tags.is_empty() {
            rows.push(Row::note(
                "",
                Line::from(Span::styled(tags.join(" · "), theme.faint())),
            ));
        }
        for warning in &probe.warnings {
            for line in form::wrap(warning, 70) {
                rows.push(Row::note(
                    "",
                    Line::from(Span::styled(line, theme.warning())),
                ));
            }
        }
    }

    fn video_rows(&self, rows: &mut Vec<Row<Field>>) {
        let picking = self.picking();
        let (video, audio) = self.streams();
        let mut quality_options = QUALITIES
            .iter()
            .map(|(_, label)| ChoiceOption::new(*label))
            .collect::<Vec<_>>();
        quality_options.push(
            ChoiceOption::new(if self.probe.is_some() {
                "Pick exact streams…"
            } else {
                "Pick exact streams (read the link first)"
            })
            .disabled(video.len() + audio.len() == 0),
        );
        let selected = if picking {
            QUALITIES.len()
        } else {
            QUALITIES
                .iter()
                .position(|(value, _)| *value == self.options.quality)
                .unwrap_or(0)
        };
        let max_height = self.max_height();
        let capped = self
            .options
            .quality
            .parse::<u32>()
            .is_ok_and(|requested| max_height > 0 && max_height < requested);
        rows.push(
            Row::new(Field::Quality, "Quality", choice(quality_options, selected))
                .help_if(
                    !picking && capped,
                    format!("This source tops out at {max_height}p, so that is what you get."),
                )
                .help_if(
                    !picking && self.options.quality == "best" && !self.ffmpeg_ready,
                    "Saves a single file until FFmpeg is installed.",
                ),
        );
        if picking {
            let mut video_options = vec![ChoiceOption::new("None, audio only")];
            video_options.extend(
                video
                    .iter()
                    .map(|format| ChoiceOption::new(stream_label(format))),
            );
            let video_selected = self
                .video_stream
                .as_ref()
                .and_then(|id| video.iter().position(|format| &format.format_id == id))
                .map_or(0, |index| index + 1);
            rows.push(
                Row::new(
                    Field::VideoStream,
                    "Video stream",
                    choice(video_options, video_selected),
                )
                .indent(),
            );
            let mut audio_options = vec![ChoiceOption::new(
                "None, or the one inside the video stream",
            )];
            audio_options.extend(
                audio
                    .iter()
                    .map(|format| ChoiceOption::new(stream_label(format))),
            );
            let audio_selected = self
                .audio_stream
                .as_ref()
                .and_then(|id| audio.iter().position(|format| &format.format_id == id))
                .map_or(0, |index| index + 1);
            rows.push(
                Row::new(
                    Field::AudioStream,
                    "Audio stream",
                    choice(audio_options, audio_selected),
                )
                .indent(),
            );
        }
        let container = CONTAINERS
            .iter()
            .position(|(value, _)| *value == self.options.container)
            .unwrap_or(0);
        rows.push(Row::new(
            Field::Container,
            "Container",
            choices(&CONTAINERS.map(|(_, label)| label), container),
        ));
        if !picking {
            let (options, selected) = self.codec_options();
            rows.push(Row::new(
                Field::VideoCodec,
                "Video codec",
                choice(options, selected),
            ));
            if let Some(conversion) = self.options.video_conversion.as_ref() {
                let decode_ready = self
                    .hardware
                    .encoder_for(&conversion.codec)
                    .is_some_and(|encoder| encoder.decode_available);
                let qualities = (1..=51)
                    .map(|value| {
                        ChoiceOption::new(match value {
                            23 => "23 (balanced)".to_string(),
                            value => value.to_string(),
                        })
                    })
                    .collect();
                rows.push(
                    Row::new(
                        Field::GpuQuality,
                        "GPU quality",
                        choice(qualities, conversion.quality as usize - 1),
                    )
                    .indent()
                    .help("Re-encodes to MKV after downloading. Lower numbers keep more detail and make larger files. The original is removed only once the new file is complete."),
                );
                rows.push(
                    Row::new(
                        Field::GpuDecode,
                        "",
                        toggle("Hardware decoding", conversion.use_hardware_decode),
                    )
                    .indent()
                    .disabled(!decode_ready)
                    .help(if decode_ready {
                        "Off: the CPU reads the source and the GPU encodes. On: the GPU does both, which is faster but fails on some sources."
                    } else {
                        "This GPU can encode but not decode here, so the CPU reads the source."
                    }),
                );
            }
        }
    }

    fn audio_rows(&self, rows: &mut Vec<Row<Field>>) {
        let format = AUDIO_FORMATS
            .iter()
            .position(|(value, _)| *value == self.options.audio_format)
            .unwrap_or(0);
        rows.push(Row::new(
            Field::AudioFormat,
            "Audio format",
            choices(&AUDIO_FORMATS.map(|(_, label)| label), format),
        ));
        let lossless = !self.options.audio_format.supports_bitrate();
        let quality = AUDIO_QUALITIES
            .iter()
            .position(|(value, _)| *value == self.options.audio_quality)
            .unwrap_or(0);
        rows.push(
            Row::new(
                Field::Bitrate,
                "Bitrate",
                choices(&AUDIO_QUALITIES.map(|(_, label)| label), quality),
            )
            .disabled(lossless)
            .help_if(
                lossless,
                if self.options.audio_format == AudioFormat::Best {
                    "The original audio keeps its quality."
                } else {
                    "Lossless formats have no target bitrate."
                },
            ),
        );
    }

    fn clip_rows(&self, rows: &mut Vec<Row<Field>>, theme: &Theme) {
        let Some(probe) = &self.probe else {
            return;
        };
        let reason = self.clip_disabled_reason();
        rows.push(
            Row::new(
                Field::Clip,
                "Clip",
                toggle("Download only a selected timeframe", self.clip_on),
            )
            .disabled(reason.is_some())
            .help(reason.unwrap_or("Save just the part you need.")),
        );
        if !self.clip_on {
            return;
        }
        rows.push(
            Row::new(Field::ClipStart, "Start", text(&self.clip_start, "0:00"))
                .indent()
                .hint("Seconds, MM:SS, or HH:MM:SS"),
        );
        rows.push(
            Row::new(Field::ClipEnd, "End", text(&self.clip_end, "0:30"))
                .indent()
                .hint("Seconds, MM:SS, or HH:MM:SS"),
        );
        rows.push(
            Row::new(
                Field::ClipCut,
                "Cut",
                choices(
                    &[
                        "Accurate, re-encodes at the cut points",
                        "Fast, cuts at the nearest keyframes",
                    ],
                    usize::from(!self.clip_precise),
                ),
            )
            .indent(),
        );
        match validate_clip(
            self.clip_start.value(),
            self.clip_end.value(),
            self.clip_precise,
            probe.duration_seconds,
        ) {
            Ok(clip) => {
                let selected = if clip.duration_seconds < 60.0 {
                    format!(
                        "{} seconds selected",
                        (clip.duration_seconds * 1000.0).round() / 1000.0
                    )
                } else {
                    format!("{} selected", format_timecode(clip.duration_seconds))
                };
                let timeline = probe
                    .duration_seconds
                    .filter(|duration| *duration > 0.0)
                    .map(|duration| {
                        let width = 30.0;
                        let start = (clip.start_seconds / duration * width).round() as usize;
                        let end = ((clip.start_seconds + clip.duration_seconds) / duration * width)
                            .round()
                            .max(start as f64 + 1.0) as usize;
                        let end = end.min(width as usize);
                        vec![
                            Span::styled("─".repeat(start), theme.faint()),
                            Span::styled("━".repeat(end.saturating_sub(start)), theme.accent()),
                            Span::styled("─".repeat(width as usize - end), theme.faint()),
                            Span::raw("  "),
                        ]
                    });
                let mut spans = timeline.unwrap_or_default();
                spans.push(Span::styled(selected, theme.faint()));
                rows.push(Row::note("", Line::from(spans)));
            }
            Err(message) => rows.push(Row::note(
                "",
                Line::from(Span::styled(message, theme.error())),
            )),
        }
    }

    fn advanced_rows(&self, rows: &mut Vec<Row<Field>>) {
        let options = &self.options;
        if options.mode == MediaMode::Audio && options.audio_format != AudioFormat::Best {
            rows.push(Row::new(
                Field::KeepVideo,
                "Audio",
                toggle("Keep the original video file too", options.keep_video),
            ));
        }

        rows.push(Row::new(
            Field::WriteSubs,
            "Subtitles",
            toggle("Download available subtitles", options.write_subtitles),
        ));
        rows.push(Row::new(
            Field::WriteAutoSubs,
            "",
            toggle(
                "Include automatic captions",
                options.write_automatic_subtitles,
            ),
        ));
        if options.write_subtitles || options.write_automatic_subtitles {
            let chosen = self.chosen_languages();
            let offered = self.subtitle_choices();
            if !offered.is_empty() {
                rows.push(
                    Row::new(
                        Field::SubtitlePick,
                        "",
                        Control::Multi {
                            items: offered
                                .iter()
                                .map(|language| MultiItem {
                                    label: language_name(language),
                                    checked: chosen.contains(language),
                                    disabled: false,
                                })
                                .collect(),
                            empty: "Choose from this video's languages".into(),
                        },
                    )
                    .indent(),
                );
            }
            rows.push(
                Row::new(
                    Field::SubtitleLanguages,
                    "Languages",
                    text(&self.subtitle_languages, "en.*, ja (empty means English)"),
                )
                .indent()
                .hint("Comma-separated codes or patterns, such as en.* or all"),
            );
            let format = SUBTITLE_FORMATS
                .iter()
                .position(|(value, _)| *value == options.subtitle_format)
                .unwrap_or(0);
            rows.push(
                Row::new(
                    Field::SubtitleFormat,
                    "Save as",
                    choices(&SUBTITLE_FORMATS.map(|(_, label)| label), format),
                )
                .indent(),
            );
            rows.push(
                Row::new(
                    Field::EmbedSubs,
                    "",
                    toggle("Embed subtitles in the file", options.embed_subtitles),
                )
                .indent(),
            );
        }

        rows.push(Row::new(
            Field::EmbedChapters,
            "Chapters",
            toggle("Embed chapter markers", options.embed_chapters),
        ));
        rows.push(Row::new(
            Field::SplitChapters,
            "",
            toggle(
                "Also save each chapter as its own file",
                options.split_chapters,
            ),
        ));
        rows.push(Row::new(
            Field::RemoveChapters,
            "Cut chapters",
            text(&self.remove_chapters, "Chapters whose title contains…"),
        ));

        let mode = match options
            .sponsorblock
            .as_ref()
            .map(|sponsorblock| sponsorblock.mode)
        {
            None => 0,
            Some(SponsorBlockMode::Mark) => 1,
            Some(SponsorBlockMode::Remove) => 2,
        };
        rows.push(Row::new(
            Field::SponsorBlock,
            "SponsorBlock",
            choices(
                &[
                    "Off",
                    "Mark segments as chapters",
                    "Cut segments out of the file",
                ],
                mode,
            ),
        ));
        if let Some(sponsorblock) = options.sponsorblock.as_ref() {
            let removing = sponsorblock.mode == SponsorBlockMode::Remove;
            let row = Row::new(
                Field::SponsorCategories,
                "Segments",
                Control::Multi {
                    items: self
                        .sponsor_categories()
                        .into_iter()
                        .map(|category| MultiItem {
                            label: category.label().into(),
                            checked: sponsorblock.categories.contains(&category),
                            disabled: removing && !category.removable(),
                        })
                        .collect(),
                    empty: "None chosen".into(),
                },
            )
            .indent();
            rows.push(if sponsorblock.categories.is_empty() {
                row.error("Choose at least one kind of segment.")
            } else {
                row
            });
        }

        rows.push(Row::new(
            Field::WriteThumbnail,
            "Extra files",
            toggle("Thumbnail image", options.write_thumbnail),
        ));
        if options.write_thumbnail {
            let format = THUMBNAIL_FORMATS
                .iter()
                .position(|(value, _)| Some(*value) == options.thumbnail_format)
                .unwrap_or(0);
            rows.push(
                Row::new(
                    Field::ThumbnailFormat,
                    "Image format",
                    choices(&THUMBNAIL_FORMATS.map(|(_, label)| label), format),
                )
                .indent(),
            );
        }
        rows.push(Row::new(
            Field::WriteDescription,
            "",
            toggle("Video description (text)", options.write_description),
        ));
        rows.push(Row::new(
            Field::WriteInfoJson,
            "",
            toggle("Full metadata (JSON)", options.write_info_json),
        ));
        rows.push(Row::new(
            Field::WriteComments,
            "",
            toggle(
                "Comments (saved in the metadata JSON)",
                options.write_comments,
            ),
        ));
        rows.push(Row::new(
            Field::WriteLink,
            "",
            toggle("Shortcut to the source page", options.write_link),
        ));

        let presets = self.filename_presets();
        let preset = presets
            .iter()
            .position(|(key, _, _)| *key == self.filename_preset)
            .unwrap_or(0);
        rows.push(Row::new(
            Field::FilenamePreset,
            "File name",
            choices(
                &presets
                    .iter()
                    .map(|(_, label, _)| *label)
                    .collect::<Vec<_>>(),
                preset,
            ),
        ));
        rows.push(Row::new(
            Field::EmbedMetadata,
            "",
            toggle("Embed title and metadata", options.embed_metadata),
        ));
        rows.push(Row::new(
            Field::EmbedThumbnail,
            "",
            toggle("Embed thumbnail as cover art", options.embed_thumbnail),
        ));
        rows.push(Row::new(
            Field::RestrictFilenames,
            "",
            toggle(
                "Simple file names (no spaces or special characters)",
                options.restrict_filenames,
            ),
        ));
        rows.push(Row::new(
            Field::WindowsFilenames,
            "",
            toggle("Windows-safe file names", options.windows_filenames),
        ));
        let trim = TRIM_LENGTHS
            .iter()
            .position(|(value, _)| *value == options.trim_filenames)
            .unwrap_or(0);
        rows.push(Row::new(
            Field::TrimFilenames,
            "Shorten names to",
            choices(&TRIM_LENGTHS.map(|(_, label)| label), trim),
        ));
        rows.push(Row::new(
            Field::ForceOverwrites,
            "",
            toggle(
                "Replace a file that already exists",
                options.force_overwrites,
            ),
        ));

        if self.is_playlist() {
            let reverse = options.playlist_order == Some(PlaylistOrder::Reverse);
            rows.push(
                Row::new(
                    Field::PlaylistItems,
                    "Playlist items",
                    text(&self.playlist_items, "All, or 1:10 and 1,3,8"),
                )
                .disabled(reverse)
                .help_if(reverse, "Reversed order downloads every item.")
                .hint("Numbers, ranges like 1:10, or steps like 1:20:2"),
            );
            let order = match options.playlist_order {
                None => 0,
                Some(PlaylistOrder::Reverse) => 1,
                Some(PlaylistOrder::Random) => 2,
            };
            rows.push(Row::new(
                Field::PlaylistOrder,
                "Order",
                choices(
                    &["As listed", "Reversed, oldest first on channels", "Random"],
                    order,
                ),
            ));
            rows.push(Row::new(
                Field::DateAfter,
                "Uploaded from",
                text(&self.date_after, "YYYY-MM-DD, any"),
            ));
            rows.push(Row::new(
                Field::DateBefore,
                "Uploaded until",
                text(&self.date_before, "YYYY-MM-DD, any"),
            ));
            rows.push(Row::new(
                Field::MinFilesize,
                "Smallest file",
                text(&self.min_filesize, "MB, any"),
            ));
            rows.push(Row::new(
                Field::MaxFilesize,
                "Largest file",
                text(&self.max_filesize, "MB, any"),
            ));
            let minimum = MIN_DURATIONS
                .iter()
                .position(|(value, _)| *value == options.min_duration_seconds)
                .unwrap_or(0);
            rows.push(Row::new(
                Field::MinDuration,
                "Skip shorter than",
                choices(&MIN_DURATIONS.map(|(_, label)| label), minimum),
            ));
            rows.push(Row::new(
                Field::SkipLive,
                "",
                toggle("Skip live streams and premieres", options.skip_live),
            ));
            rows.push(Row::new(
                Field::MaxDownloads,
                "Stop after",
                text(&self.max_downloads, "All items"),
            ));
            let sleep = SLEEP_INTERVALS
                .iter()
                .position(|(value, _)| *value == options.sleep_interval)
                .unwrap_or(0);
            rows.push(Row::new(
                Field::SleepInterval,
                "Pause between",
                choices(&SLEEP_INTERVALS.map(|(_, label)| label), sleep),
            ));
        }

        let connections = CONNECTIONS
            .iter()
            .position(|(value, _)| *value == options.concurrent_fragments)
            .unwrap_or(0);
        rows.push(Row::new(
            Field::Connections,
            "Connections",
            choices(&CONNECTIONS.map(|(_, label)| label), connections),
        ));
        rows.push(Row::new(
            Field::Referer,
            "Referer",
            text(&self.referer, "Page the video is embedded on, if required"),
        ));
        rows.push(Row::new(
            Field::UserAgent,
            "User agent",
            text(&self.user_agent, "Automatic"),
        ));
    }

    /// All rows, using `theme` for the read-only notes.
    fn rows_themed(&self, theme: &Theme) -> Vec<Row<Field>> {
        let mut rows = vec![
            Row::new(
                Field::Link,
                "Link",
                text(&self.link, "Paste a video, playlist, or channel link"),
            )
            .hint("Enter reads the link"),
        ];
        self.header_rows(&mut rows, theme);
        if is_video_in_playlist(self.link.value().trim()) {
            rows.push(Row::new(
                Field::Scope,
                "This link",
                choices(
                    &["Just this video", "Whole playlist"],
                    usize::from(self.whole_playlist),
                ),
            ));
        }
        rows.push(Row::new(
            Field::Kind,
            "Download",
            choices(
                &["Video", "Audio only"],
                usize::from(self.options.mode == MediaMode::Audio),
            ),
        ));
        if self.options.mode == MediaMode::Audio {
            self.audio_rows(&mut rows);
        } else {
            self.video_rows(&mut rows);
        }
        let languages = self.audio_languages();
        if languages.len() > 1 {
            let mut options = vec![ChoiceOption::new("Original language")];
            options.extend(
                languages
                    .iter()
                    .map(|language| ChoiceOption::new(language_name(language))),
            );
            let selected = self
                .options
                .audio_language
                .as_ref()
                .and_then(|chosen| languages.iter().position(|language| language == chosen))
                .map_or(0, |index| index + 1);
            rows.push(Row::new(
                Field::AudioTrack,
                "Audio track",
                choice(options, selected),
            ));
        }
        if self.probe.as_ref().is_some_and(|probe| probe.is_live) {
            rows.push(Row::new(
                Field::LiveFromStart,
                "Live stream",
                toggle(
                    "Record from the beginning, not from now",
                    self.options.live_from_start,
                ),
            ));
        }
        self.clip_rows(&mut rows, theme);

        let destination = expand_home(self.destination.value());
        let mut destination_row = Row::new(
            Field::Destination,
            "Save to",
            Control::Text {
                input: self.destination.clone(),
                placeholder: "~/Downloads".into(),
                suggestion: self.destination_suggestion.clone(),
            },
        )
        .hint("→ at the end accepts the suggested folder");
        if self.destination.trimmed().is_some() && !destination.is_absolute() {
            destination_row = destination_row.error("Use an absolute folder, such as ~/Downloads.");
        } else if self.destination.trimmed().is_some() && !destination.is_dir() {
            destination_row =
                destination_row.help("This folder doesn't exist yet. It will be created.");
        }
        rows.push(destination_row);

        rows.push(Row::new(
            Field::Advanced,
            "Advanced",
            button(if self.advanced {
                "Hide advanced options"
            } else {
                "Show advanced options"
            }),
        ));
        if self.advanced {
            self.advanced_rows(&mut rows);
        }
        if self.probe.is_some() {
            rows.push(Row::section(""));
            rows.push(
                Row::new(
                    Field::Queue,
                    "",
                    button(if self.submitting {
                        "Adding…"
                    } else {
                        "Add to queue"
                    }),
                )
                .hint("Ctrl+Q from anywhere in this form"),
            );
            rows.push(
                Row::new(
                    Field::Download,
                    "",
                    button(if self.submitting {
                        "Starting…"
                    } else {
                        "Download"
                    }),
                )
                .hint("Ctrl+D from anywhere in this form"),
            );
        } else {
            rows.push(Row::section(""));
            rows.push(
                Row::new(
                    Field::Analyze,
                    "",
                    button(if self.analyzing {
                        "Reading link…"
                    } else {
                        "Analyze"
                    }),
                )
                .disabled(
                    self.link.value().trim().is_empty() || !self.yt_dlp_ready || self.analyzing,
                ),
            );
        }
        rows
    }
}

impl FormModel for NewDownload {
    type Id = Field;

    fn rows(&self) -> Vec<Row<Field>> {
        self.rows_themed(&self.theme)
    }

    fn form(&mut self) -> &mut FormState<Field> {
        &mut self.form
    }

    fn choose(&mut self, id: Field, index: usize) {
        let options = &mut self.options;
        match id {
            Field::Scope => {
                let whole = index == 1;
                if whole != self.whole_playlist {
                    self.whole_playlist = whole;
                    if self.probe.is_some() || self.analyze_error.is_some() {
                        self.probe = None;
                        self.analyze_error = None;
                    }
                }
            }
            Field::Kind => {
                let audio = index == 1;
                if audio {
                    if options.mode == MediaMode::Custom {
                        options.quality = "best".into();
                    }
                    options.mode = MediaMode::Audio;
                    options.video_conversion = None;
                } else if options.mode == MediaMode::Audio {
                    options.mode = MediaMode::Video;
                }
            }
            Field::Quality => {
                if index >= QUALITIES.len() {
                    options.mode = MediaMode::Custom;
                    options.video_conversion = None;
                    let (video, audio) = self.streams();
                    let first_video = video.first().map(|format| format.format_id.clone());
                    let first_audio = audio.first().map(|format| format.format_id.clone());
                    if self.video_stream.is_none() {
                        self.video_stream = first_video;
                    }
                    if self.audio_stream.is_none() {
                        self.audio_stream = first_audio;
                    }
                } else {
                    options.mode = MediaMode::Video;
                    options.quality = QUALITIES[index].0.into();
                }
            }
            Field::VideoStream => {
                let (video, _) = self.streams();
                self.video_stream = index
                    .checked_sub(1)
                    .and_then(|index| video.get(index))
                    .map(|format| format.format_id.clone());
            }
            Field::AudioStream => {
                let (_, audio) = self.streams();
                self.audio_stream = index
                    .checked_sub(1)
                    .and_then(|index| audio.get(index))
                    .map(|format| format.format_id.clone());
            }
            Field::Container => options.container = CONTAINERS[index].0,
            Field::VideoCodec => {
                if let Some(codec) = index
                    .checked_sub(CODECS.len())
                    .and_then(|index| HardwareCodec::ALL.get(index))
                {
                    let decode = options
                        .video_conversion
                        .as_ref()
                        .is_some_and(|conversion| conversion.use_hardware_decode)
                        && self
                            .hardware
                            .encoder_for(codec)
                            .is_some_and(|encoder| encoder.decode_available);
                    let quality = options
                        .video_conversion
                        .as_ref()
                        .map_or(23, |conversion| conversion.quality);
                    options.codec_preference = None;
                    options.video_conversion = Some(VideoConversionOptions {
                        codec: *codec,
                        quality,
                        use_hardware_decode: decode,
                    });
                } else {
                    options.codec_preference = CODECS[index.min(CODECS.len() - 1)].0;
                    options.video_conversion = None;
                }
            }
            Field::GpuQuality => {
                if let Some(conversion) = options.video_conversion.as_mut() {
                    conversion.quality = (index + 1) as u8;
                }
            }
            Field::AudioFormat => {
                options.audio_format = AUDIO_FORMATS[index].0;
                if !options.audio_format.supports_bitrate() {
                    options.audio_quality = AudioQuality::Best;
                }
            }
            Field::Bitrate => options.audio_quality = AUDIO_QUALITIES[index].0,
            Field::AudioTrack => {
                let languages = self.audio_languages();
                self.options.audio_language = index
                    .checked_sub(1)
                    .and_then(|index| languages.get(index).cloned());
            }
            Field::ClipCut => self.clip_precise = index == 0,
            Field::SubtitleFormat => options.subtitle_format = SUBTITLE_FORMATS[index].0,
            Field::SponsorBlock => {
                let categories = options.sponsorblock.as_ref().map_or_else(
                    || vec![SponsorCategory::Sponsor],
                    |sponsorblock| sponsorblock.categories.clone(),
                );
                options.sponsorblock = match index {
                    1 => Some(SponsorBlockOptions {
                        mode: SponsorBlockMode::Mark,
                        categories,
                    }),
                    2 => Some(SponsorBlockOptions {
                        mode: SponsorBlockMode::Remove,
                        categories: categories
                            .into_iter()
                            .filter(|category| category.removable())
                            .collect(),
                    }),
                    _ => None,
                };
            }
            Field::ThumbnailFormat => {
                options.thumbnail_format = match THUMBNAIL_FORMATS[index].0 {
                    ThumbnailFormat::Jpg => None,
                    format => Some(format),
                };
            }
            Field::FilenamePreset => {
                if let Some((key, _, _)) = self.filename_presets().get(index) {
                    self.filename_preset = key;
                }
            }
            Field::TrimFilenames => options.trim_filenames = TRIM_LENGTHS[index].0,
            Field::PlaylistOrder => {
                options.playlist_order = match index {
                    1 => Some(PlaylistOrder::Reverse),
                    2 => Some(PlaylistOrder::Random),
                    _ => None,
                };
                if options.playlist_order == Some(PlaylistOrder::Reverse) {
                    self.playlist_items.set("");
                }
            }
            Field::MinDuration => options.min_duration_seconds = MIN_DURATIONS[index].0,
            Field::SleepInterval => options.sleep_interval = SLEEP_INTERVALS[index].0,
            Field::Connections => options.concurrent_fragments = CONNECTIONS[index].0,
            _ => {}
        }
    }

    fn toggle(&mut self, id: Field) {
        let options = &mut self.options;
        match id {
            Field::GpuDecode => {
                if let Some(conversion) = options.video_conversion.as_mut() {
                    conversion.use_hardware_decode = !conversion.use_hardware_decode;
                }
            }
            Field::LiveFromStart => options.live_from_start = !options.live_from_start,
            Field::Clip => {
                self.clip_on = !self.clip_on;
                if self.clip_on {
                    let end = self
                        .probe
                        .as_ref()
                        .and_then(|probe| probe.duration_seconds)
                        .filter(|duration| *duration > 0.0)
                        .unwrap_or(30.0);
                    self.clip_start.set("0:00");
                    self.clip_end.set(format_timecode(end));
                    self.clip_precise = true;
                }
            }
            Field::KeepVideo => options.keep_video = !options.keep_video,
            Field::WriteSubs => options.write_subtitles = !options.write_subtitles,
            Field::WriteAutoSubs => {
                options.write_automatic_subtitles = !options.write_automatic_subtitles
            }
            Field::EmbedSubs => options.embed_subtitles = !options.embed_subtitles,
            Field::EmbedChapters => options.embed_chapters = !options.embed_chapters,
            Field::SplitChapters => options.split_chapters = !options.split_chapters,
            Field::WriteThumbnail => options.write_thumbnail = !options.write_thumbnail,
            Field::WriteDescription => options.write_description = !options.write_description,
            Field::WriteInfoJson => options.write_info_json = !options.write_info_json,
            Field::WriteComments => options.write_comments = !options.write_comments,
            Field::WriteLink => options.write_link = !options.write_link,
            Field::EmbedMetadata => options.embed_metadata = !options.embed_metadata,
            Field::EmbedThumbnail => options.embed_thumbnail = !options.embed_thumbnail,
            Field::RestrictFilenames => options.restrict_filenames = !options.restrict_filenames,
            Field::WindowsFilenames => options.windows_filenames = !options.windows_filenames,
            Field::ForceOverwrites => options.force_overwrites = !options.force_overwrites,
            Field::SkipLive => options.skip_live = !options.skip_live,
            _ => {}
        }
    }

    fn input(&mut self, id: Field) -> Option<&mut TextInput> {
        Some(match id {
            Field::Link => &mut self.link,
            Field::ClipStart => &mut self.clip_start,
            Field::ClipEnd => &mut self.clip_end,
            Field::Destination => &mut self.destination,
            Field::SubtitleLanguages => &mut self.subtitle_languages,
            Field::RemoveChapters => &mut self.remove_chapters,
            Field::PlaylistItems => &mut self.playlist_items,
            Field::DateAfter => &mut self.date_after,
            Field::DateBefore => &mut self.date_before,
            Field::MinFilesize => &mut self.min_filesize,
            Field::MaxFilesize => &mut self.max_filesize,
            Field::MaxDownloads => &mut self.max_downloads,
            Field::Referer => &mut self.referer,
            Field::UserAgent => &mut self.user_agent,
            _ => return None,
        })
    }

    fn set_item(&mut self, id: Field, index: usize, checked: bool) {
        match id {
            Field::SubtitlePick => {
                let Some(language) = self.subtitle_choices().get(index).cloned() else {
                    return;
                };
                let mut chosen = self.chosen_languages();
                chosen.retain(|existing| *existing != language);
                if checked {
                    chosen.push(language);
                }
                self.subtitle_languages.set(chosen.join(", "));
            }
            Field::SponsorCategories => {
                let category = self.sponsor_categories()[index];
                if let Some(sponsorblock) = self.options.sponsorblock.as_mut() {
                    sponsorblock
                        .categories
                        .retain(|existing| *existing != category);
                    if checked {
                        sponsorblock.categories.push(category);
                        let order = |category: &SponsorCategory| {
                            SponsorCategory::ALL
                                .iter()
                                .position(|item| item == category)
                        };
                        sponsorblock.categories.sort_by_key(order);
                    }
                }
            }
            _ => {}
        }
    }

    fn text_changed(&mut self, id: Field) {
        match id {
            Field::Link => {
                let collapsed = self
                    .link
                    .value()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if collapsed != self.link.value() {
                    self.link.set(collapsed);
                }
                if self.analyzing {
                    self.cancel_analysis();
                }
                self.probe = None;
                self.analyze_error = None;
                self.clip_on = false;
            }
            Field::Destination => {
                self.destination_suggestion = path_suggestion(self.destination.value(), false);
            }
            _ => {}
        }
        self.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SubtitleTrack;

    fn probe(playlist: bool) -> MediaProbe {
        let format =
            |id: &str, height: Option<u32>, video: &str, audio: &str, tbr: f64| MediaFormat {
                format_id: id.into(),
                extension: "mp4".into(),
                width: None,
                height,
                fps: Some(60.0),
                video_codec: Some(video.into()),
                audio_codec: Some(audio.into()),
                bitrate_kbps: Some(tbr),
                file_size: Some(2_100_000),
                note: None,
                hdr: false,
                language: (audio != "none").then(|| "en".into()),
            };
        MediaProbe {
            id: "abc".into(),
            url: "https://www.youtube.com/watch?v=abc".into(),
            title: "Fixture".into(),
            creator: Some("Uploader".into()),
            duration_seconds: Some(120.0),
            is_playlist: playlist,
            playlist_count: playlist.then_some(12),
            is_live: false,
            formats: vec![
                format("137", Some(1080), "avc1.640028", "none", 4000.0),
                format("22", Some(720), "avc1", "mp4a", 1500.0),
                format("140", None, "none", "mp4a.40.2", 128.0),
                format("251", None, "none", "opus", 160.0),
            ],
            subtitles: vec![
                SubtitleTrack {
                    language: "en".into(),
                    name: None,
                    extensions: vec!["vtt".into()],
                    automatic: false,
                },
                SubtitleTrack {
                    language: "ja".into(),
                    name: None,
                    extensions: vec!["vtt".into()],
                    automatic: true,
                },
            ],
            warnings: vec![],
        }
    }

    fn ready(playlist: bool) -> NewDownload {
        let settings = AppSettings {
            download_directory: std::env::temp_dir().to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        let mut form = NewDownload::new(
            &settings,
            true,
            true,
            HardwareAccelerationInfo::checking(),
            Theme::new("blue"),
        );
        let Effect::Analyze { .. } = form.set_link("https://www.youtube.com/watch?v=abc") else {
            panic!("pasting a link reads it");
        };
        let revision = form.revision;
        form.analyzed(revision, Ok(probe(playlist)));
        form
    }

    #[test]
    fn a_stale_analysis_result_is_ignored() {
        let mut form = ready(false);
        form.set_link("https://example.com/other");
        form.analyzed(1, Ok(probe(false)));
        assert!(form.probe.is_none());
        assert!(form.analyzing);
    }

    #[test]
    fn builds_the_default_video_request() {
        let form = ready(false);
        let request = form.build_request().unwrap();
        assert_eq!(request.url, "https://www.youtube.com/watch?v=abc");
        assert_eq!(request.options.mode, MediaMode::Video);
        assert_eq!(request.filename_template, DEFAULT_TEMPLATE);
        assert!(!request.is_playlist);
        assert!(request.options.embed_metadata);
    }

    #[test]
    fn picking_streams_builds_an_exact_selector() {
        let mut form = ready(false);
        form.choose(Field::Quality, QUALITIES.len());
        assert_eq!(form.options.mode, MediaMode::Custom);
        let request = form.build_request().unwrap();
        assert_eq!(request.options.custom_format.as_deref(), Some("137+251"));

        form.choose(Field::VideoStream, 0);
        form.choose(Field::AudioStream, 0);
        assert!(form.build_request().is_err());
    }

    #[test]
    fn audio_conversion_drops_unsupported_bitrates() {
        let mut form = ready(false);
        form.choose(Field::Kind, 1);
        form.choose(Field::AudioFormat, 1);
        form.choose(Field::Bitrate, 1);
        assert_eq!(form.options.audio_quality, AudioQuality::Kbps320);
        form.choose(Field::AudioFormat, 4);
        assert_eq!(form.options.audio_quality, AudioQuality::Best);
    }

    #[test]
    fn clip_defaults_to_the_whole_video_and_validates_edits() {
        let mut form = ready(false);
        form.toggle(Field::Clip);
        assert_eq!(form.clip_end.value(), "2:00");
        form.clip_start.set("0:30");
        let clip = form.build_request().unwrap().options.clip.unwrap();
        assert_eq!(clip.start_seconds, 30.0);
        assert_eq!(clip.duration_seconds, 90.0);
        assert!(clip.precise);
        form.clip_end.set("5:00");
        assert!(form.build_request().is_err());
    }

    #[test]
    fn playlists_need_confirmation_without_a_scope() {
        let mut form = ready(true);
        assert!(form.needs_playlist_confirmation().unwrap().contains("12"));
        form.playlist_items.set("1 : 5, 8");
        assert!(form.needs_playlist_confirmation().is_none());
        let request = form.build_request().unwrap();
        assert!(request.is_playlist);
        assert_eq!(request.options.playlist_items.as_deref(), Some("1:5,8"));
    }

    #[test]
    fn removal_mode_drops_categories_that_can_only_be_marked() {
        let mut form = ready(false);
        form.choose(Field::SponsorBlock, 1);
        form.set_item(Field::SponsorCategories, 9, true);
        form.set_item(Field::SponsorCategories, 6, true);
        let marked = &form.options.sponsorblock.as_ref().unwrap().categories;
        assert_eq!(
            marked,
            &[
                SponsorCategory::Sponsor,
                SponsorCategory::Hook,
                SponsorCategory::PoiHighlight
            ]
        );
        form.choose(Field::SponsorBlock, 2);
        let removed = &form.options.sponsorblock.as_ref().unwrap().categories;
        assert_eq!(removed, &[SponsorCategory::Sponsor, SponsorCategory::Hook]);
    }

    #[test]
    fn subtitle_picks_and_typed_languages_stay_in_sync() {
        let mut form = ready(false);
        form.toggle(Field::WriteSubs);
        assert_eq!(form.subtitle_choices(), ["en"]);
        form.toggle(Field::WriteAutoSubs);
        assert_eq!(form.subtitle_choices(), ["en", "ja"]);
        form.set_item(Field::SubtitlePick, 1, true);
        form.subtitle_languages.set("ja, en.*");
        assert_eq!(form.chosen_languages(), ["ja", "en.*"]);
        form.set_item(Field::SubtitlePick, 1, false);
        assert_eq!(form.subtitle_languages.value(), "en.*");
    }

    #[test]
    fn a_video_inside_a_playlist_defaults_to_just_that_video() {
        let settings = AppSettings::default();
        let mut form = NewDownload::new(
            &settings,
            true,
            true,
            HardwareAccelerationInfo::checking(),
            Theme::new("blue"),
        );
        let effect = form.set_link("https://www.youtube.com/watch?v=abc&list=PL1");
        assert_eq!(
            effect,
            Effect::Analyze {
                url: "https://www.youtube.com/watch?v=abc&list=PL1".into(),
                no_playlist: true,
            }
        );
    }

    #[test]
    fn expands_home_and_completes_paths() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_home("~/Videos"), home.join("Videos"));
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("Music")).unwrap();
        std::fs::create_dir(root.path().join(".hidden")).unwrap();
        std::fs::write(root.path().join("Mumble.txt"), b"").unwrap();
        let typed = format!("{}/Mu", root.path().display());
        assert_eq!(path_suggestion(&typed, false).as_deref(), Some("sic/"));
        assert_eq!(path_suggestion(&typed, true).as_deref(), Some("mble.txt"));
        let typed = format!("{}/.h", root.path().display());
        assert_eq!(path_suggestion(&typed, false).as_deref(), Some("idden/"));
    }
}
