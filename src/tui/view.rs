//! Drawing. Layout follows a conventional download manager: filters on the
//! left, the downloads table in the middle, details on the right, and the
//! transfer totals plus key hints along the bottom.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::{
    app::{App, Modal, Screen},
    form,
    format::{
        DownloadFilter, clip_summary, format_bytes, format_duration, format_eta, format_summary,
        format_timestamp, format_when, hostname, job_meta, job_title, merge_downloads, percent_of,
        status_label, truncate,
    },
    mouse::{Click, Hits, Layer, span_area},
    new_download::Engine,
    theme::Theme,
};
use crate::{
    domain::{DependencyKind, DownloadJob, JobStatus, MediaMode},
    integration::installer,
};

const SIDEBAR_WIDTH: u16 = 26;
/// Main-area width from which details sit beside the list instead of over it.
pub const SIDE_BY_SIDE: u16 = 90;

pub fn render(frame: &mut Frame, app: &mut App) {
    let mut hits = Hits::default();
    let area = frame.area();
    let [body, status, hints] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let show_sidebar = app.sidebar && area.width >= 80;
    let main = if show_sidebar {
        let [sidebar, rule, main] = Layout::horizontal([
            Constraint::Length(SIDEBAR_WIDTH),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(body);
        render_sidebar(frame, sidebar, app, &mut hits);
        render_rule(frame, rule, &app.theme);
        main
    } else {
        body
    };
    app.main_width = main.width;
    match app.screen {
        Screen::Downloads => render_downloads(frame, main, app, &mut hits),
        Screen::Settings => render_settings(frame, main, app, &mut hits),
    }
    render_status(frame, status, app, &mut hits);
    render_hints(frame, hints, app, &mut hits);
    match app.modal.as_mut() {
        Some(Modal::NewDownload(form)) => {
            let theme = app.theme;
            render_new_download(frame, body, form, &theme, &mut hits)
        }
        Some(Modal::Help) => render_help(frame, body, &app.theme),
        None => {}
    }
    if app.confirm.is_some() {
        render_confirm(frame, body, app, &mut hits);
    }
    highlight_hover(frame, app, &hits);
    app.hits = hits;
}

/// Underlines the clickable item under the pointer.
fn highlight_hover(frame: &mut Frame, app: &App, hits: &Hits) {
    let Some(hovered) = &app.hovered else {
        return;
    };
    let layers = app.open_layers();
    for hit in hits
        .items
        .iter()
        .filter(|hit| hit.hover && layers.contains(&hit.layer) && &hit.click == hovered)
    {
        frame.buffer_mut().set_style(
            hit.area,
            Style::default().add_modifier(Modifier::UNDERLINED),
        );
    }
}

fn render_rule(frame: &mut Frame, area: Rect, theme: &Theme) {
    let lines = (0..area.height)
        .map(|_| Line::from(Span::styled("│", theme.faint())))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_sidebar(frame: &mut Frame, area: Rect, app: &App, hits: &mut Hits) {
    let theme = &app.theme;
    let width = area.width as usize;
    let all = merge_downloads(&app.queue, &app.history);
    let count =
        |filter: DownloadFilter| all.iter().filter(|job| filter.includes(job.status)).count();
    let entry = |label: &str, trailing: String, active: bool| {
        let gap = width.saturating_sub(label.width() + trailing.width() + 2);
        let line = Line::from(vec![
            Span::styled(
                format!(" {label}"),
                if active { theme.accent() } else { theme.text() },
            ),
            Span::raw(" ".repeat(gap)),
            Span::styled(format!("{trailing} "), theme.faint()),
        ]);
        if active {
            line.style(theme.selected())
        } else {
            line
        }
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(" yt-dlp ", theme.strong()),
            Span::styled("TUI", theme.accent()),
        ]),
        Line::default(),
        entry(
            "+ New download",
            "n".into(),
            matches!(app.modal, Some(Modal::NewDownload(_))),
        ),
        Line::default(),
    ];
    for (index, filter) in DownloadFilter::ALL.iter().enumerate() {
        let total = count(*filter);
        let trailing = if total > 0 {
            format!("{total}")
        } else {
            format!("{}", index + 1)
        };
        let style_trailing = if total > 0 { trailing } else { String::new() };
        lines.push(entry(
            filter.label(),
            style_trailing,
            app.screen == Screen::Downloads && app.filter == *filter,
        ));
    }
    lines.push(Line::default());
    lines.push(entry(
        "Settings",
        "s".into(),
        app.screen == Screen::Settings,
    ));
    frame.render_widget(Paragraph::new(lines), area);
    // The whole row clicks; the label underlines on hover.
    let mut clickable = |index: usize, label: &str, click: Click| {
        let y = area.y + index as u16;
        if y < area.bottom() {
            hits.push_plain(
                Rect::new(area.x, y, area.width, 1),
                click.clone(),
                Layer::Base,
            );
            hits.push(
                span_area(area.x + 1, y, label.width() as u16, area),
                click,
                Layer::Base,
            );
        }
    };
    clickable(2, "+ New download", Click::NewDownload);
    for (index, filter) in DownloadFilter::ALL.iter().enumerate() {
        clickable(4 + index, filter.label(), Click::Filter(*filter));
    }
    clickable(9, "Settings", Click::Settings);
}

fn render_downloads(frame: &mut Frame, area: Rect, app: &App, hits: &mut Hits) {
    let wide = area.width >= SIDE_BY_SIDE;
    let selected = app.selected_job();
    let (list_area, details_area) = match (app.details, selected) {
        (true, Some(_)) if wide => {
            let details_width = (area.width / 3).clamp(36, 52);
            let [list, rule, details] = Layout::horizontal([
                Constraint::Min(40),
                Constraint::Length(1),
                Constraint::Length(details_width),
            ])
            .areas(area);
            render_rule(frame, rule, &app.theme);
            (Some(list), Some(details))
        }
        (true, Some(_)) => (None, Some(area)),
        _ => (Some(area), None),
    };
    if let Some(list) = list_area {
        hits.list = Some(list);
        render_list(frame, list, app, hits);
    }
    if let (Some(details), Some(job)) = (details_area, selected) {
        hits.details = Some(details);
        render_details(frame, inset(details, 1), job, app, hits);
    }
}

fn inset(area: Rect, horizontal: u16) -> Rect {
    Rect {
        x: area.x + horizontal,
        width: area.width.saturating_sub(horizontal * 2),
        ..area
    }
}

/// A message above the list when the download engine needs attention.
fn engine_banner(app: &App) -> Option<(String, Style)> {
    let theme = &app.theme;
    if let Some(install) = &app.install {
        let status = install.status.as_ref().map_or_else(
            || "Preparing the tool installation…".to_string(),
            |status| status.describe(),
        );
        return Some((format!("{status}  I cancels"), theme.accent()));
    }
    let missing = app.engine() == Engine::Missing;
    if app.settings.yt_dlp_path.is_none() && installer::current_target().is_some() {
        let outdated = installer::outdated_components(&app.tools_dir);
        let installed = installer::installed_versions(&app.tools_dir);
        let size = format_bytes(Some(installer::download_size(&outdated) as f64));
        if missing {
            return Some((
                format!(
                    "The download engine isn't set up yet. Press I to install the app's own copy of yt-dlp, Deno, and FFmpeg ({size}). Nothing else needs to be installed."
                ),
                theme.warning(),
            ));
        }
        if !outdated.is_empty() {
            let names = outdated
                .iter()
                .map(|component| match installed.get(&component.id) {
                    Some(old) => format!("{} {old} → {}", component.name, component.version),
                    None => format!("{} {}", component.name, component.version),
                })
                .collect::<Vec<_>>()
                .join(", ");
            return Some((
                format!("Pinned tools to install: {names} ({size}). Press I to install them."),
                theme.warning(),
            ));
        }
    }
    if missing {
        return Some((
            "The chosen yt-dlp isn't working. Open Settings (s) to check the download engine."
                .into(),
            theme.error(),
        ));
    }
    None
}

const COLUMNS: [(&str, u16); 5] = [
    ("Size", 9),
    ("Progress", 18),
    ("Status", 16),
    ("Speed", 11),
    ("ETA", 8),
];

fn render_list(frame: &mut Frame, area: Rect, app: &App, hits: &mut Hits) {
    let theme = &app.theme;
    let area = inset(area, 1);
    let jobs = app.visible_jobs();
    let mut lines = Vec::new();
    let mut header = vec![
        Span::styled(app.filter.label(), theme.strong()),
        Span::styled(format!("  {}", jobs.len()), theme.faint()),
    ];
    if app.queue_paused {
        header.push(Span::styled("   Queue paused · p resumes", theme.warning()));
    }
    lines.push(Line::from(header));
    if let Some((banner, style)) = engine_banner(app) {
        for line in form::wrap(&banner, area.width as usize) {
            lines.push(Line::from(Span::styled(line, style)));
        }
    }
    lines.push(Line::default());

    // Narrow terminals drop columns from the right edge inward.
    let mut columns = COLUMNS.to_vec();
    while !columns.is_empty()
        && area.width < 30 + columns.iter().map(|(_, width)| width + 1).sum::<u16>()
    {
        let drop = ["ETA", "Speed", "Size", "Status"]
            .iter()
            .find(|name| columns.iter().any(|(column, _)| column == *name));
        match drop {
            Some(name) => columns.retain(|(column, _)| column != name),
            None => break,
        }
    }
    let fixed = columns.iter().map(|(_, width)| width + 1).sum::<u16>();
    let name_width = area.width.saturating_sub(fixed) as usize;
    let mut column_header = vec![Span::styled(
        format!("{:<name_width$}", "Name"),
        theme.faint(),
    )];
    for (name, width) in &columns {
        column_header.push(Span::styled(
            format!(" {:<width$}", name, width = *width as usize),
            theme.faint(),
        ));
    }
    lines.push(Line::from(column_header));

    if jobs.is_empty() {
        lines.push(Line::default());
        let empty = match app.filter {
            DownloadFilter::All => vec![
                Line::from(Span::styled("Paste a link to start", theme.strong())),
                Line::from(Span::styled(
                    "Copy a video, playlist, or channel link and paste it here, or press n for a new download.",
                    theme.faint(),
                )),
            ],
            DownloadFilter::Downloading => vec![Line::from(Span::styled(
                "Nothing is downloading or waiting right now.",
                theme.faint(),
            ))],
            DownloadFilter::Completed => vec![Line::from(Span::styled(
                "Finished downloads appear here. Removing one never deletes its file.",
                theme.faint(),
            ))],
            DownloadFilter::Stopped => vec![Line::from(Span::styled(
                "Failed, cancelled, and interrupted downloads appear here for retry.",
                theme.faint(),
            ))],
        };
        for line in empty {
            for wrapped in form::wrap(&line.to_string(), area.width as usize) {
                lines.push(Line::from(Span::styled(
                    wrapped,
                    line.style.patch(line.spans[0].style),
                )));
            }
        }
        frame.render_widget(Paragraph::new(lines), area);
        return;
    }

    let header_height = lines.len();
    let rows_height = (area.height as usize).saturating_sub(header_height);
    let per_page = (rows_height / 2).max(1);
    let selected_index = app
        .selected
        .as_ref()
        .and_then(|id| jobs.iter().position(|job| &job.id == id))
        .unwrap_or(0);
    let first = selected_index.saturating_sub(per_page.saturating_sub(1));
    for job in jobs.iter().skip(first).take(per_page) {
        let is_selected = app.selected.as_ref() == Some(&job.id);
        let y = area.y + lines.len() as u16;
        if y < area.bottom() {
            let height = (area.bottom() - y).min(2);
            hits.push_plain(
                Rect::new(area.x, y, area.width, height),
                Click::Job(job.id.clone()),
                Layer::Base,
            );
        }
        let (top, bottom) = job_lines(job, name_width, &columns, theme);
        if is_selected {
            lines.push(top.style(theme.selected()));
            lines.push(bottom.style(theme.selected()));
        } else {
            lines.push(top);
            lines.push(bottom);
        }
    }
    frame.render_widget(Paragraph::new(lines), area);
}

pub fn status_style(status: JobStatus, theme: &Theme) -> Style {
    match status {
        JobStatus::Downloading | JobStatus::Analyzing | JobStatus::PostProcessing => theme.accent(),
        JobStatus::Completed => theme.success(),
        JobStatus::Failed => theme.error(),
        JobStatus::Interrupted => theme.warning(),
        JobStatus::Queued | JobStatus::Cancelled => theme.faint(),
    }
}

fn bar(percent: f64, width: usize, status: JobStatus, theme: &Theme) -> Vec<Span<'static>> {
    let filled = ((percent / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let style = match status {
        JobStatus::Completed => theme.success(),
        JobStatus::Failed => theme.error(),
        JobStatus::Queued | JobStatus::Cancelled | JobStatus::Interrupted => theme.faint(),
        _ => theme.accent(),
    };
    vec![
        Span::styled("━".repeat(filled), style),
        Span::styled("─".repeat(width - filled), theme.faint()),
    ]
}

fn job_lines(
    job: &DownloadJob,
    name_width: usize,
    columns: &[(&str, u16)],
    theme: &Theme,
) -> (Line<'static>, Line<'static>) {
    let kind = if job.request.is_playlist {
        "▤ "
    } else if job.request.options.mode == MediaMode::Audio {
        "♪ "
    } else {
        "▶ "
    };
    let title = truncate(&job_title(job), name_width.saturating_sub(3));
    let mut top = vec![
        Span::styled(kind, theme.faint()),
        Span::styled(
            format!("{:<width$}", title, width = name_width.saturating_sub(2)),
            theme.text(),
        ),
    ];
    let detail = match job.status {
        JobStatus::Failed | JobStatus::Interrupted => job
            .error_message
            .clone()
            .unwrap_or_else(|| "Download failed".into()),
        JobStatus::PostProcessing => job
            .progress
            .stage
            .clone()
            .unwrap_or_else(|| "Processing media".into()),
        _ => job_meta(job),
    };
    let detail_style = if job.status.is_stopped() && job.status != JobStatus::Cancelled {
        theme.error()
    } else {
        theme.faint()
    };
    let mut bottom = vec![
        Span::raw("  "),
        Span::styled(
            format!(
                "{:<width$}",
                truncate(&detail, name_width.saturating_sub(3)),
                width = name_width.saturating_sub(2)
            ),
            detail_style,
        ),
    ];
    let progress = &job.progress;
    let percent = percent_of(job);
    let downloading = job.status == JobStatus::Downloading;
    for (name, width) in columns {
        let width = *width as usize;
        match *name {
            "Size" => {
                let size = progress
                    .total_bytes
                    .map(|bytes| format_bytes(Some(bytes as f64)))
                    .unwrap_or_default();
                top.push(Span::styled(format!(" {size:<width$}"), theme.faint()));
                bottom.push(Span::raw(" ".repeat(width + 1)));
            }
            "Progress" => {
                top.push(Span::raw(" "));
                top.extend(bar(percent, width, job.status, theme));
                let numbers = format!(
                    "{:.*}%",
                    usize::from(downloading && percent < 10.0),
                    percent
                );
                let done = match (progress.downloaded_bytes, job.status) {
                    (Some(bytes), status) if status != JobStatus::Completed => {
                        format_bytes(Some(bytes as f64))
                    }
                    _ => progress
                        .total_bytes
                        .map(|bytes| format_bytes(Some(bytes as f64)))
                        .unwrap_or_default(),
                };
                let gap = width.saturating_sub(numbers.width() + done.width());
                bottom.push(Span::styled(
                    format!(" {numbers}{}{done}", " ".repeat(gap)),
                    theme.faint(),
                ));
            }
            "Status" => {
                top.push(Span::styled(
                    format!(" {:<width$}", status_label(job.status)),
                    status_style(job.status, theme),
                ));
                let when = if job.status.is_running() || job.status == JobStatus::Queued {
                    String::new()
                } else {
                    format_when(job.finished_at.as_deref())
                };
                bottom.push(Span::styled(format!(" {when:<width$}"), theme.faint()));
            }
            "Speed" => {
                let speed = progress
                    .speed_bytes_per_second
                    .filter(|_| downloading)
                    .map(|speed| format!("{}/s", format_bytes(Some(speed))))
                    .unwrap_or_default();
                top.push(Span::styled(format!(" {speed:<width$}"), theme.text()));
                bottom.push(Span::raw(" ".repeat(width + 1)));
            }
            _ => {
                let eta = progress
                    .eta_seconds
                    .filter(|_| downloading)
                    .map(format_duration)
                    .unwrap_or_default();
                top.push(Span::styled(format!(" {eta:<width$}"), theme.text()));
                bottom.push(Span::raw(" ".repeat(width + 1)));
            }
        }
    }
    (Line::from(top), Line::from(bottom))
}

fn render_details(frame: &mut Frame, area: Rect, job: &DownloadJob, app: &App, hits: &mut Hits) {
    let theme = &app.theme;
    let width = area.width as usize;
    let status_width = status_label(job.status).width() as u16;
    hits.push(
        span_area(area.x + status_width + 3, area.y, 10, area),
        Click::key(crossterm::event::KeyCode::Esc),
        Layer::Base,
    );
    let mut lines = vec![Line::from(vec![
        Span::styled(
            status_label(job.status),
            status_style(job.status, theme).add_modifier(ratatui::style::Modifier::BOLD),
        ),
        Span::styled("   Esc closes", theme.faint()),
    ])];
    for line in form::wrap(&job_title(job), width) {
        lines.push(Line::from(Span::styled(line, theme.strong())));
    }
    lines.push(Line::from(Span::styled(
        hostname(&job.request.url),
        theme.faint(),
    )));
    // Buttons for what the keys do, wrapped to the panel width.
    let mut actions: Vec<(&str, char)> = Vec::new();
    if job.status.is_running() || job.status == JobStatus::Queued {
        actions.push(("Cancel", 'c'));
    }
    if job.status == JobStatus::Queued {
        actions.extend([("Move up", 'K'), ("Move down", 'J')]);
    }
    if job.status.is_stopped() {
        actions.push(("Retry", 'r'));
    }
    if job.status == JobStatus::Completed {
        actions.extend([("Open file", 'o'), ("Show in folder", 'f')]);
    }
    actions.push(("Copy link", 'y'));
    if !job.diagnostics.is_empty() || job.error_message.is_some() {
        actions.push(("Copy details", 'Y'));
    }
    if !job.status.is_running() {
        actions.push(("Remove", 'd'));
    }
    let mut row: Vec<Span> = Vec::new();
    let mut used = 0;
    for (label, key) in actions {
        let text = format!("[ {label} ]");
        let button_width = text.width();
        if used > 0 && used + 1 + button_width > width {
            lines.push(Line::from(std::mem::take(&mut row)));
            used = 0;
        }
        if used > 0 {
            row.push(Span::raw(" "));
            used += 1;
        }
        hits.push(
            span_area(
                area.x + used as u16,
                area.y + lines.len() as u16,
                button_width as u16,
                area,
            ),
            Click::char(key),
            Layer::Base,
        );
        row.push(Span::styled(text, theme.accent()));
        used += button_width;
    }
    if !row.is_empty() {
        lines.push(Line::from(row));
    }
    lines.push(Line::default());

    let progress = &job.progress;
    if job.status.is_running() {
        if matches!(job.status, JobStatus::PostProcessing | JobStatus::Analyzing) {
            lines.push(Line::from(
                progress
                    .stage
                    .clone()
                    .unwrap_or_else(|| "Preparing the media file".into()),
            ));
        } else {
            let percent = progress.percent.unwrap_or(0.0).clamp(0.0, 100.0);
            let speed = progress
                .speed_bytes_per_second
                .map(|speed| format!("{}/s", format_bytes(Some(speed))))
                .unwrap_or_else(|| "Connecting…".into());
            let percent_text = format!("{:.*}%", usize::from(percent < 10.0), percent);
            let gap = width.saturating_sub(percent_text.width() + speed.width());
            lines.push(Line::from(vec![
                Span::styled(percent_text, theme.strong()),
                Span::raw(" ".repeat(gap)),
                Span::raw(speed),
            ]));
            lines.push(Line::from(bar(percent, width, job.status, theme)));
            let amount = match progress.total_bytes {
                Some(total) => format!(
                    "{} of {}",
                    format_bytes(progress.downloaded_bytes.map(|bytes| bytes as f64)),
                    format_bytes(Some(total as f64))
                ),
                None => format!(
                    "{} so far",
                    format_bytes(progress.downloaded_bytes.map(|bytes| bytes as f64))
                ),
            };
            let eta = format_eta(progress.eta_seconds);
            let gap = width.saturating_sub(amount.width() + eta.width());
            lines.push(Line::from(vec![
                Span::styled(amount, theme.faint()),
                Span::raw(" ".repeat(gap)),
                Span::styled(eta, theme.faint()),
            ]));
            if let Some(count) = progress.playlist_count {
                lines.push(Line::from(Span::styled(
                    format!("Item {} of {count}", progress.playlist_index.unwrap_or(1)),
                    theme.faint(),
                )));
            }
        }
        lines.push(Line::default());
    }

    let callout = match job.status {
        JobStatus::Failed => Some((
            job.error_category
                .as_deref()
                .map(|category| {
                    let text = category.replace('_', " ");
                    let mut characters = text.chars();
                    characters
                        .next()
                        .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
                        .unwrap_or_default()
                })
                .unwrap_or_else(|| "Download failed".into()),
            job.error_message
                .clone()
                .unwrap_or_else(|| "Review the technical details below.".into()),
            theme.error(),
        )),
        JobStatus::Interrupted => Some((
            "Stopped when the app closed".into(),
            "Press r to retry. Partial data is kept so yt-dlp can resume.".into(),
            theme.warning(),
        )),
        JobStatus::Cancelled => Some((
            "Cancelled".into(),
            "Partial files were kept where it was safe to do so.".into(),
            theme.faint(),
        )),
        _ => None,
    };
    if let Some((title, message, style)) = callout {
        lines.push(Line::from(Span::styled(
            title,
            style.add_modifier(ratatui::style::Modifier::BOLD),
        )));
        for line in form::wrap(&message, width) {
            lines.push(Line::from(Span::styled(line, style)));
        }
        lines.push(Line::default());
    }

    let mut fact = |label: &str, value: String| {
        let label_width = 10;
        let wrapped = form::wrap(&value, width.saturating_sub(label_width));
        for (index, line) in wrapped.into_iter().enumerate() {
            let label = if index == 0 { label } else { "" };
            lines.push(Line::from(vec![
                Span::styled(format!("{label:<label_width$}"), theme.faint()),
                Span::raw(line),
            ]));
        }
    };
    fact("Format", format_summary(job));
    if let Some(clip) = clip_summary(job) {
        fact("Clip", clip);
    }
    if job.request.is_playlist {
        fact(
            "Items",
            job.request
                .options
                .playlist_items
                .clone()
                .unwrap_or_else(|| "Whole playlist".into()),
        );
    }
    if let Some(conversion) = &job.request.options.video_conversion {
        fact(
            "GPU",
            format!(
                "{}, quality {}",
                conversion.codec.label(),
                conversion.quality
            ),
        );
    }
    match &job.output_path {
        Some(path) => fact("File", path.clone()),
        None => fact("Folder", job.request.destination.clone()),
    }
    fact("Added", format_timestamp(&job.created_at));
    if let Some(finished) = &job.finished_at {
        fact("Finished", format_timestamp(finished));
    }

    if !job.diagnostics.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled("Technical details", theme.strong()),
            Span::styled("  PgUp/PgDn scroll", theme.faint()),
        ]));
        let room = (area.height as usize).saturating_sub(lines.len()).max(1);
        let total = job.diagnostics.len();
        let scroll = (app.details_scroll as usize).min(total.saturating_sub(room));
        let end = total - scroll;
        let start = end.saturating_sub(room);
        for line in &job.diagnostics[start..end] {
            lines.push(Line::from(Span::styled(
                truncate(line, width),
                theme.faint(),
            )));
        }
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_settings(frame: &mut Frame, area: Rect, app: &mut App, hits: &mut Hits) {
    let theme = app.theme;
    let Some(form) = app.settings_form.as_mut() else {
        return;
    };
    let area = inset(area, 1);
    let dirty = form.is_dirty();
    let strip = dirty || form.error.is_some() || form.saving;
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(if strip { 2 } else { 0 }),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("Settings", theme.strong()))),
        header,
    );
    form::render(
        frame,
        body,
        form,
        &form.theme.clone(),
        app.confirm.is_none(),
    );
    if strip {
        let line = if form.saving {
            Line::from(Span::styled("Saving…", theme.faint()))
        } else if let Some(error) = &form.error {
            Line::from(Span::styled(
                format!("Settings couldn't be saved: {error}"),
                theme.error(),
            ))
        } else {
            let y = footer.y + 1;
            hits.push(
                span_area(footer.x + 27, y, 11, footer),
                Click::ctrl('s'),
                Layer::Base,
            );
            hits.push(
                span_area(footer.x + 40, y, 11, footer),
                Click::key(crossterm::event::KeyCode::Esc),
                Layer::Base,
            );
            Line::from(vec![
                Span::styled("You have unsaved changes.  ", theme.warning()),
                Span::styled("Ctrl+S", theme.key()),
                Span::styled(" save  ", theme.faint()),
                Span::styled("Esc", theme.key()),
                Span::styled(" discard", theme.faint()),
            ])
        };
        frame.render_widget(
            Paragraph::new(vec![Line::default(), line]).wrap(Wrap { trim: true }),
            footer,
        );
    }
}

fn render_status(frame: &mut Frame, area: Rect, app: &App, hits: &mut Hits) {
    let theme = &app.theme;
    let (mut speed, mut active, mut waiting) = (0.0, 0, 0);
    for job in &app.queue {
        if job.status == JobStatus::Queued {
            waiting += 1;
        } else if job.status.is_running() {
            active += 1;
            speed += job.progress.speed_bytes_per_second.unwrap_or(0.0);
        }
    }
    let all = merge_downloads(&app.queue, &app.history);
    let completed = all
        .iter()
        .filter(|job| job.status == JobStatus::Completed)
        .count();
    let stopped = all.iter().filter(|job| job.status.is_stopped()).count();
    let mut left = vec![
        Span::styled(format!(" ↓ {}/s", format_bytes(Some(speed))), theme.text()),
        Span::styled(
            format!(
                "   Active {active}  Waiting {waiting}  Completed {completed}  Stopped {stopped}"
            ),
            theme.faint(),
        ),
    ];
    if app.queue_paused {
        left.push(Span::styled("   Queue paused", theme.warning()));
    }
    let (dot, label) = match app.engine() {
        _ if app.install.is_some() => (theme.accent(), "Setting up engine"),
        Engine::Checking => (theme.faint(), "Checking engine"),
        Engine::Ready if tool_ok(app, DependencyKind::Ffmpeg) => (theme.success(), "Engine ready"),
        Engine::Ready => (theme.warning(), "FFmpeg missing"),
        Engine::Missing => (theme.warning(), "Engine not set up"),
    };
    let right = vec![
        Span::styled("● ", dot),
        Span::styled(format!("{label} "), theme.faint()),
    ];
    let right_width = Line::from(right.clone()).width() as u16;
    let engine_click = if app.engine() == Engine::Missing && app.install.is_none() {
        Click::char('I')
    } else {
        Click::Settings
    };
    hits.push(
        span_area(
            area.right().saturating_sub(right_width),
            area.y,
            right_width,
            area,
        ),
        engine_click,
        Layer::Base,
    );
    let used = Line::from(left.clone()).width() + Line::from(right.clone()).width();
    left.push(Span::raw(
        " ".repeat((area.width as usize).saturating_sub(used)),
    ));
    left.extend(right);
    frame.render_widget(Paragraph::new(Line::from(left)), area);
}

fn tool_ok(app: &App, kind: DependencyKind) -> bool {
    super::settings::tool_ready(&app.dependencies, kind)
}

/// Key hints for the current context; a notice replaces them briefly.
fn render_hints(frame: &mut Frame, area: Rect, app: &App, hits: &mut Hits) {
    let theme = &app.theme;
    if let Some(toast) = &app.toast {
        let style = if toast.error {
            theme.error()
        } else {
            theme.accent()
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(
                    " {}",
                    truncate(&toast.text, area.width.saturating_sub(1) as usize)
                ),
                style,
            ))),
            area,
        );
        return;
    }
    let hints: Vec<(&str, &str)> = if app.confirm.is_some() {
        vec![("Enter", "confirm"), ("←→", "switch"), ("Esc", "cancel")]
    } else {
        match app.modal.as_ref() {
            Some(Modal::Help) => vec![("any key", "close")],
            Some(Modal::NewDownload(form)) => {
                let mut hints = vec![("↑↓", "move"), ("←→", "change"), ("Enter", "open")];
                if form.probe.is_some() {
                    hints.extend([("Ctrl+D", "download"), ("Ctrl+Q", "add to queue")]);
                } else {
                    hints.push(("Ctrl+D", "read link"));
                }
                hints.push(("Esc", "close"));
                hints
            }
            None if app.screen == Screen::Settings => vec![
                ("↑↓", "move"),
                ("←→", "change"),
                ("Ctrl+S", "save"),
                ("Esc", "back"),
            ],
            None => {
                let mut hints = vec![("n", "new")];
                if app.engine() == Engine::Missing {
                    hints.push(("I", "set up engine"));
                }
                if let Some(job) = app.selected_job() {
                    hints.push(("Enter", "details"));
                    if job.status.is_running() || job.status == JobStatus::Queued {
                        hints.push(("c", "cancel"));
                    }
                    if job.status == JobStatus::Queued {
                        hints.push(("K/J", "move"));
                    }
                    if job.status.is_stopped() {
                        hints.push(("r", "retry"));
                    }
                    if job.status == JobStatus::Completed {
                        hints.extend([("o", "open"), ("f", "folder")]);
                    }
                    if !job.status.is_running() {
                        hints.push(("d", "remove"));
                    }
                }
                hints.push(("p", if app.queue_paused { "resume" } else { "pause" }));
                hints.extend([("s", "settings"), ("?", "help"), ("q", "quit")]);
                hints
            }
        }
    };
    let mut spans = vec![Span::raw(" ")];
    let mut used = 1;
    for (key, label) in hints {
        let width = key.width() + label.width() + 3;
        if used + width > area.width as usize {
            break;
        }
        if let Some(click) = Click::for_hint(key) {
            let label_width = (key.width() + 1 + label.width()) as u16;
            hits.push(
                span_area(area.x + used as u16, area.y, label_width, area),
                click,
                Layer::Bar,
            );
        }
        used += width;
        spans.push(Span::styled(key, theme.key()));
        spans.push(Span::styled(format!(" {label}  "), theme.faint()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn panel(title: &str, theme: &Theme) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(theme.faint())
        .title(Span::styled(format!(" {title} "), theme.strong()))
}

fn render_new_download(
    frame: &mut Frame,
    area: Rect,
    form: &mut super::new_download::NewDownload,
    theme: &Theme,
    hits: &mut Hits,
) {
    let width = area.width.saturating_sub(4).min(104);
    let height = area.height.saturating_sub(2).max(10);
    let outer = centered(area, width, height);
    frame.render_widget(Clear, outer);
    let block = panel("New download", theme)
        .title_bottom(Line::from(Span::styled(" Esc closes ", theme.faint())).right_aligned());
    let inner = block.inner(outer);
    frame.render_widget(block, outer);
    hits.push(
        span_area(
            outer.right().saturating_sub(13),
            outer.bottom() - 1,
            12,
            outer,
        ),
        Click::key(crossterm::event::KeyCode::Esc),
        Layer::Modal,
    );
    let inner = inset(inner, 1);
    let error = form.error.clone();
    let [body, footer] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(if error.is_some() { 2 } else { 0 }),
    ])
    .areas(inner);
    form::render(frame, body, form, theme, true);
    if let Some(error) = error {
        frame.render_widget(
            Paragraph::new(vec![
                Line::default(),
                Line::from(Span::styled(format!("⚠ {error}"), theme.error())),
            ]),
            footer,
        );
    }
}

fn render_confirm(frame: &mut Frame, area: Rect, app: &App, hits: &mut Hits) {
    let Some(confirm) = &app.confirm else {
        return;
    };
    let theme = &app.theme;
    let width = 64.min(area.width.saturating_sub(4)).max(30);
    let text_width = width.saturating_sub(4) as usize;
    let message = form::wrap(&confirm.message, text_width);
    let height = message.len() as u16 + 6;
    let outer = centered(area, width, height);
    frame.render_widget(Clear, outer);
    let block = panel(&confirm.title, theme);
    let inner = inset(block.inner(outer), 1);
    frame.render_widget(block, outer);
    let button = |label: &str, focused: bool| {
        Span::styled(
            format!("[ {label} ]"),
            if focused {
                theme.key().patch(theme.selected())
            } else {
                theme.faint()
            },
        )
    };
    let buttons_y = inner.y + message.len() as u16 + 2;
    let confirm_width = confirm.confirm.width() as u16 + 4;
    hits.push(
        span_area(inner.x, buttons_y, confirm_width, inner),
        Click::Confirm(true),
        Layer::Confirm,
    );
    hits.push(
        span_area(
            inner.x + confirm_width + 2,
            buttons_y,
            confirm.cancel.width() as u16 + 4,
            inner,
        ),
        Click::Confirm(false),
        Layer::Confirm,
    );
    let mut lines = vec![Line::default()];
    lines.extend(message.into_iter().map(Line::from));
    lines.push(Line::default());
    lines.push(Line::from(vec![
        button(&confirm.confirm, confirm.confirm_focused),
        Span::raw("  "),
        button(&confirm.cancel, !confirm.confirm_focused),
    ]));
    frame.render_widget(Paragraph::new(lines), inner);
}

/// One list of keys, grouped by where they work.
pub const KEYS: &[(&str, &[(&str, &str)])] = &[
    (
        "Downloads",
        &[
            ("n", "New download (or paste a link anywhere)"),
            ("↑↓ j k", "Select a download"),
            ("Enter / Esc", "Show or hide details"),
            ("1–4, Tab", "All, Downloading, Completed, Stopped"),
            ("c", "Cancel a running or waiting download"),
            ("r", "Retry a stopped download"),
            ("d", "Remove from the list (never deletes files)"),
            ("o / f", "Open the file / show it in its folder"),
            ("y", "Copy the source link"),
            ("Y", "Copy the technical details, for a bug report"),
            ("K / J", "Move a waiting download up or down"),
            ("p", "Pause or resume the queue"),
            ("C", "Clear completed downloads from the list"),
            ("I", "Install or update the pinned tools"),
            ("b", "Hide or show the sidebar"),
            ("s", "Settings"),
            ("q", "Quit"),
        ],
    ),
    (
        "Forms",
        &[
            ("↑↓ Tab", "Move between fields"),
            ("←→", "Change a choice"),
            ("Enter / Space", "Open a list, toggle, or press a button"),
            ("→ at the end", "Accept a suggested folder"),
            ("Ctrl+D", "Download now (reads the link first if needed)"),
            ("Ctrl+Q", "Add to the queue"),
            ("Ctrl+S", "Save settings"),
            ("Ctrl+U / Ctrl+W", "Clear field / delete a word"),
        ],
    ),
    (
        "Mouse",
        &[
            ("Click", "Sidebar, rows, buttons, fields, and key hints"),
            ("Double-click", "Show a download's details"),
            ("Wheel", "Scroll the list, details, forms, and pickers"),
            ("Shift+drag", "Select text in the terminal"),
        ],
    ),
];

fn render_help(frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines = Vec::new();
    for (index, (group, keys)) in KEYS.iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        lines.push(Line::from(Span::styled(*group, theme.strong())));
        for (key, description) in *keys {
            lines.push(Line::from(vec![
                Span::styled(format!("  {key:<18}"), theme.key()),
                Span::raw(*description),
            ]));
        }
    }
    let height = lines.len() as u16 + 2;
    let outer = centered(area, 72, height);
    frame.render_widget(Clear, outer);
    let block = panel("Keys", theme);
    let inner = inset(block.inner(outer), 1);
    frame.render_widget(block, outer);
    frame.render_widget(Paragraph::new(lines), inner);
}
