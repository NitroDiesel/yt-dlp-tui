//! Application state and input handling. Slow work runs on spawned tasks that
//! report back as [`Message`]s, so the interface never waits on yt-dlp.

use std::{
    future::Future,
    io::Write,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Position;
use tokio::sync::mpsc::UnboundedSender;

use super::{
    format::{DownloadFilter, link_from_text, merge_downloads},
    mouse::{Click, Hits, Layer},
    new_download::{self, Engine, NewDownload},
    settings::{self, InstallStatus, SettingsForm, tool_ready},
    theme::Theme,
};
use crate::{
    application::{AppService, ServiceEvent},
    domain::{
        AppSettings, AppSnapshot, DependencyInfo, DependencyKind, DownloadJob, DownloadRequest,
        HardwareAccelerationInfo, JobStatus, MediaProbe,
    },
    integration::installer::{self, InstallProgress},
    platform,
};

/// Results of background work, delivered to the event loop.
#[derive(Debug)]
pub enum Message {
    Probed {
        revision: u64,
        result: Result<MediaProbe, String>,
    },
    Enqueued {
        result: Result<DownloadJob, String>,
        destination: String,
    },
    Retried {
        old: String,
        result: Result<DownloadJob, String>,
    },
    Removed(String),
    ClearedCompleted,
    Queue(Vec<DownloadJob>),
    Paused(bool),
    Dependencies(Vec<DependencyInfo>),
    Hardware(HardwareAccelerationInfo),
    SettingsSaved(Result<AppSettings, String>),
    Remembered(AppSettings),
    Install(InstallProgress),
    Installed(Result<Vec<String>, String>),
    Notice(String),
    Problem(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Downloads,
    Settings,
}

pub enum Modal {
    NewDownload(Box<NewDownload>),
    Help,
}

#[derive(Debug)]
pub enum ConfirmAction {
    Quit,
    ClearCompleted,
    DownloadPlaylist {
        request: Box<DownloadRequest>,
        start_immediately: bool,
        title: Option<String>,
    },
    DiscardSettings,
    InstallTools,
}

#[derive(Debug)]
pub struct Confirm {
    pub title: String,
    pub message: String,
    pub confirm: String,
    pub cancel: String,
    pub action: ConfirmAction,
    /// Whether the confirm button (rather than cancel) has focus.
    pub confirm_focused: bool,
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    expires: Instant,
}

pub struct Install {
    cancel: Arc<AtomicBool>,
    pub status: Option<InstallStatus>,
}

pub struct App {
    service: Arc<AppService>,
    tx: UnboundedSender<Message>,
    pub settings: AppSettings,
    pub theme: Theme,
    pub dependencies: Vec<DependencyInfo>,
    pub hardware: HardwareAccelerationInfo,
    hardware_requested: bool,
    pub queue: Vec<DownloadJob>,
    pub history: Vec<DownloadJob>,
    pub queue_paused: bool,
    pub screen: Screen,
    pub filter: DownloadFilter,
    pub selected: Option<String>,
    pub details: bool,
    pub details_scroll: u16,
    pub sidebar: bool,
    pub modal: Option<Modal>,
    pub confirm: Option<Confirm>,
    pub settings_form: Option<SettingsForm>,
    pub install: Option<Install>,
    pub toast: Option<Toast>,
    pub should_quit: bool,
    /// Setup is offered by itself once per session, when the engine is missing.
    setup_offered: bool,
    pub tools_dir: PathBuf,
    /// Width of the area beside the sidebar at the last draw.
    pub main_width: u16,
    /// Clickable areas from the last draw.
    pub hits: Hits,
    /// The clickable item under the pointer, drawn underlined.
    pub hovered: Option<Click>,
    /// The last clicked download and when, to recognise a double-click.
    last_click: Option<(String, Instant)>,
}

/// Two clicks on the same download within this time are a double-click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

impl App {
    pub fn new(
        service: Arc<AppService>,
        snapshot: AppSnapshot,
        tx: UnboundedSender<Message>,
    ) -> Self {
        let tools_dir = service.tools_dir();
        let mut app = Self {
            service,
            tx,
            theme: Theme::new(&snapshot.settings.accent),
            settings: snapshot.settings,
            dependencies: snapshot.dependencies,
            hardware: snapshot.hardware_acceleration,
            hardware_requested: false,
            queue: snapshot.queue,
            history: snapshot.history,
            queue_paused: snapshot.queue_paused,
            screen: Screen::Downloads,
            filter: DownloadFilter::All,
            selected: None,
            details: false,
            details_scroll: 0,
            sidebar: true,
            modal: None,
            confirm: None,
            settings_form: None,
            install: None,
            toast: None,
            should_quit: false,
            setup_offered: false,
            tools_dir,
            main_width: 0,
            hits: Hits::default(),
            hovered: None,
            last_click: None,
        };
        app.settle_selection();
        app
    }

    /// Checks the tools in the background and opens a link given at launch.
    pub fn start(&mut self, link: Option<String>) {
        self.refresh_tools(false);
        if let Some(link) = link {
            self.open_new_download(Some(link));
        }
    }

    fn spawn<F>(&self, work: F)
    where
        F: Future<Output = Message> + Send + 'static,
    {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(work.await);
        });
    }

    pub fn notify(&mut self, text: impl Into<String>, error: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            error,
            expires: Instant::now() + Duration::from_secs(if error { 8 } else { 4 }),
        });
    }

    /// Drops an expired notice. Returns true when the screen should redraw.
    pub fn tick(&mut self) -> bool {
        if self
            .toast
            .as_ref()
            .is_some_and(|toast| toast.expires <= Instant::now())
        {
            self.toast = None;
            return true;
        }
        false
    }

    pub fn running_jobs(&self) -> usize {
        self.queue
            .iter()
            .filter(|job| job.status.is_running())
            .count()
    }

    /// Whether downloads can run: still checking, ready, or not set up.
    pub fn engine(&self) -> Engine {
        if self.dependencies.is_empty() || self.install.is_some() {
            Engine::Checking
        } else if self.yt_dlp_ready() {
            Engine::Ready
        } else {
            Engine::Missing
        }
    }

    pub fn yt_dlp_ready(&self) -> bool {
        tool_ready(&self.dependencies, DependencyKind::YtDlp)
    }

    pub fn visible_jobs(&self) -> Vec<&DownloadJob> {
        merge_downloads(&self.queue, &self.history)
            .into_iter()
            .filter(|job| self.filter.includes(job.status))
            .collect()
    }

    pub fn selected_job(&self) -> Option<&DownloadJob> {
        let id = self.selected.as_ref()?;
        self.queue
            .iter()
            .chain(&self.history)
            .find(|job| &job.id == id)
    }

    fn settle_selection(&mut self) {
        let visible = self.visible_jobs();
        let keep = self
            .selected
            .as_ref()
            .is_some_and(|id| visible.iter().any(|job| &job.id == id));
        if !keep {
            self.selected = visible.first().map(|job| job.id.clone());
            self.details_scroll = 0;
        }
    }

    fn merge_job(list: &mut Vec<DownloadJob>, job: DownloadJob) {
        match list.iter_mut().find(|existing| existing.id == job.id) {
            Some(existing) => *existing = job,
            None => list.insert(0, job),
        }
    }

    fn update_job(&mut self, job: DownloadJob) {
        if DownloadFilter::of(job.status) != DownloadFilter::Downloading
            && job.status != JobStatus::Interrupted
        {
            Self::merge_job(&mut self.history, job.clone());
        }
        Self::merge_job(&mut self.queue, job);
        self.settle_selection();
    }

    /// Adds a job returned by enqueue or retry. Service events travel on a
    /// separate channel and may already have delivered a newer state, which
    /// this older snapshot must not overwrite.
    fn add_job(&mut self, job: DownloadJob) {
        let known = self
            .queue
            .iter()
            .chain(&self.history)
            .any(|existing| existing.id == job.id);
        if !known {
            self.update_job(job);
        }
    }

    fn forget_job(&mut self, id: &str) {
        self.queue.retain(|job| job.id != id);
        self.history.retain(|job| job.id != id);
        if self.selected.as_deref() == Some(id) {
            self.selected = None;
        }
        self.settle_selection();
    }

    pub fn handle_service(&mut self, event: ServiceEvent) {
        match event {
            ServiceEvent::JobChanged(job) => self.update_job(*job),
        }
    }

    pub fn handle_message(&mut self, message: Message) {
        match message {
            Message::Probed { revision, result } => {
                if let Some(Modal::NewDownload(form)) = self.modal.as_mut() {
                    form.analyzed(revision, result);
                }
            }
            Message::Enqueued {
                result,
                destination,
            } => match result {
                Ok(job) => {
                    if matches!(self.modal, Some(Modal::NewDownload(_))) {
                        self.modal = None;
                    }
                    if self.filter != DownloadFilter::All {
                        self.filter = DownloadFilter::Downloading;
                    }
                    self.selected = Some(job.id.clone());
                    self.details = self.main_width >= super::view::SIDE_BY_SIDE;
                    self.add_job(job);
                    let service = self.service.clone();
                    if self.settings.last_download_directory.as_deref()
                        != Some(destination.as_str())
                    {
                        self.spawn(async move {
                            match service.remember_download_directory(&destination).await {
                                Ok(settings) => Message::Remembered(settings),
                                Err(_) => Message::Notice(String::new()),
                            }
                        });
                    }
                }
                Err(error) => {
                    if let Some(Modal::NewDownload(form)) = self.modal.as_mut() {
                        form.submitting = false;
                        form.error = Some(error);
                    } else {
                        self.notify(error, true);
                    }
                }
            },
            Message::Retried { old, result } => match result {
                Ok(job) => {
                    self.forget_job(&old);
                    self.selected = Some(job.id.clone());
                    self.add_job(job);
                    self.notify("Started a new attempt", false);
                }
                Err(error) => self.notify(error, true),
            },
            Message::Removed(id) => self.forget_job(&id),
            Message::ClearedCompleted => {
                self.queue.retain(|job| job.status != JobStatus::Completed);
                self.history
                    .retain(|job| job.status != JobStatus::Completed);
                self.settle_selection();
                self.notify(
                    "Completed downloads cleared from the list. The files stay where they are.",
                    false,
                );
            }
            Message::Queue(queue) => {
                self.queue = queue;
                self.settle_selection();
            }
            Message::Paused(paused) => self.queue_paused = paused,
            Message::Dependencies(dependencies) => {
                if let Some(form) = self.settings_form.as_mut() {
                    form.dependencies = dependencies.clone();
                    form.checking = false;
                }
                self.dependencies = dependencies;
                self.engine_changed();
                self.offer_setup_once();
            }
            Message::Hardware(hardware) => {
                if let Some(Modal::NewDownload(form)) = self.modal.as_mut() {
                    form.hardware = hardware.clone();
                }
                if let Some(form) = self.settings_form.as_mut() {
                    form.hardware = hardware.clone();
                }
                self.hardware = hardware;
            }
            Message::SettingsSaved(result) => {
                let Some(form) = self.settings_form.as_mut() else {
                    return;
                };
                form.saving = false;
                match result {
                    Ok(saved) => {
                        let tools_changed = saved.yt_dlp_path != self.settings.yt_dlp_path
                            || saved.ffmpeg_path != self.settings.ffmpeg_path
                            || saved.deno_path != self.settings.deno_path;
                        form.saved(saved.clone());
                        self.theme = Theme::new(&saved.accent);
                        self.settings = saved;
                        if tools_changed {
                            self.refresh_tools(true);
                        }
                        self.notify("Settings saved", false);
                    }
                    Err(error) => form.error = Some(error),
                }
            }
            Message::Remembered(settings) => {
                self.settings.last_download_directory = settings.last_download_directory;
            }
            Message::Install(progress) => {
                if let Some(install) = self.install.as_mut() {
                    InstallStatus::apply(&mut install.status, &progress);
                    if let Some(form) = self.settings_form.as_mut() {
                        form.install_status = install.status.clone();
                    }
                }
                if let InstallProgress::Installed { component, version } = progress {
                    self.notify(format!("Installed {component} {version}"), false);
                }
            }
            Message::Installed(result) => {
                self.install = None;
                if let Some(form) = self.settings_form.as_mut() {
                    form.installing = false;
                    form.install_status = None;
                }
                match result {
                    Ok(installed) if installed.is_empty() => {
                        self.notify("The pinned tools are already installed", false)
                    }
                    Ok(installed) => self.notify(
                        format!("Installed {}. Ready to download.", installed.join(", ")),
                        false,
                    ),
                    Err(error) => self.notify(error, true),
                }
                self.refresh_tools(true);
            }
            Message::Notice(text) if text.is_empty() => {}
            Message::Notice(text) => self.notify(text, false),
            Message::Problem(text) => self.notify(text, true),
        }
    }

    /// Tells New download about the engine, which may read a waiting link.
    fn engine_changed(&mut self) {
        let engine = self.engine();
        let ffmpeg_ready = tool_ready(&self.dependencies, DependencyKind::Ffmpeg);
        if let Some(Modal::NewDownload(form)) = self.modal.as_mut() {
            form.ffmpeg_ready = ffmpeg_ready;
            let effect = form.set_engine(engine);
            self.run_new_download_effect(effect);
        }
    }

    /// The first time the engine turns out to be missing, offer to set it up,
    /// so a fresh install works without installing yt-dlp or anything else.
    fn offer_setup_once(&mut self) {
        if self.setup_offered
            || self.engine() != Engine::Missing
            || self.settings.yt_dlp_path.is_some()
            || installer::current_target().is_none()
            || self.confirm.is_some()
        {
            return;
        }
        self.setup_offered = true;
        self.confirm_install();
    }

    /// Re-checks tool versions; with `hardware`, also forgets the GPU result.
    fn refresh_tools(&mut self, hardware: bool) {
        let service = self.service.clone();
        self.spawn(async move { Message::Dependencies(service.dependencies().await) });
        if hardware {
            self.hardware = HardwareAccelerationInfo::checking();
            self.hardware_requested = false;
            if self.modal.is_some() || self.settings_form.is_some() {
                self.ensure_hardware(true);
            }
        }
    }

    /// GPU detection runs real test encodes, so it starts only when needed.
    fn ensure_hardware(&mut self, force: bool) {
        if self.hardware_requested && !force {
            return;
        }
        if !force && self.hardware.status != "checking" {
            return;
        }
        self.hardware_requested = true;
        let service = self.service.clone();
        self.spawn(async move { Message::Hardware(service.hardware_acceleration(force).await) });
    }

    /// Applies terminal input. Returns false when nothing visible changed,
    /// such as pointer movement that changes no highlight.
    pub fn handle_event(&mut self, event: Event) -> bool {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.handle_key(key),
            Event::Paste(text) => self.handle_paste(&text),
            Event::Mouse(mouse) => return self.handle_mouse(mouse),
            Event::Resize(..) => {}
            _ => return false,
        }
        true
    }

    /// Layers that take clicks right now: only the topmost dialog, plus the
    /// key hints under a form.
    pub fn open_layers(&self) -> &'static [Layer] {
        if self.confirm.is_some() {
            &[Layer::Confirm]
        } else if self.modal.is_some() {
            &[Layer::Modal, Layer::Bar]
        } else {
            &[Layer::Base, Layer::Bar]
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> bool {
        let (column, line) = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => self.click(column, line),
            MouseEventKind::ScrollDown => self.scroll(column, line, true),
            MouseEventKind::ScrollUp => self.scroll(column, line, false),
            MouseEventKind::Moved => return self.hover(column, line),
            _ => return false,
        }
        true
    }

    fn click(&mut self, column: u16, line: u16) {
        if self.confirm.is_none() && matches!(self.modal, Some(Modal::Help)) {
            self.modal = None;
            return;
        }
        let hit = self
            .hits
            .at(column, line, self.open_layers())
            .map(|hit| hit.click.clone());
        if let Some(click) = hit {
            self.run_click(click);
            return;
        }
        if self.confirm.is_some() {
            return;
        }
        if let Some(Modal::NewDownload(form)) = self.modal.as_mut() {
            let effect = form.handle_click(column, line);
            self.run_new_download_effect(effect);
        } else if self.screen == Screen::Settings
            && let Some(form) = self.settings_form.as_mut()
        {
            let effect = form.handle_click(column, line);
            self.run_settings_effect(effect);
        }
    }

    fn run_click(&mut self, click: Click) {
        match click {
            Click::Key(key) => self.handle_key(key),
            Click::NewDownload => self.open_new_download(None),
            Click::Settings => {
                if self.screen != Screen::Settings {
                    self.open_settings();
                }
            }
            Click::Filter(filter) => self.show_downloads(filter),
            Click::Job(id) => self.click_job(id),
            Click::Confirm(confirmed) => {
                if let Some(confirm) = self.confirm.take()
                    && confirmed
                {
                    self.run_confirmed(confirm.action);
                }
            }
        }
    }

    /// Selects a download; a double-click, or any click when details fit
    /// beside the list, also shows its details.
    fn click_job(&mut self, id: String) {
        let now = Instant::now();
        let double = self
            .last_click
            .as_ref()
            .is_some_and(|(last, at)| *last == id && now.duration_since(*at) < DOUBLE_CLICK);
        self.last_click = (!double).then(|| (id.clone(), now));
        if self.selected.as_ref() != Some(&id) {
            self.selected = Some(id);
            self.details_scroll = 0;
        }
        if double || self.main_width >= super::view::SIDE_BY_SIDE {
            self.details = true;
        }
    }

    /// The sidebar filters also leave Settings, asking first if it has changes.
    fn show_downloads(&mut self, filter: DownloadFilter) {
        if self.screen == Screen::Settings {
            if self
                .settings_form
                .as_ref()
                .is_some_and(SettingsForm::is_dirty)
            {
                self.run_settings_effect(settings::Effect::Leave);
                return;
            }
            self.settings_form = None;
            self.screen = Screen::Downloads;
        }
        self.set_filter(filter);
    }

    fn scroll(&mut self, column: u16, line: u16, down: bool) {
        if self.confirm.is_some() {
            return;
        }
        match self.modal.as_mut() {
            Some(Modal::NewDownload(form)) => return form.scroll(down),
            Some(Modal::Help) => return,
            None => {}
        }
        if self.screen == Screen::Settings {
            if let Some(form) = self.settings_form.as_mut() {
                form.scroll(down);
            }
            return;
        }
        let point = Position::new(column, line);
        if self.hits.details.is_some_and(|area| area.contains(point)) {
            self.details_scroll = if down {
                self.details_scroll.saturating_sub(3)
            } else {
                self.details_scroll.saturating_add(3)
            };
        } else {
            self.select_offset(if down { 1 } else { -1 });
        }
    }

    /// Tracks the item under the pointer. Returns true when a highlight moved.
    fn hover(&mut self, column: u16, line: u16) -> bool {
        let mut changed = false;
        if self.confirm.is_none() {
            match self.modal.as_mut() {
                Some(Modal::NewDownload(form)) => changed |= form.hover(column, line),
                Some(Modal::Help) => {}
                None if self.screen == Screen::Settings => {
                    if let Some(form) = self.settings_form.as_mut() {
                        changed |= form.hover(column, line);
                    }
                }
                None => {}
            }
        }
        let hovered = self
            .hits
            .at(column, line, self.open_layers())
            .filter(|hit| hit.hover)
            .map(|hit| hit.click.clone());
        if hovered != self.hovered {
            self.hovered = hovered;
            changed = true;
        }
        changed
    }

    fn handle_paste(&mut self, text: &str) {
        if self.confirm.is_some() {
            return;
        }
        match self.modal.as_mut() {
            Some(Modal::NewDownload(form)) => {
                let effect = form.paste(text);
                self.run_new_download_effect(effect);
                return;
            }
            Some(Modal::Help) => self.modal = None,
            None => {}
        }
        if self.screen == Screen::Settings {
            if let Some(form) = self.settings_form.as_mut() {
                form.paste(text);
            }
            return;
        }
        if let Some(link) = link_from_text(text) {
            self.open_new_download(Some(link));
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && key.code == KeyCode::Char('c') {
            self.request_quit();
            return;
        }
        if self.confirm.is_some() {
            self.handle_confirm_key(key);
            return;
        }
        match self.modal.as_mut() {
            Some(Modal::Help) => {
                self.modal = None;
                return;
            }
            Some(Modal::NewDownload(form)) => {
                let effect = form.handle_key(key);
                self.run_new_download_effect(effect);
                return;
            }
            None => {}
        }
        if self.screen == Screen::Settings {
            self.handle_settings_key(key);
            return;
        }
        self.handle_downloads_key(key);
    }

    fn handle_confirm_key(&mut self, key: KeyEvent) {
        let Some(confirm) = self.confirm.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => self.confirm = None,
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                confirm.confirm_focused = !confirm.confirm_focused;
            }
            KeyCode::Char('y') => {
                if let Some(confirm) = self.confirm.take() {
                    self.run_confirmed(confirm.action);
                }
            }
            KeyCode::Enter => {
                if let Some(confirm) = self.confirm.take()
                    && confirm.confirm_focused
                {
                    self.run_confirmed(confirm.action);
                }
            }
            _ => {}
        }
    }

    fn run_confirmed(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::Quit => self.should_quit = true,
            ConfirmAction::ClearCompleted => {
                let service = self.service.clone();
                self.spawn(async move {
                    match service.clear_completed().await {
                        Ok(()) => Message::ClearedCompleted,
                        Err(error) => Message::Problem(error.to_string()),
                    }
                });
            }
            ConfirmAction::DownloadPlaylist {
                request,
                start_immediately,
                title,
            } => self.enqueue(*request, start_immediately, title),
            ConfirmAction::DiscardSettings => {
                self.settings_form = None;
                self.screen = Screen::Downloads;
            }
            ConfirmAction::InstallTools => self.start_install(),
        }
    }

    fn ask(
        &mut self,
        title: &str,
        message: String,
        confirm: &str,
        cancel: &str,
        action: ConfirmAction,
    ) {
        self.confirm = Some(Confirm {
            title: title.into(),
            message,
            confirm: confirm.into(),
            cancel: cancel.into(),
            action,
            confirm_focused: true,
        });
    }

    pub fn request_quit(&mut self) {
        let running = self.running_jobs();
        if running == 0 && self.install.is_none() {
            self.should_quit = true;
            return;
        }
        let mut message = Vec::new();
        if running > 0 {
            message.push(format!(
                "{running} {} will stop. Retry resumes from the partial file.",
                if running == 1 {
                    "download is running and"
                } else {
                    "downloads are running and"
                }
            ));
        }
        if self.install.is_some() {
            message.push("The tool installation will be cancelled.".into());
        }
        self.ask(
            "Quit yt-dlp TUI?",
            message.join(" "),
            "Quit",
            "Keep running",
            ConfirmAction::Quit,
        );
    }

    pub fn open_new_download(&mut self, link: Option<String>) {
        self.screen_downloads_if_clean();
        let form = NewDownload::new(
            &self.settings,
            self.engine(),
            tool_ready(&self.dependencies, DependencyKind::Ffmpeg),
            self.hardware.clone(),
            self.theme,
        );
        self.modal = Some(Modal::NewDownload(Box::new(form)));
        self.ensure_hardware(false);
        if let Some(link) = link
            && let Some(Modal::NewDownload(form)) = self.modal.as_mut()
        {
            let effect = form.set_link(&link);
            self.run_new_download_effect(effect);
        }
    }

    /// New download opens over the downloads list; unsaved settings stay open.
    fn screen_downloads_if_clean(&mut self) {
        if self.screen == Screen::Settings
            && !self
                .settings_form
                .as_ref()
                .is_some_and(SettingsForm::is_dirty)
        {
            self.screen = Screen::Downloads;
            self.settings_form = None;
        }
    }

    fn run_new_download_effect(&mut self, effect: new_download::Effect) {
        let Some(Modal::NewDownload(form)) = self.modal.as_mut() else {
            return;
        };
        match effect {
            new_download::Effect::None => {}
            new_download::Effect::Close => {
                if form.analyzing {
                    let service = self.service.clone();
                    tokio::spawn(async move { service.cancel_probe().await });
                }
                self.modal = None;
            }
            new_download::Effect::Analyze { url, no_playlist } => {
                let revision = form.revision;
                let service = self.service.clone();
                self.spawn(async move {
                    Message::Probed {
                        revision,
                        result: service
                            .probe_media(url, no_playlist)
                            .await
                            .map_err(|error| error.to_string()),
                    }
                });
            }
            new_download::Effect::SetupEngine => {
                if self.install.is_none() {
                    self.confirm_install();
                }
            }
            new_download::Effect::Submit { start_immediately } => match form.build_request() {
                Err(error) => form.error = Some(error),
                Ok(request) => {
                    let title = form.probe.as_ref().map(|probe| probe.title.clone());
                    if let Some(question) = form.needs_playlist_confirmation() {
                        self.ask(
                            "Download the whole playlist?",
                            question,
                            "Download all",
                            "Go back",
                            ConfirmAction::DownloadPlaylist {
                                request: Box::new(request),
                                start_immediately,
                                title,
                            },
                        );
                    } else {
                        self.enqueue(request, start_immediately, title);
                    }
                }
            },
        }
    }

    fn enqueue(
        &mut self,
        request: DownloadRequest,
        start_immediately: bool,
        title: Option<String>,
    ) {
        if let Some(Modal::NewDownload(form)) = self.modal.as_mut() {
            form.submitting = true;
            form.error = None;
        }
        let service = self.service.clone();
        let destination = request.destination.clone();
        self.spawn(async move {
            let result = match tokio::fs::create_dir_all(&request.destination).await {
                Ok(()) => service
                    .enqueue(request, start_immediately, title)
                    .await
                    .map_err(|error| error.to_string()),
                Err(error) => Err(format!("The download folder can't be created: {error}")),
            };
            Message::Enqueued {
                result,
                destination,
            }
        });
    }

    fn open_settings(&mut self) {
        self.screen = Screen::Settings;
        if self.settings_form.is_none() {
            self.settings_form = Some(SettingsForm::new(
                &self.settings,
                self.dependencies.clone(),
                self.hardware.clone(),
                self.tools_dir.clone(),
                self.theme,
            ));
        }
        if let Some(form) = self.settings_form.as_mut() {
            form.installing = self.install.is_some();
            form.install_status = self
                .install
                .as_ref()
                .and_then(|install| install.status.clone());
        }
        self.ensure_hardware(false);
    }

    fn handle_settings_key(&mut self, key: KeyEvent) {
        let Some(form) = self.settings_form.as_mut() else {
            self.screen = Screen::Downloads;
            return;
        };
        let effect = form.handle_key(key);
        self.run_settings_effect(effect);
    }

    fn run_settings_effect(&mut self, effect: settings::Effect) {
        let Some(form) = self.settings_form.as_mut() else {
            return;
        };
        match effect {
            settings::Effect::None => {}
            settings::Effect::Leave => {
                if form.is_dirty() {
                    self.ask(
                        "Discard changes?",
                        "Your settings changes haven't been saved.".into(),
                        "Discard",
                        "Keep editing",
                        ConfirmAction::DiscardSettings,
                    );
                } else {
                    self.settings_form = None;
                    self.screen = Screen::Downloads;
                }
            }
            settings::Effect::Save => match form.build() {
                Err(error) => form.error = Some(error),
                Ok(settings) => {
                    form.saving = true;
                    let service = self.service.clone();
                    self.spawn(async move {
                        Message::SettingsSaved(
                            service
                                .save_settings(settings)
                                .await
                                .map_err(|error| error.to_string()),
                        )
                    });
                }
            },
            settings::Effect::CheckAgain => {
                form.checking = true;
                self.refresh_tools(false);
                self.ensure_hardware(true);
            }
            settings::Effect::InstallTools => self.confirm_install(),
        }
    }

    /// `I`: install the pinned tools, or cancel an installation in progress.
    fn toggle_install(&mut self) {
        if let Some(install) = &self.install {
            install.cancel.store(true, Ordering::SeqCst);
            self.notify("Cancelling the tool installation…", false);
        } else {
            self.confirm_install();
        }
    }

    fn confirm_install(&mut self) {
        if self.install.is_some() {
            return;
        }
        if installer::current_target().is_none() {
            self.notify(
                "No pinned builds exist for this platform. Choose your own executables in Settings.",
                true,
            );
            return;
        }
        let outdated = installer::outdated_components(&self.tools_dir);
        let components = if outdated.is_empty() {
            installer::manifest().components.iter().collect::<Vec<_>>()
        } else {
            outdated
        };
        let list = components
            .iter()
            .map(|component| format!("{} {}", component.name, component.version))
            .collect::<Vec<_>>()
            .join(", ");
        let size = super::format::format_bytes(Some(installer::download_size(&components) as f64));
        let folder = self.tools_dir.display();
        if self.engine() == Engine::Missing {
            self.ask(
                "Set up the download engine?",
                format!(
                    "yt-dlp TUI runs its own copy of {list}, so you don't need yt-dlp, FFmpeg, Python, or Deno installed. Setup downloads {size} from their official releases into {folder} and checks every file against its pinned SHA-256."
                ),
                "Set up",
                "Not now",
                ConfirmAction::InstallTools,
            );
        } else {
            self.ask(
                "Install the pinned tools?",
                format!(
                    "Downloads {list} ({size}) from their official releases into {folder}. Every file is checked against its pinned size and SHA-256 before it is used."
                ),
                "Install",
                "Not now",
                ConfirmAction::InstallTools,
            );
        }
    }

    fn start_install(&mut self) {
        let reinstall = installer::outdated_components(&self.tools_dir).is_empty();
        let cancel = Arc::new(AtomicBool::new(false));
        self.install = Some(Install {
            cancel: cancel.clone(),
            status: None,
        });
        if let Some(form) = self.settings_form.as_mut() {
            form.installing = true;
        }
        self.engine_changed();
        let (progress, mut events) = tokio::sync::mpsc::unbounded_channel();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                let _ = tx.send(Message::Install(event));
            }
        });
        let service = self.service.clone();
        self.spawn(async move {
            Message::Installed(
                service
                    .install_tools(reinstall, cancel, progress)
                    .await
                    .map_err(|error| error.to_string()),
            )
        });
    }

    fn select_offset(&mut self, offset: isize) {
        let visible = self.visible_jobs();
        if visible.is_empty() {
            return;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|id| visible.iter().position(|job| &job.id == id))
            .unwrap_or(0) as isize;
        let next = (current + offset).clamp(0, visible.len() as isize - 1) as usize;
        let id = visible[next].id.clone();
        if self.selected.as_ref() != Some(&id) {
            self.selected = Some(id);
            self.details_scroll = 0;
        }
    }

    fn set_filter(&mut self, filter: DownloadFilter) {
        self.filter = filter;
        self.settle_selection();
    }

    fn handle_downloads_key(&mut self, key: KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let selected = self.selected_job().cloned();
        match key.code {
            KeyCode::Char('q') => self.request_quit(),
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Char('n') | KeyCode::Char('a') => self.open_new_download(None),
            KeyCode::Char('s') | KeyCode::Char(',') => self.open_settings(),
            KeyCode::Char('b') => self.sidebar = !self.sidebar,
            KeyCode::Char('I') => self.toggle_install(),
            KeyCode::Char(digit @ '1'..='4') => {
                self.set_filter(DownloadFilter::ALL[digit as usize - '1' as usize]);
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let index = DownloadFilter::ALL
                    .iter()
                    .position(|filter| *filter == self.filter)
                    .unwrap_or(0);
                let step = if key.code == KeyCode::BackTab { 3 } else { 1 };
                self.set_filter(DownloadFilter::ALL[(index + step) % 4]);
            }
            KeyCode::Up if shift => self.reorder(selected, true),
            KeyCode::Down if shift => self.reorder(selected, false),
            KeyCode::Char('K') => self.reorder(selected, true),
            KeyCode::Char('J') => self.reorder(selected, false),
            KeyCode::Up | KeyCode::Char('k') => self.select_offset(-1),
            KeyCode::Down | KeyCode::Char('j') => self.select_offset(1),
            KeyCode::Home | KeyCode::Char('g') => self.select_offset(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.select_offset(isize::MAX / 2),
            KeyCode::PageUp => self.details_scroll = self.details_scroll.saturating_add(10),
            KeyCode::PageDown => self.details_scroll = self.details_scroll.saturating_sub(10),
            KeyCode::Enter => {
                if selected.is_some() {
                    self.details = !self.details;
                    self.details_scroll = 0;
                }
            }
            KeyCode::Esc => self.details = false,
            KeyCode::Char('p') => {
                let paused = !self.queue_paused;
                let service = self.service.clone();
                self.spawn(async move {
                    match service.set_paused(paused).await {
                        Ok(()) => Message::Paused(paused),
                        Err(error) => Message::Problem(error.to_string()),
                    }
                });
            }
            KeyCode::Char('C') => {
                let count = merge_downloads(&self.queue, &self.history)
                    .iter()
                    .filter(|job| job.status == JobStatus::Completed)
                    .count();
                if count == 0 {
                    self.notify("There are no completed downloads to clear", false);
                } else {
                    self.ask(
                        "Clear completed downloads?",
                        format!(
                            "Remove {count} completed {} from the list? The files stay on your computer.",
                            if count == 1 { "download" } else { "downloads" }
                        ),
                        "Clear list",
                        "Keep",
                        ConfirmAction::ClearCompleted,
                    );
                }
            }
            _ => {
                if let Some(job) = selected {
                    self.handle_job_key(key, job);
                }
            }
        }
    }

    fn handle_job_key(&mut self, key: KeyEvent, job: DownloadJob) {
        let service = self.service.clone();
        let id = job.id.clone();
        match key.code {
            KeyCode::Char('c') | KeyCode::Char('x')
                if job.status.is_running() || job.status == JobStatus::Queued =>
            {
                self.spawn(async move {
                    match service.cancel_job(&id).await {
                        Ok(()) => Message::Notice(String::new()),
                        Err(error) => Message::Problem(error.to_string()),
                    }
                });
            }
            KeyCode::Char('r') if job.status.is_stopped() => {
                self.spawn(async move {
                    Message::Retried {
                        result: service.retry(&id).await.map_err(|error| error.to_string()),
                        old: id,
                    }
                });
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if job.status.is_running() {
                    self.notify("Cancel the download before removing it", true);
                } else {
                    self.spawn(async move {
                        match service.remove_job(&id).await {
                            Ok(()) => Message::Removed(id),
                            Err(error) => Message::Problem(error.to_string()),
                        }
                    });
                }
            }
            KeyCode::Char(action @ ('o' | 'f')) if job.status == JobStatus::Completed => {
                self.spawn(async move {
                    let opened = async {
                        let path = service.output_path(&id).await?;
                        if action == 'o' {
                            platform::open_path(&path).await
                        } else {
                            let folder = path.parent().ok_or_else(|| {
                                crate::error::AppError::Validation(
                                    "The download folder is unavailable".into(),
                                )
                            })?;
                            platform::open_path(folder).await
                        }
                    };
                    match opened.await {
                        Ok(()) => Message::Notice(String::new()),
                        Err(error) => Message::Problem(error.to_string()),
                    }
                });
            }
            KeyCode::Char('y') => {
                copy_to_clipboard(&job.request.url);
                self.notify("Copied the source link", false);
            }
            KeyCode::Char('Y') => {
                copy_to_clipboard(&super::format::technical_report(&job));
                self.notify("Copied the technical details", false);
            }
            _ => {}
        }
    }

    fn reorder(&mut self, job: Option<DownloadJob>, up: bool) {
        let Some(job) = job.filter(|job| job.status == JobStatus::Queued) else {
            return;
        };
        let service = self.service.clone();
        self.spawn(async move {
            match service.reorder(&job.id, up).await {
                Ok(queue) => Message::Queue(queue),
                Err(error) => Message::Problem(error.to_string()),
            }
        });
    }
}

/// Copies through the terminal (OSC 52), which works over SSH too.
fn copy_to_clipboard(text: &str) {
    let mut stdout = std::io::stdout();
    let _ = write!(stdout, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let _ = stdout.flush();
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for index in 0..4 {
            if index <= chunk.len() {
                output.push(ALPHABET[(value >> (18 - 6 * index) & 63) as usize] as char);
            } else {
                output.push('=');
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DownloadOptions, DownloadProgress};

    async fn app() -> (App, tempfile::TempDir) {
        let data = tempfile::tempdir().unwrap();
        let (events, _) = tokio::sync::mpsc::unbounded_channel();
        let service = AppService::new(data.path().into(), data.path().join("tools"), events)
            .await
            .unwrap();
        let snapshot = AppSnapshot {
            settings: service.settings().await,
            queue: vec![],
            history: vec![],
            dependencies: vec![],
            hardware_acceleration: HardwareAccelerationInfo::checking(),
            queue_paused: false,
        };
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        (App::new(service, snapshot, tx), data)
    }

    fn job(status: JobStatus) -> DownloadJob {
        DownloadJob {
            id: "job".into(),
            request: DownloadRequest {
                url: "https://example.test/video".into(),
                destination: "/tmp".into(),
                filename_template: "%(title)s.%(ext)s".into(),
                is_playlist: false,
                options: DownloadOptions::default(),
            },
            title: Some("Video".into()),
            status,
            progress: DownloadProgress::default(),
            created_at: "2026-10-07T12:00:00Z".into(),
            started_at: None,
            finished_at: None,
            output_path: None,
            error_category: None,
            error_message: None,
            diagnostics: vec![],
        }
    }

    fn tools(yt_dlp: bool) -> Vec<DependencyInfo> {
        [
            DependencyKind::YtDlp,
            DependencyKind::Ffmpeg,
            DependencyKind::Ffprobe,
            DependencyKind::JavascriptRuntime,
        ]
        .into_iter()
        .map(|kind| DependencyInfo {
            kind,
            status: if yt_dlp { "available" } else { "missing" }.into(),
            source: if yt_dlp { "managed" } else { "not_found" }.into(),
            path: None,
            version: None,
            message: None,
        })
        .collect()
    }

    #[tokio::test]
    async fn offers_setup_once_when_the_engine_is_missing() {
        let (mut app, _data) = app().await;
        assert_eq!(app.engine(), Engine::Checking);
        app.handle_message(Message::Dependencies(tools(false)));
        assert_eq!(app.engine(), Engine::Missing);
        if installer::current_target().is_none() {
            return;
        }
        let confirm = app.confirm.take().expect("setup is offered by itself");
        assert_eq!(confirm.title, "Set up the download engine?");
        assert!(confirm.message.contains("don't need yt-dlp"));
        assert!(matches!(confirm.action, ConfirmAction::InstallTools));
        app.handle_message(Message::Dependencies(tools(false)));
        assert!(app.confirm.is_none(), "offered only once");
    }

    #[tokio::test]
    async fn a_link_given_at_launch_is_read_once_the_engine_is_ready() {
        let (mut app, _data) = app().await;
        app.open_new_download(Some("https://example.test/video".into()));
        let Some(Modal::NewDownload(form)) = &app.modal else {
            panic!("New download is open");
        };
        assert!(!form.analyzing, "the startup check is still running");
        app.handle_message(Message::Dependencies(tools(true)));
        let Some(Modal::NewDownload(form)) = &app.modal else {
            panic!("New download is open");
        };
        assert!(form.analyzing, "the waiting link is read");
    }

    #[tokio::test]
    async fn a_late_enqueue_result_never_hides_a_running_download() {
        let (mut app, _data) = app().await;
        app.handle_service(ServiceEvent::JobChanged(Box::new(job(
            JobStatus::Downloading,
        ))));
        app.handle_message(Message::Enqueued {
            result: Ok(job(JobStatus::Queued)),
            destination: "/tmp".into(),
        });
        assert_eq!(app.running_jobs(), 1);
        app.request_quit();
        assert!(
            !app.should_quit,
            "quitting asks first while a download runs"
        );
        assert!(app.confirm.is_some());
    }

    #[tokio::test]
    async fn finished_jobs_join_history_and_filters_follow() {
        let (mut app, _data) = app().await;
        app.handle_service(ServiceEvent::JobChanged(Box::new(job(
            JobStatus::Downloading,
        ))));
        assert_eq!(app.history.len(), 0);
        app.handle_service(ServiceEvent::JobChanged(Box::new(job(
            JobStatus::Completed,
        ))));
        assert_eq!(app.history.len(), 1);
        assert_eq!(app.visible_jobs().len(), 1);
        app.set_filter(DownloadFilter::Stopped);
        assert!(app.visible_jobs().is_empty());
        assert!(app.selected.is_none());
        app.set_filter(DownloadFilter::Completed);
        assert_eq!(app.selected.as_deref(), Some("job"));
    }

    #[test]
    fn encodes_base64_with_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"https://youtu.be/x"), "aHR0cHM6Ly95b3V0dS5iZS94");
    }
}
