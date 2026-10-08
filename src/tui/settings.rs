//! Settings: download defaults, network access, the pinned download engine,
//! GPU capability, and the accent color. Changes stay a draft until saved.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::{Line, Span};

use super::{
    form::{self, Control, FormModel, FormOutcome, FormState, Row, button, choices, toggle},
    format::format_bytes,
    new_download::{expand_home, path_suggestion},
    text_input::TextInput,
    theme::Theme,
};
use crate::{
    domain::{
        AppSettings, DependencyInfo, DependencyKind, HardwareAccelerationInfo, HardwareCodec,
        IpVersion, MediaMode,
    },
    integration::installer::{self, InstallProgress},
};

const QUALITIES: [(&str, &str); 6] = [
    ("best", "Best available"),
    ("2160", "Up to 2160p"),
    ("1440", "Up to 1440p"),
    ("1080", "Up to 1080p"),
    ("720", "Up to 720p"),
    ("single", "Best single file"),
];

/// `--cookies-from-browser` names accepted by yt-dlp 2026.08.19.
const BROWSERS: [(Option<&str>, &str); 10] = [
    (None, "None"),
    (Some("brave"), "Brave"),
    (Some("chrome"), "Chrome"),
    (Some("chromium"), "Chromium"),
    (Some("edge"), "Edge"),
    (Some("firefox"), "Firefox"),
    (Some("opera"), "Opera"),
    (Some("safari"), "Safari"),
    (Some("vivaldi"), "Vivaldi"),
    (Some("whale"), "Whale"),
];

/// `--impersonate` targets offered through curl_cffi in yt-dlp 2026.08.19.
const IMPERSONATE: [(Option<&str>, &str); 6] = [
    (None, "Off"),
    (Some("chrome"), "Chrome"),
    (Some("edge"), "Edge"),
    (Some("safari"), "Safari"),
    (Some("firefox"), "Firefox"),
    (Some("tor"), "Tor Browser"),
];

const TIMEOUTS: [(Option<u32>, &str); 4] = [
    (None, "Default (20 seconds)"),
    (Some(10), "10 seconds"),
    (Some(60), "1 minute"),
    (Some(180), "3 minutes"),
];

const SLEEP_REQUESTS: [(Option<u32>, &str); 4] = [
    (None, "No pause"),
    (Some(1), "1 second"),
    (Some(3), "3 seconds"),
    (Some(10), "10 seconds"),
];

const CHUNK_SIZES: [(Option<u32>, &str); 3] =
    [(None, "Off"), (Some(10), "10 MB"), (Some(50), "50 MB")];

const ACCENTS: [(&str, &str); 4] = [
    ("blue", "Blue"),
    ("violet", "Violet"),
    ("pink", "Pink"),
    ("graphite", "Graphite"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    DownloadDirectory,
    FilenameTemplate,
    DefaultMode,
    DefaultQuality,
    Concurrency,
    UseDownloadTime,
    Retries,
    FragmentRetries,
    RateLimit,
    Proxy,
    CookieBrowser,
    CookieFile,
    Impersonate,
    Region,
    Country,
    IpVersion,
    Timeout,
    SleepRequests,
    ChunkSize,
    ExtractorRetries,
    LegacyServerConnect,
    InstallTools,
    CheckAgain,
    YtDlpPath,
    FfmpegPath,
    DenoPath,
    Accent,
    Mouse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    Save,
    Leave,
    InstallTools,
    CheckAgain,
}

/// Live state of a tool installation, for the engine section and status bar.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallStatus {
    pub component: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub extracting: bool,
}

impl InstallStatus {
    pub fn describe(&self) -> String {
        if self.extracting {
            return format!("Installing {}…", self.component);
        }
        match self.total {
            Some(total) => format!(
                "Downloading {}… {} of {}",
                self.component,
                format_bytes(Some(self.downloaded as f64)),
                format_bytes(Some(total as f64))
            ),
            None => format!(
                "Downloading {}… {}",
                self.component,
                format_bytes(Some(self.downloaded as f64))
            ),
        }
    }

    pub fn apply(status: &mut Option<Self>, event: &InstallProgress) {
        *status = match event {
            InstallProgress::Downloading {
                component,
                downloaded,
                total,
            } => Some(Self {
                component: component.clone(),
                downloaded: *downloaded,
                total: *total,
                extracting: false,
            }),
            InstallProgress::Extracting { component } => Some(Self {
                component: component.clone(),
                downloaded: 0,
                total: None,
                extracting: true,
            }),
            InstallProgress::Installed { .. } => status.take(),
        };
    }
}

pub struct SettingsForm {
    form: FormState<Field>,
    saved: AppSettings,
    draft: AppSettings,
    download_directory: TextInput,
    filename_template: TextInput,
    retries: TextInput,
    fragment_retries: TextInput,
    rate_limit: TextInput,
    proxy: TextInput,
    cookie_file: TextInput,
    country: TextInput,
    extractor_retries: TextInput,
    yt_dlp_path: TextInput,
    ffmpeg_path: TextInput,
    deno_path: TextInput,
    suggestion: Option<(Field, String)>,
    pub dependencies: Vec<DependencyInfo>,
    pub hardware: HardwareAccelerationInfo,
    pub installing: bool,
    pub install_status: Option<InstallStatus>,
    pub checking: bool,
    pub saving: bool,
    pub error: Option<String>,
    pub tools_dir: std::path::PathBuf,
    pub theme: Theme,
}

fn optional(value: Option<&String>) -> TextInput {
    TextInput::new(value.cloned().unwrap_or_default())
}

fn parse_retries(input: &TextInput, label: &str) -> Result<u8, String> {
    input
        .value()
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|value| *value <= 100)
        .ok_or_else(|| format!("{label} must be a whole number from 0 to 100."))
}

fn index_of<T: PartialEq + Copy>(table: &[(T, &str)], value: T) -> usize {
    table
        .iter()
        .position(|(item, _)| *item == value)
        .unwrap_or(0)
}

fn labels<T>(table: &[(T, &'static str)]) -> Vec<&'static str> {
    table.iter().map(|(_, label)| *label).collect()
}

impl SettingsForm {
    pub fn new(
        settings: &AppSettings,
        dependencies: Vec<DependencyInfo>,
        hardware: HardwareAccelerationInfo,
        tools_dir: std::path::PathBuf,
        theme: Theme,
    ) -> Self {
        let country = settings
            .geo_bypass
            .clone()
            .filter(|value| !matches!(value.as_str(), "default" | "never"))
            .unwrap_or_default();
        Self {
            form: FormState::default(),
            saved: settings.clone(),
            draft: settings.clone(),
            download_directory: TextInput::new(settings.download_directory.clone()),
            filename_template: TextInput::new(settings.filename_template.clone()),
            retries: TextInput::new(settings.retries.to_string()),
            fragment_retries: TextInput::new(settings.fragment_retries.to_string()),
            rate_limit: optional(settings.rate_limit.as_ref()),
            proxy: optional(settings.proxy.as_ref()),
            cookie_file: optional(settings.cookie_file.as_ref()),
            country: TextInput::new(country),
            extractor_retries: TextInput::new(
                settings
                    .extractor_retries
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ),
            yt_dlp_path: optional(settings.yt_dlp_path.as_ref()),
            ffmpeg_path: optional(settings.ffmpeg_path.as_ref()),
            deno_path: optional(settings.deno_path.as_ref()),
            suggestion: None,
            dependencies,
            hardware,
            installing: false,
            install_status: None,
            checking: false,
            saving: false,
            error: None,
            tools_dir,
            theme,
        }
    }

    /// Makes `settings` the new saved baseline, keeping focus and engine state.
    pub fn saved(&mut self, settings: AppSettings) {
        let mut next = Self::new(
            &settings,
            std::mem::take(&mut self.dependencies),
            self.hardware.clone(),
            self.tools_dir.clone(),
            Theme::new(&settings.accent),
        );
        next.form = self.form.clone();
        next.installing = self.installing;
        next.install_status = self.install_status.take();
        next.checking = self.checking;
        *self = next;
    }

    pub fn is_dirty(&self) -> bool {
        self.build().map_or(true, |settings| settings != self.saved)
    }

    /// The edited settings, or the first value that can't be used.
    pub fn build(&self) -> Result<AppSettings, String> {
        let path = |input: &TextInput| {
            input
                .trimmed()
                .map(|value| expand_home(&value).to_string_lossy().into_owned())
        };
        let mut settings = self.draft.clone();
        settings.download_directory = path(&self.download_directory).unwrap_or_default();
        settings.filename_template = self.filename_template.value().trim().to_owned();
        settings.retries = parse_retries(&self.retries, "Retries")?;
        settings.fragment_retries = parse_retries(&self.fragment_retries, "Fragment retries")?;
        settings.rate_limit = self.rate_limit.trimmed();
        settings.proxy = self.proxy.trimmed();
        settings.cookie_file = path(&self.cookie_file);
        if settings
            .geo_bypass
            .as_deref()
            .is_some_and(|value| !matches!(value, "default" | "never"))
        {
            settings.geo_bypass = Some(self.country.value().trim().to_uppercase());
        }
        settings.extractor_retries = self
            .extractor_retries
            .trimmed()
            .map(|value| {
                value
                    .parse::<u32>()
                    .ok()
                    .filter(|value| *value <= 100)
                    .ok_or("Page retries must be a whole number from 0 to 100.")
            })
            .transpose()?;
        settings.yt_dlp_path = path(&self.yt_dlp_path);
        settings.ffmpeg_path = path(&self.ffmpeg_path);
        settings.deno_path = path(&self.deno_path);
        Ok(settings)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Effect {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if !self.form.popup_open() {
            match key.code {
                KeyCode::Esc => return Effect::Leave,
                KeyCode::Char('s') if control => return Effect::Save,
                _ => {}
            }
        }
        let outcome = form::handle_key(self, key);
        self.after(outcome)
    }

    /// A left click at a screen position.
    pub fn handle_click(&mut self, column: u16, line: u16) -> Effect {
        let outcome = form::click(self, column, line);
        self.after(outcome)
    }

    pub fn scroll(&mut self, down: bool) {
        form::scroll(self, down);
    }

    /// Highlights the picker option under the pointer; true when it changed.
    pub fn hover(&mut self, column: u16, line: u16) -> bool {
        form::hover(self, column, line)
    }

    pub fn contains(&mut self, column: u16, line: u16) -> bool {
        form::contains(self, column, line)
    }

    /// Enter on a text field or a button press, from a key or a click.
    fn after(&mut self, outcome: FormOutcome<Field>) -> Effect {
        match outcome {
            FormOutcome::Activated(Field::InstallTools) => Effect::InstallTools,
            FormOutcome::Activated(Field::CheckAgain) => Effect::CheckAgain,
            FormOutcome::Activated(_) => {
                form::handle_key(self, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
                Effect::None
            }
            FormOutcome::Handled | FormOutcome::Ignored => Effect::None,
        }
    }

    pub fn paste(&mut self, text: &str) {
        form::paste(self, text);
    }

    fn text_row(&self, id: Field, label: &str, placeholder: &str) -> Row<Field> {
        let input = self.input_ref(id);
        Row::new(
            id,
            label,
            Control::Text {
                input: input.clone(),
                placeholder: placeholder.into(),
                suggestion: self
                    .suggestion
                    .as_ref()
                    .filter(|(field, _)| *field == id)
                    .map(|(_, suggestion)| suggestion.clone()),
            },
        )
    }

    fn input_ref(&self, id: Field) -> &TextInput {
        match id {
            Field::DownloadDirectory => &self.download_directory,
            Field::FilenameTemplate => &self.filename_template,
            Field::Retries => &self.retries,
            Field::FragmentRetries => &self.fragment_retries,
            Field::RateLimit => &self.rate_limit,
            Field::Proxy => &self.proxy,
            Field::CookieFile => &self.cookie_file,
            Field::Country => &self.country,
            Field::ExtractorRetries => &self.extractor_retries,
            Field::YtDlpPath => &self.yt_dlp_path,
            Field::FfmpegPath => &self.ffmpeg_path,
            _ => &self.deno_path,
        }
    }

    fn engine_rows(&self, rows: &mut Vec<Row<Field>>) {
        let theme = &self.theme;
        rows.push(Row::section("Download engine"));
        let pinned = installer::manifest()
            .components
            .iter()
            .map(|component| format!("{} {}", component.name, component.version))
            .collect::<Vec<_>>()
            .join(", ");
        rows.push(
            Row::note("", Line::from(Span::styled(format!("Pinned: {pinned}"), theme.faint())))
                .help("Downloaded from the official releases and checked against SHA-256 checksums built into this app. Programs on your PATH are never used automatically."),
        );
        for dependency in &self.dependencies {
            let (state, style) = match (dependency.status.as_str(), dependency.source.as_str()) {
                ("available", "custom") => ("Custom", theme.text()),
                ("available", _) => ("Pinned", theme.success()),
                ("missing", _) => ("Missing", theme.error()),
                _ => ("Not working", theme.error()),
            };
            let version = dependency
                .version
                .as_deref()
                .map(short_version)
                .unwrap_or_default();
            let mut row = Row::note(
                dependency.kind.label(),
                Line::from(vec![
                    Span::styled(format!("{state:<10}"), style),
                    Span::styled(version, theme.text()),
                ]),
            );
            let mut details = Vec::new();
            if let Some(message) = &dependency.message {
                details.push(message.clone());
            }
            if let Some(path) = &dependency.path {
                details.push(path.clone());
            }
            if !details.is_empty() {
                row = row.help(details.join(" · "));
            }
            rows.push(row);
        }
        if self.dependencies.is_empty() {
            rows.push(Row::note(
                "",
                Line::from(Span::styled("Checking tools…", theme.faint())),
            ));
        }
        let outdated = installer::outdated_components(&self.tools_dir);
        let installed = installer::installed_versions(&self.tools_dir);
        let action = if self.installing {
            self.install_status
                .as_ref()
                .map_or_else(|| "Installing…".to_string(), InstallStatus::describe)
        } else if outdated.is_empty() {
            "Reinstall pinned tools".into()
        } else if outdated
            .iter()
            .any(|component| installed.contains_key(&component.id))
        {
            "Update pinned tools".into()
        } else {
            "Install pinned tools".into()
        };
        let mut install =
            Row::new(Field::InstallTools, "", button(action)).disabled(self.installing);
        if installer::current_target().is_none() {
            install = install.disabled(true).warn(
                "No pinned builds exist for this platform. Choose your own executables below.",
            );
        } else if !outdated.is_empty() && !self.installing {
            let names = outdated
                .iter()
                .map(|component| match installed.get(&component.id) {
                    Some(old) => format!("{} {old} → {}", component.name, component.version),
                    None => format!("{} {}", component.name, component.version),
                })
                .collect::<Vec<_>>()
                .join(", ");
            install = install.warn(format!("Needed: {names}"));
        } else if self.installing {
            install = install.hint("Press I on the downloads list to cancel.");
        }
        rows.push(install);
        rows.push(
            Row::new(
                Field::CheckAgain,
                "",
                button(if self.checking {
                    "Checking…"
                } else {
                    "Check again"
                }),
            )
            .disabled(self.checking),
        );
        rows.push(
            self.text_row(Field::YtDlpPath, "Custom yt-dlp", "Pinned build")
                .hint("An executable you trust. Leave empty to use the pinned build."),
        );
        rows.push(
            self.text_row(Field::FfmpegPath, "Custom FFmpeg", "Pinned build")
                .hint("FFprobe is taken from the same folder."),
        );
        rows.push(self.text_row(Field::DenoPath, "Custom Deno", "Pinned build"));
    }

    fn gpu_rows(&self, rows: &mut Vec<Row<Field>>) {
        let theme = &self.theme;
        rows.push(Row::section("GPU acceleration"));
        let ready = self
            .hardware
            .encoders
            .iter()
            .filter(|encoder| encoder.available)
            .collect::<Vec<_>>();
        let summary = if self.hardware.status == "checking" {
            "Checking…".to_string()
        } else if ready.is_empty() {
            "Not available".to_string()
        } else {
            let mut vendors = Vec::new();
            for encoder in &ready {
                if !vendors.contains(&encoder.provider.label()) {
                    vendors.push(encoder.provider.label());
                }
            }
            vendors.join(" and ")
        };
        rows.push(
            Row::note(
                "Hardware encoder",
                Line::from(Span::styled(summary, theme.text())),
            )
            .help(self.hardware.message.clone()),
        );
        if !ready.is_empty() {
            let codecs = HardwareCodec::ALL
                .iter()
                .filter(|codec| ready.iter().any(|encoder| encoder.codec == **codec))
                .map(|codec| codec.label())
                .collect::<Vec<_>>()
                .join(", ");
            rows.push(Row::note("Codecs", Line::from(codecs)));
            let decode = ready.iter().any(|encoder| encoder.decode_available);
            rows.push(Row::note(
                "Hardware decoding",
                Line::from(if decode { "Available" } else { "Not available" }),
            ));
        }
    }
}

/// "ffmpeg version n8.1.2-21-g… Copyright" becomes "n8.1.2-21-g…".
fn short_version(version: &str) -> String {
    version
        .split_whitespace()
        .find(|word| word.chars().any(|character| character.is_ascii_digit()))
        .unwrap_or(version)
        .to_owned()
}

impl FormModel for SettingsForm {
    type Id = Field;

    fn rows(&self) -> Vec<Row<Field>> {
        let draft = &self.draft;
        let mut rows = vec![Row::section("Downloads")];
        let folder = expand_home(self.download_directory.value());
        let mut folder_row = self
            .text_row(Field::DownloadDirectory, "Default folder", "~/Downloads")
            .hint("Used until you pick another folder for a download.");
        if !folder.is_absolute() {
            folder_row = folder_row.error("Use an absolute folder, such as ~/Downloads.");
        }
        rows.push(folder_row);
        rows.push(
            self.text_row(Field::FilenameTemplate, "File name", "%(title)s.%(ext)s")
                .hint("A yt-dlp output template. Folders aren't allowed."),
        );
        rows.push(Row::new(
            Field::DefaultMode,
            "Default type",
            choices(
                &["Video", "Audio"],
                usize::from(draft.default_mode == MediaMode::Audio),
            ),
        ));
        rows.push(Row::new(
            Field::DefaultQuality,
            "Default quality",
            choices(
                &labels(&QUALITIES),
                index_of(&QUALITIES, draft.default_quality.as_str()),
            ),
        ));
        rows.push(
            Row::new(
                Field::Concurrency,
                "Downloads at once",
                choices(
                    &["1", "2", "3", "4"],
                    draft.queue_concurrency.clamp(1, 4) as usize - 1,
                ),
            )
            .hint("More parallel downloads use more bandwidth and can trigger site rate limits."),
        );
        rows.push(
            Row::new(
                Field::UseDownloadTime,
                "",
                toggle("Date files by download time", draft.use_download_time),
            )
            .hint("Off: files carry the video's upload date, as yt-dlp does by default."),
        );

        rows.push(Row::section("Network"));
        rows.push(
            self.text_row(Field::Retries, "Retries", "10")
                .hint("Attempts for a whole download."),
        );
        rows.push(
            self.text_row(Field::FragmentRetries, "Fragment retries", "10")
                .hint("Attempts for each piece of a streamed download."),
        );
        rows.push(
            self.text_row(Field::RateLimit, "Speed limit", "No limit, or 5M, 750K")
                .hint("Bytes per second."),
        );
        rows.push(
            self.text_row(Field::Proxy, "Proxy", "socks5://127.0.0.1:1080")
                .hint("A URL without a username or password; credentials aren't stored."),
        );
        rows.push(
            Row::new(
                Field::CookieBrowser,
                "Browser cookies",
                choices(
                    &labels(&BROWSERS),
                    index_of(&BROWSERS, draft.cookie_browser.as_deref()),
                ),
            )
            .hint(
                "Lets yt-dlp read a signed-in browser session for private or age-restricted media.",
            ),
        );
        rows.push(
            self.text_row(
                Field::CookieFile,
                "Cookie file",
                "Netscape-format cookies.txt",
            )
            .disabled(draft.cookie_browser.is_some())
            .help_if(
                draft.cookie_browser.is_some(),
                "Browser cookies are used instead.",
            )
            .hint("The file is read by yt-dlp; its contents are never copied."),
        );
        rows.push(
            Row::new(
                Field::Impersonate,
                "Impersonation",
                choices(
                    &labels(&IMPERSONATE),
                    index_of(&IMPERSONATE, draft.impersonate.as_deref()),
                ),
            )
            .hint(
                "Some sites block downloaders. This makes requests look like the chosen browser.",
            ),
        );
        let region = match draft.geo_bypass.as_deref() {
            None | Some("default") => 0,
            Some("never") => 1,
            Some(_) => 2,
        };
        rows.push(
            Row::new(
                Field::Region,
                "Region",
                choices(&["Automatic", "Off", "Specific country"], region),
            )
            .hint("Ask sites to treat you as being in another country, for region-locked media."),
        );
        if region == 2 {
            rows.push(
                self.text_row(Field::Country, "Country code", "US")
                    .indent()
                    .hint("Two letters, such as US or JP."),
            );
        }
        let ip = match draft.ip_version {
            None => 0,
            Some(IpVersion::Ipv4) => 1,
            Some(IpVersion::Ipv6) => 2,
        };
        rows.push(Row::new(
            Field::IpVersion,
            "IP version",
            choices(&["Automatic", "IPv4 only", "IPv6 only"], ip),
        ));
        rows.push(Row::new(
            Field::Timeout,
            "Timeout",
            choices(
                &labels(&TIMEOUTS),
                index_of(&TIMEOUTS, draft.socket_timeout),
            ),
        ));
        rows.push(
            Row::new(
                Field::SleepRequests,
                "Request pause",
                choices(
                    &labels(&SLEEP_REQUESTS),
                    index_of(&SLEEP_REQUESTS, draft.sleep_requests),
                ),
            )
            .hint("Slows analysis and downloads slightly to avoid site rate limits."),
        );
        rows.push(
            Row::new(
                Field::ChunkSize,
                "Download in chunks",
                choices(
                    &labels(&CHUNK_SIZES),
                    index_of(&CHUNK_SIZES, draft.http_chunk_size_mb),
                ),
            )
            .hint("Splits large files into smaller requests, which some sites throttle less."),
        );
        rows.push(
            self.text_row(Field::ExtractorRetries, "Page retries", "3")
                .hint("Extra attempts when a site's page fails to load during analysis."),
        );
        rows.push(
            Row::new(
                Field::LegacyServerConnect,
                "",
                toggle("Legacy server connections", draft.legacy_server_connect),
            )
            .hint("Allows older HTTPS servers that newer security defaults refuse."),
        );

        self.engine_rows(&mut rows);
        self.gpu_rows(&mut rows);

        rows.push(Row::section("Appearance"));
        rows.push(
            Row::new(
                Field::Accent,
                "Accent color",
                choices(&labels(&ACCENTS), index_of(&ACCENTS, draft.accent.as_str())),
            )
            .hint("Selection, progress, and key hints. Set NO_COLOR to turn colors off."),
        );
        rows.push(
            Row::new(Field::Mouse, "Mouse", toggle("Click, scroll, and hover", draft.mouse))
                .hint("While on, hold Shift and drag to select text. Off leaves the mouse to the terminal."),
        );
        rows
    }

    fn form(&mut self) -> &mut FormState<Field> {
        &mut self.form
    }

    fn choose(&mut self, id: Field, index: usize) {
        let draft = &mut self.draft;
        match id {
            Field::DefaultMode => {
                draft.default_mode = if index == 1 {
                    MediaMode::Audio
                } else {
                    MediaMode::Video
                }
            }
            Field::DefaultQuality => draft.default_quality = QUALITIES[index].0.into(),
            Field::Concurrency => draft.queue_concurrency = index as u8 + 1,
            Field::CookieBrowser => draft.cookie_browser = BROWSERS[index].0.map(str::to_owned),
            Field::Impersonate => draft.impersonate = IMPERSONATE[index].0.map(str::to_owned),
            Field::Region => {
                draft.geo_bypass = match index {
                    1 => Some("never".into()),
                    2 => {
                        if self.country.value().trim().is_empty() {
                            self.country.set("US");
                        }
                        Some(self.country.value().trim().to_uppercase())
                    }
                    _ => None,
                }
            }
            Field::IpVersion => {
                draft.ip_version = match index {
                    1 => Some(IpVersion::Ipv4),
                    2 => Some(IpVersion::Ipv6),
                    _ => None,
                }
            }
            Field::Timeout => draft.socket_timeout = TIMEOUTS[index].0,
            Field::SleepRequests => draft.sleep_requests = SLEEP_REQUESTS[index].0,
            Field::ChunkSize => draft.http_chunk_size_mb = CHUNK_SIZES[index].0,
            Field::Accent => {
                draft.accent = ACCENTS[index].0.into();
                self.theme = Theme::new(&draft.accent);
            }
            _ => {}
        }
        self.error = None;
    }

    fn toggle(&mut self, id: Field) {
        match id {
            Field::UseDownloadTime => self.draft.use_download_time = !self.draft.use_download_time,
            Field::LegacyServerConnect => {
                self.draft.legacy_server_connect = !self.draft.legacy_server_connect
            }
            Field::Mouse => self.draft.mouse = !self.draft.mouse,
            _ => {}
        }
        self.error = None;
    }

    fn input(&mut self, id: Field) -> Option<&mut TextInput> {
        Some(match id {
            Field::DownloadDirectory => &mut self.download_directory,
            Field::FilenameTemplate => &mut self.filename_template,
            Field::Retries => &mut self.retries,
            Field::FragmentRetries => &mut self.fragment_retries,
            Field::RateLimit => &mut self.rate_limit,
            Field::Proxy => &mut self.proxy,
            Field::CookieFile => &mut self.cookie_file,
            Field::Country => &mut self.country,
            Field::ExtractorRetries => &mut self.extractor_retries,
            Field::YtDlpPath => &mut self.yt_dlp_path,
            Field::FfmpegPath => &mut self.ffmpeg_path,
            Field::DenoPath => &mut self.deno_path,
            _ => return None,
        })
    }

    fn text_changed(&mut self, id: Field) {
        let files = match id {
            Field::DownloadDirectory => Some(false),
            Field::CookieFile | Field::YtDlpPath | Field::FfmpegPath | Field::DenoPath => {
                Some(true)
            }
            _ => None,
        };
        self.suggestion = files.and_then(|files| {
            path_suggestion(self.input_ref(id).value(), files).map(|suggestion| (id, suggestion))
        });
        self.error = None;
    }
}

/// Whether the given tool answered its version check.
pub fn tool_ready(dependencies: &[DependencyInfo], kind: DependencyKind) -> bool {
    dependencies
        .iter()
        .any(|dependency| dependency.kind == kind && dependency.status == "available")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> SettingsForm {
        let settings = AppSettings {
            download_directory: "/tmp".into(),
            ..AppSettings::default()
        };
        SettingsForm::new(
            &settings,
            Vec::new(),
            HardwareAccelerationInfo::checking(),
            std::env::temp_dir(),
            Theme::new("blue"),
        )
    }

    #[test]
    fn starts_clean_and_tracks_edits() {
        let mut form = form();
        assert!(!form.is_dirty());
        form.choose(Field::Concurrency, 2);
        assert!(form.is_dirty());
        assert_eq!(form.build().unwrap().queue_concurrency, 3);
        form.choose(Field::Concurrency, 0);
        assert!(!form.is_dirty());
    }

    #[test]
    fn parses_typed_values_and_reports_bad_ones() {
        let mut form = form();
        form.retries.set("250");
        assert!(form.build().unwrap_err().contains("Retries"));
        form.retries.set("5");
        form.proxy.set("  socks5://127.0.0.1:1080 ");
        form.yt_dlp_path.set("~/bin/yt-dlp");
        let settings = form.build().unwrap();
        assert_eq!(settings.retries, 5);
        assert_eq!(settings.proxy.as_deref(), Some("socks5://127.0.0.1:1080"));
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            settings.yt_dlp_path.as_deref(),
            home.join("bin/yt-dlp").to_str()
        );
    }

    #[test]
    fn a_specific_region_uses_the_typed_country() {
        let mut form = form();
        form.choose(Field::Region, 2);
        assert_eq!(form.build().unwrap().geo_bypass.as_deref(), Some("US"));
        form.country.set("jp");
        assert_eq!(form.build().unwrap().geo_bypass.as_deref(), Some("JP"));
        form.choose(Field::Region, 1);
        assert_eq!(form.build().unwrap().geo_bypass.as_deref(), Some("never"));
    }

    #[test]
    fn shortens_tool_version_lines() {
        assert_eq!(short_version("2026.08.19"), "2026.08.19");
        assert_eq!(
            short_version("ffmpeg version n8.1.2-21-gce3c09c101 Copyright"),
            "n8.1.2-21-gce3c09c101"
        );
        assert_eq!(short_version("deno 2.9.7 (stable, release)"), "2.9.7");
    }
}
