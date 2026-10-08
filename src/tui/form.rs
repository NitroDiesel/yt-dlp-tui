//! A keyboard form: rows are rebuilt from model state on every frame, so
//! controls appear and disappear with the options they depend on, while focus
//! is tracked by a stable field id.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::{format::truncate, text_input::TextInput, theme::Theme};

const LABEL_WIDTH: u16 = 20;

#[derive(Debug, Clone)]
pub struct ChoiceOption {
    pub label: String,
    pub disabled: bool,
    /// Options sharing a group are listed under that heading in the picker.
    pub group: Option<String>,
}

impl ChoiceOption {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            disabled: false,
            group: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct MultiItem {
    pub label: String,
    pub checked: bool,
    pub disabled: bool,
}

#[derive(Debug, Clone)]
pub enum Control {
    /// A heading that starts a group of rows.
    Section,
    /// Read-only text in the value column.
    Note(Line<'static>),
    Choice {
        options: Vec<ChoiceOption>,
        selected: usize,
    },
    Toggle {
        text: String,
        on: bool,
    },
    Text {
        input: TextInput,
        placeholder: String,
        /// Completion shown after the cursor; Right accepts it.
        suggestion: Option<String>,
    },
    Multi {
        items: Vec<MultiItem>,
        empty: String,
    },
    Button {
        text: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Faint,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Row<Id> {
    pub id: Option<Id>,
    pub label: String,
    pub control: Control,
    pub disabled: bool,
    /// Always shown below the row.
    pub help: Option<(String, Tone)>,
    /// Shown below the row only while it has focus.
    pub hint: Option<String>,
    pub indent: u16,
}

impl<Id> Row<Id> {
    pub fn new(id: Id, label: impl Into<String>, control: Control) -> Self {
        Self {
            id: Some(id),
            label: label.into(),
            control,
            disabled: false,
            help: None,
            hint: None,
            indent: 0,
        }
    }

    pub fn section(title: impl Into<String>) -> Self {
        Self {
            id: None,
            label: title.into(),
            control: Control::Section,
            disabled: false,
            help: None,
            hint: None,
            indent: 0,
        }
    }

    pub fn note(label: impl Into<String>, text: impl Into<Line<'static>>) -> Self {
        Self {
            id: None,
            label: label.into(),
            control: Control::Note(text.into()),
            disabled: false,
            help: None,
            hint: None,
            indent: 0,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn help(mut self, text: impl Into<String>) -> Self {
        self.help = Some((text.into(), Tone::Faint));
        self
    }

    pub fn help_if(self, condition: bool, text: impl Into<String>) -> Self {
        if condition { self.help(text) } else { self }
    }

    pub fn warn(mut self, text: impl Into<String>) -> Self {
        self.help = Some((text.into(), Tone::Warning));
        self
    }

    pub fn error(mut self, text: impl Into<String>) -> Self {
        self.help = Some((text.into(), Tone::Error));
        self
    }

    pub fn hint(mut self, text: impl Into<String>) -> Self {
        self.hint = Some(text.into());
        self
    }

    pub fn indent(mut self) -> Self {
        self.indent = 2;
        self
    }

    fn focusable(&self) -> bool {
        self.id.is_some()
    }
}

pub fn choice(options: Vec<ChoiceOption>, selected: usize) -> Control {
    Control::Choice { options, selected }
}

pub fn choices(labels: &[&str], selected: usize) -> Control {
    choice(
        labels
            .iter()
            .map(|label| ChoiceOption::new(*label))
            .collect(),
        selected,
    )
}

pub fn toggle(text: impl Into<String>, on: bool) -> Control {
    Control::Toggle {
        text: text.into(),
        on,
    }
}

pub fn text(input: &TextInput, placeholder: impl Into<String>) -> Control {
    Control::Text {
        input: input.clone(),
        placeholder: placeholder.into(),
        suggestion: None,
    }
}

pub fn button(text: impl Into<String>) -> Control {
    Control::Button { text: text.into() }
}

#[derive(Debug, Clone)]
enum Popup<Id> {
    Single { id: Id, cursor: usize },
    Multi { id: Id, cursor: usize },
}

/// Focus, scroll, and the open picker for one form.
#[derive(Debug, Clone)]
pub struct FormState<Id> {
    pub focus: Option<Id>,
    focus_index: usize,
    scroll: usize,
    popup: Option<Popup<Id>>,
    /// Where the last frame drew each row, for mouse clicks.
    geometry: Option<Geometry<Id>>,
}

#[derive(Debug, Clone)]
struct Geometry<Id> {
    area: Rect,
    /// Screen line to the row drawn on it; a row's first line comes first.
    lines: Vec<(u16, Id)>,
    value_x: u16,
    value_width: usize,
    /// The open picker's area and the screen line of each option.
    popup: Option<(Rect, Vec<(u16, usize)>)>,
}

impl<Id> Default for FormState<Id> {
    fn default() -> Self {
        Self {
            focus: None,
            focus_index: 0,
            scroll: 0,
            popup: None,
            geometry: None,
        }
    }
}

impl<Id: Copy + PartialEq> FormState<Id> {
    pub fn focus(&mut self, id: Id) {
        self.focus = Some(id);
        self.popup = None;
    }

    pub fn popup_open(&self) -> bool {
        self.popup.is_some()
    }

    /// Keeps focus on an existing row after rows were added or removed.
    fn settle(&mut self, rows: &[Row<Id>]) {
        if let Some(index) = rows
            .iter()
            .position(|row| row.id.is_some() && row.id == self.focus)
        {
            self.focus_index = index;
            return;
        }
        let candidates = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.focusable())
            .collect::<Vec<_>>();
        let nearest = candidates
            .iter()
            .find(|(index, _)| *index >= self.focus_index)
            .or(candidates.last());
        if let Some((index, row)) = nearest {
            self.focus_index = *index;
            self.focus = row.id;
        } else {
            self.focus = None;
        }
    }

    fn step(&mut self, rows: &[Row<Id>], forward: bool) {
        self.settle(rows);
        let focusable = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.focusable())
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let next = if forward {
            focusable.iter().find(|index| **index > self.focus_index)
        } else {
            focusable
                .iter()
                .rev()
                .find(|index| **index < self.focus_index)
        };
        if let Some(index) = next {
            self.focus_index = *index;
            self.focus = rows[*index].id;
        }
    }
}

/// What a form model must do for the shared key handling.
pub trait FormModel {
    type Id: Copy + PartialEq;

    fn rows(&self) -> Vec<Row<Self::Id>>;
    fn form(&mut self) -> &mut FormState<Self::Id>;
    fn choose(&mut self, id: Self::Id, index: usize);
    fn toggle(&mut self, id: Self::Id);
    fn input(&mut self, id: Self::Id) -> Option<&mut TextInput>;
    fn set_item(&mut self, _id: Self::Id, _index: usize, _checked: bool) {}
    fn text_changed(&mut self, _id: Self::Id) {}
}

/// Result of a key the form did not fully handle itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormOutcome<Id> {
    Handled,
    Ignored,
    /// Enter on a text field or a button.
    Activated(Id),
}

pub fn handle_key<M: FormModel>(model: &mut M, key: KeyEvent) -> FormOutcome<M::Id> {
    let rows = model.rows();
    model.form().settle(&rows);
    if let Some(popup) = model.form().popup.clone() {
        handle_popup_key(model, &rows, popup, key);
        return FormOutcome::Handled;
    }
    let Some(id) = model.form().focus else {
        return FormOutcome::Ignored;
    };
    let Some(row) = rows.iter().find(|row| row.id == Some(id)) else {
        return FormOutcome::Ignored;
    };
    let shift_tab = key.code == KeyCode::BackTab;
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Tab | KeyCode::Down => {
            model.form().step(&rows, true);
            return FormOutcome::Handled;
        }
        KeyCode::Up if !control => {
            model.form().step(&rows, false);
            return FormOutcome::Handled;
        }
        _ if shift_tab => {
            model.form().step(&rows, false);
            return FormOutcome::Handled;
        }
        KeyCode::PageDown => {
            for _ in 0..8 {
                model.form().step(&rows, true);
            }
            return FormOutcome::Handled;
        }
        KeyCode::PageUp => {
            for _ in 0..8 {
                model.form().step(&rows, false);
            }
            return FormOutcome::Handled;
        }
        _ => {}
    }
    match &row.control {
        Control::Text {
            input, suggestion, ..
        } => {
            if key.code == KeyCode::Enter {
                return FormOutcome::Activated(id);
            }
            if row.disabled {
                return FormOutcome::Ignored;
            }
            if key.code == KeyCode::Right
                && input.cursor_at_end()
                && let Some(suggestion) = suggestion.clone()
                && let Some(target) = model.input(id)
            {
                target.insert_str(&suggestion);
                model.text_changed(id);
                return FormOutcome::Handled;
            }
            if TextInput::accepts(&key) {
                if let Some(target) = model.input(id)
                    && target.handle_key(key)
                {
                    model.text_changed(id);
                }
                return FormOutcome::Handled;
            }
            FormOutcome::Ignored
        }
        Control::Choice { options, selected } if !row.disabled => match key.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') => {
                let forward = matches!(key.code, KeyCode::Right | KeyCode::Char('l'));
                if let Some(index) = cycle(options, *selected, forward) {
                    model.choose(id, index);
                }
                FormOutcome::Handled
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                model.form().popup = Some(Popup::Single {
                    id,
                    cursor: *selected,
                });
                FormOutcome::Handled
            }
            _ => FormOutcome::Ignored,
        },
        Control::Toggle { .. } if !row.disabled => match key.code {
            KeyCode::Enter | KeyCode::Char(' ') => {
                model.toggle(id);
                FormOutcome::Handled
            }
            _ => FormOutcome::Ignored,
        },
        Control::Multi { .. } if !row.disabled => match key.code {
            KeyCode::Enter | KeyCode::Char(' ') => {
                model.form().popup = Some(Popup::Multi { id, cursor: 0 });
                FormOutcome::Handled
            }
            _ => FormOutcome::Ignored,
        },
        Control::Button { .. } if !row.disabled => match key.code {
            KeyCode::Enter | KeyCode::Char(' ') => FormOutcome::Activated(id),
            _ => FormOutcome::Ignored,
        },
        _ => FormOutcome::Ignored,
    }
}

/// Inserts pasted text into the focused text field.
pub fn paste<M: FormModel>(model: &mut M, text: &str) -> Option<M::Id> {
    let rows = model.rows();
    model.form().settle(&rows);
    let id = model.form().focus?;
    let row = rows.iter().find(|row| row.id == Some(id))?;
    if !matches!(row.control, Control::Text { .. }) || row.disabled {
        return None;
    }
    model.input(id)?.insert_str(text);
    model.text_changed(id);
    Some(id)
}

fn cycle(options: &[ChoiceOption], selected: usize, forward: bool) -> Option<usize> {
    let count = options.len();
    (1..count)
        .map(|step| {
            if forward {
                (selected + step) % count
            } else {
                (selected + count - step) % count
            }
        })
        .find(|index| !options[*index].disabled)
}

fn handle_popup_key<M: FormModel>(
    model: &mut M,
    rows: &[Row<M::Id>],
    popup: Popup<M::Id>,
    key: KeyEvent,
) {
    let (id, cursor) = match popup {
        Popup::Single { id, cursor } | Popup::Multi { id, cursor } => (id, cursor),
    };
    let Some(row) = rows.iter().find(|row| row.id == Some(id)) else {
        model.form().popup = None;
        return;
    };
    let count = match &row.control {
        Control::Choice { options, .. } => options.len(),
        Control::Multi { items, .. } => items.len(),
        _ => 0,
    };
    let move_to = |cursor: usize| match popup {
        Popup::Single { id, .. } => Popup::Single { id, cursor },
        Popup::Multi { id, .. } => Popup::Multi { id, cursor },
    };
    match key.code {
        KeyCode::Esc => model.form().popup = None,
        KeyCode::Up | KeyCode::Char('k') if cursor > 0 => {
            model.form().popup = Some(move_to(cursor - 1));
        }
        KeyCode::Down | KeyCode::Char('j') if cursor + 1 < count => {
            model.form().popup = Some(move_to(cursor + 1));
        }
        KeyCode::Home => model.form().popup = Some(move_to(0)),
        KeyCode::End => model.form().popup = Some(move_to(count.saturating_sub(1))),
        KeyCode::PageUp => model.form().popup = Some(move_to(cursor.saturating_sub(10))),
        KeyCode::PageDown => {
            model.form().popup = Some(move_to((cursor + 10).min(count.saturating_sub(1))));
        }
        KeyCode::Enter | KeyCode::Char(' ') => match (&row.control, popup) {
            (Control::Choice { options, .. }, Popup::Single { .. }) => {
                if options.get(cursor).is_some_and(|option| !option.disabled) {
                    model.choose(id, cursor);
                    model.form().popup = None;
                }
            }
            (Control::Multi { items, .. }, Popup::Multi { .. }) => {
                if key.code == KeyCode::Enter {
                    model.form().popup = None;
                } else if let Some(item) = items.get(cursor).filter(|item| !item.disabled) {
                    model.set_item(id, cursor, !item.checked);
                }
            }
            _ => model.form().popup = None,
        },
        _ => {}
    }
}

/// A left click at a screen position. Clicking a row focuses it; on the
/// row's first line a choice or list opens its picker, a toggle flips, a
/// button is pressed, and a text field moves its cursor to the click.
pub fn click<M: FormModel>(model: &mut M, column: u16, line: u16) -> FormOutcome<M::Id> {
    let rows = model.rows();
    model.form().settle(&rows);
    let Some(geometry) = model.form().geometry.clone() else {
        return FormOutcome::Ignored;
    };
    let point = Position::new(column, line);
    if let Some(popup) = model.form().popup.clone() {
        match &geometry.popup {
            Some((area, options)) if area.contains(point) => {
                if let Some((_, option)) = options.iter().find(|(y, _)| *y == line) {
                    let (popup, key) = match popup {
                        Popup::Single { id, .. } => (
                            Popup::Single {
                                id,
                                cursor: *option,
                            },
                            KeyCode::Enter,
                        ),
                        Popup::Multi { id, .. } => (
                            Popup::Multi {
                                id,
                                cursor: *option,
                            },
                            KeyCode::Char(' '),
                        ),
                    };
                    model.form().popup = Some(popup.clone());
                    handle_popup_key(model, &rows, popup, KeyEvent::new(key, KeyModifiers::NONE));
                }
            }
            _ => model.form().popup = None,
        }
        return FormOutcome::Handled;
    }
    if !geometry.area.contains(point) {
        return FormOutcome::Ignored;
    }
    let Some(&(_, id)) = geometry.lines.iter().find(|(y, _)| *y == line) else {
        return FormOutcome::Handled;
    };
    let first_line = geometry
        .lines
        .iter()
        .find(|(_, owner)| *owner == id)
        .map(|(y, _)| *y);
    model.form().focus = Some(id);
    let Some(row) = rows.iter().find(|row| row.id == Some(id)) else {
        return FormOutcome::Handled;
    };
    if row.disabled || first_line != Some(line) {
        return FormOutcome::Handled;
    }
    match &row.control {
        Control::Choice { selected, .. } => {
            model.form().popup = Some(Popup::Single {
                id,
                cursor: *selected,
            });
        }
        Control::Multi { .. } => model.form().popup = Some(Popup::Multi { id, cursor: 0 }),
        Control::Toggle { .. } => model.toggle(id),
        Control::Button { .. } => return FormOutcome::Activated(id),
        Control::Text { .. } if column >= geometry.value_x => {
            let width = geometry.value_width.saturating_sub(1);
            if let Some(input) = model.input(id) {
                input.click(width, (column - geometry.value_x) as usize);
            }
        }
        _ => {}
    }
    FormOutcome::Handled
}

/// The mouse wheel: moves through the open picker, or between rows.
pub fn scroll<M: FormModel>(model: &mut M, down: bool) {
    let key = if down { KeyCode::Down } else { KeyCode::Up };
    handle_key(model, KeyEvent::new(key, KeyModifiers::NONE));
}

/// Pointer movement: highlights the picker option under it. Returns true
/// when the highlight changed.
pub fn hover<M: FormModel>(model: &mut M, column: u16, line: u16) -> bool {
    let state = model.form();
    let (Some(popup), Some(geometry)) = (state.popup.clone(), state.geometry.as_ref()) else {
        return false;
    };
    let Some((area, options)) = &geometry.popup else {
        return false;
    };
    if !area.contains(Position::new(column, line)) {
        return false;
    }
    let Some(&(_, option)) = options.iter().find(|(y, _)| *y == line) else {
        return false;
    };
    let moved = match popup {
        Popup::Single { id, cursor } if cursor != option => Popup::Single { id, cursor: option },
        Popup::Multi { id, cursor } if cursor != option => Popup::Multi { id, cursor: option },
        _ => return false,
    };
    state.popup = Some(moved);
    true
}

/// Whether a screen position falls on this form or its open picker.
pub fn contains<M: FormModel>(model: &mut M, column: u16, line: u16) -> bool {
    let point = Position::new(column, line);
    model.form().geometry.as_ref().is_some_and(|geometry| {
        geometry.area.contains(point)
            || geometry
                .popup
                .as_ref()
                .is_some_and(|(area, _)| area.contains(point))
    })
}

fn label_width(area: Rect) -> u16 {
    if area.width < 60 { 14 } else { LABEL_WIDTH }
}

/// Draws the rows into `area`, scrolling to keep the focused row visible.
pub fn render<M: FormModel>(
    frame: &mut Frame,
    area: Rect,
    model: &mut M,
    theme: &Theme,
    active: bool,
) {
    let rows = model.rows();
    let state = model.form();
    state.settle(&rows);
    let label_column = label_width(area);
    let value_width = area.width.saturating_sub(label_column + 1) as usize;
    let mut lines: Vec<Line> = Vec::new();
    // The row each line belongs to, so a click can find it.
    let mut owners: Vec<Option<M::Id>> = Vec::new();
    let mut focus_span = (0, 0);
    let mut cursor: Option<(usize, u16)> = None;
    for (index, row) in rows.iter().enumerate() {
        let focused = active && row.id.is_some() && row.id == state.focus;
        if matches!(row.control, Control::Section) {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            lines.push(Line::from(Span::styled(row.label.clone(), theme.strong())));
            owners.resize(lines.len(), None);
            continue;
        }
        let start = lines.len();
        let label_text = format!("{}{}", " ".repeat(row.indent as usize), row.label);
        let label_style = if focused {
            theme.accent()
        } else if row.disabled {
            theme.faint()
        } else {
            theme.text()
        };
        let label = Span::styled(
            format!(
                "{:<width$} ",
                truncate(&label_text, label_column as usize),
                width = label_column as usize
            ),
            label_style,
        );
        let (value, text_cursor) = render_control(row, focused, value_width, theme);
        if let Some(column) = text_cursor {
            cursor = Some((lines.len(), label_column + 1 + column as u16));
        }
        let mut spans = vec![label];
        spans.extend(value);
        let mut line = Line::from(spans);
        if focused {
            line = line.style(theme.selected());
        }
        lines.push(line);
        let pad = " ".repeat(label_column as usize + 1);
        if let Some((help, tone)) = &row.help {
            let style = match tone {
                Tone::Faint => theme.faint(),
                Tone::Warning => theme.warning(),
                Tone::Error => theme.error(),
            };
            for wrapped in wrap(help, value_width) {
                lines.push(Line::from(vec![
                    Span::raw(pad.clone()),
                    Span::styled(wrapped, style),
                ]));
            }
        }
        if focused && let Some(hint) = &row.hint {
            for wrapped in wrap(hint, value_width) {
                lines.push(Line::from(vec![
                    Span::raw(pad.clone()),
                    Span::styled(wrapped, theme.faint()),
                ]));
            }
        }
        owners.resize(lines.len(), row.id);
        if focused {
            focus_span = (start, lines.len());
            state.focus_index = index;
        }
    }
    let height = area.height as usize;
    if focus_span.1 > state.scroll + height {
        state.scroll = focus_span.1 - height;
    }
    if focus_span.0 < state.scroll {
        state.scroll = focus_span.0;
    }
    state.scroll = state.scroll.min(lines.len().saturating_sub(height));
    let drawn = owners
        .iter()
        .enumerate()
        .skip(state.scroll)
        .take(height)
        .filter_map(|(index, owner)| owner.map(|id| (area.y + (index - state.scroll) as u16, id)))
        .collect();
    let mut geometry = Geometry {
        area,
        lines: drawn,
        value_x: area.x + label_column + 1,
        value_width,
        popup: None,
    };
    let visible = lines
        .into_iter()
        .skip(state.scroll)
        .take(height)
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(visible), area);
    if let Some((line, column)) = cursor
        && line >= state.scroll
        && line < state.scroll + height
        && state.popup.is_none()
    {
        frame.set_cursor_position(Position::new(
            area.x + column,
            area.y + (line - state.scroll) as u16,
        ));
    }
    if let Some(popup) = state.popup.clone() {
        geometry.popup = render_popup(frame, area, &rows, popup, theme);
    }
    state.geometry = Some(geometry);
}

fn render_control(
    row: &Row<impl Copy>,
    focused: bool,
    width: usize,
    theme: &Theme,
) -> (Vec<Span<'static>>, Option<usize>) {
    let base = if row.disabled {
        theme.faint()
    } else {
        theme.text()
    };
    match &row.control {
        Control::Section => (vec![], None),
        Control::Note(line) => (
            line.spans
                .iter()
                .map(|span| Span::styled(truncate(&span.content, width), span.style))
                .collect(),
            None,
        ),
        Control::Choice { options, selected } => {
            let label = options
                .get(*selected)
                .map_or(String::new(), |option| option.label.clone());
            if focused && !row.disabled {
                (
                    vec![
                        Span::styled("‹ ", theme.accent()),
                        Span::styled(truncate(&label, width.saturating_sub(4)), base),
                        Span::styled(" ›", theme.accent()),
                    ],
                    None,
                )
            } else {
                (vec![Span::styled(truncate(&label, width), base)], None)
            }
        }
        Control::Toggle { text, on } => {
            let mark = if *on { "[x] " } else { "[ ] " };
            let mark_style = if *on && !row.disabled {
                theme.accent()
            } else {
                base
            };
            (
                vec![
                    Span::styled(mark, mark_style),
                    Span::styled(truncate(text, width.saturating_sub(4)), base),
                ],
                None,
            )
        }
        Control::Text {
            input,
            placeholder,
            suggestion,
        } => {
            if input.value().is_empty() && !(focused && suggestion.is_some()) {
                let cursor = focused.then_some(0);
                return (
                    vec![Span::styled(
                        truncate(placeholder, width),
                        theme.faint().add_modifier(ratatui::style::Modifier::ITALIC),
                    )],
                    cursor,
                );
            }
            let (visible, column) = input.window(width.saturating_sub(1));
            let mut spans = vec![Span::styled(visible.clone(), base)];
            if focused
                && input.cursor_at_end()
                && let Some(suggestion) = suggestion
            {
                let room = width.saturating_sub(visible.width() + 1);
                spans.push(Span::styled(truncate(suggestion, room), theme.faint()));
            }
            (spans, focused.then_some(column))
        }
        Control::Multi { items, empty } => {
            let chosen = items
                .iter()
                .filter(|item| item.checked)
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>();
            let summary = if chosen.is_empty() {
                empty.clone()
            } else {
                chosen.join(", ")
            };
            let style = if chosen.is_empty() {
                theme.faint()
            } else {
                base
            };
            (
                vec![
                    Span::styled(truncate(&summary, width.saturating_sub(2)), style),
                    Span::styled(" …", theme.faint()),
                ],
                None,
            )
        }
        Control::Button { text } => {
            let style = if row.disabled {
                theme.faint()
            } else if focused {
                theme.key()
            } else {
                theme.accent()
            };
            (vec![Span::styled(format!("[ {text} ]"), style)], None)
        }
    }
}

/// Draws the open picker and returns its area and each option's line.
fn render_popup<Id: Copy + PartialEq>(
    frame: &mut Frame,
    area: Rect,
    rows: &[Row<Id>],
    popup: Popup<Id>,
    theme: &Theme,
) -> Option<(Rect, Vec<(u16, usize)>)> {
    let (id, cursor) = match popup {
        Popup::Single { id, cursor } | Popup::Multi { id, cursor } => (id, cursor),
    };
    let row = rows.iter().find(|row| row.id == Some(id))?;
    let mut entries: Vec<Line> = Vec::new();
    // The option each entry shows; group headings show none.
    let mut options_at: Vec<Option<usize>> = Vec::new();
    let mut cursor_line = 0;
    let mut hint = "↑↓ move · Enter choose · Esc close";
    match &row.control {
        Control::Choice { options, selected } => {
            let mut group: Option<&String> = None;
            for (index, option) in options.iter().enumerate() {
                if option.group.is_some() && option.group.as_ref() != group {
                    group = option.group.as_ref();
                    entries.push(Line::from(Span::styled(
                        option.group.clone().unwrap_or_default(),
                        theme.faint().add_modifier(ratatui::style::Modifier::BOLD),
                    )));
                    options_at.push(None);
                }
                let marker = if index == *selected { "● " } else { "  " };
                let style = if option.disabled {
                    theme.faint()
                } else {
                    theme.text()
                };
                let mut line = Line::from(vec![
                    Span::styled(marker, theme.accent()),
                    Span::styled(option.label.clone(), style),
                ]);
                if index == cursor {
                    line = line.style(theme.selected());
                    cursor_line = entries.len();
                }
                entries.push(line);
                options_at.push(Some(index));
            }
        }
        Control::Multi { items, .. } => {
            hint = "↑↓ move · Space check · Enter done";
            for (index, item) in items.iter().enumerate() {
                let mark = if item.checked { "[x] " } else { "[ ] " };
                let style = if item.disabled {
                    theme.faint()
                } else {
                    theme.text()
                };
                let mut line = Line::from(vec![
                    Span::styled(mark, theme.accent()),
                    Span::styled(item.label.clone(), style),
                ]);
                if index == cursor {
                    line = line.style(theme.selected());
                    cursor_line = entries.len();
                }
                entries.push(line);
                options_at.push(Some(index));
            }
        }
        _ => return None,
    }
    let longest = entries
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(hint.width()) as u16;
    let width = (longest + 4).min(area.width).max(20.min(area.width));
    let height = (entries.len() as u16 + 3).min(area.height);
    let popup_area = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    let inner_height = height.saturating_sub(3) as usize;
    let scroll = cursor_line.saturating_sub(inner_height.saturating_sub(1));
    let option_lines = options_at
        .iter()
        .enumerate()
        .skip(scroll)
        .take(inner_height)
        .filter_map(|(entry, option)| {
            option.map(|option| (popup_area.y + 1 + (entry - scroll) as u16, option))
        })
        .collect();
    let mut lines = entries
        .into_iter()
        .skip(scroll)
        .take(inner_height)
        .collect::<Vec<_>>();
    lines.push(Line::from(Span::styled(hint, theme.faint())));
    frame.render_widget(Clear, popup_area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.faint())
                .title(Span::styled(
                    format!(" {} ", row.label.trim()),
                    theme.strong(),
                )),
        ),
        popup_area,
    );
    Some((popup_area, option_lines))
}

/// Greedy word wrap; words longer than a line, such as paths, are split.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    let width = width.max(10);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.width() + 1 + word.width() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        for character in word.chars() {
            if current.width() + character.width().unwrap_or(0) > width {
                lines.push(std::mem::take(&mut current));
            }
            current.push(character);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Model {
        state: FormState<u8>,
        choice: usize,
        flag: bool,
        name: TextInput,
        show_extra: bool,
    }

    impl FormModel for Model {
        type Id = u8;

        fn rows(&self) -> Vec<Row<u8>> {
            let mut rows = vec![
                Row::new(1, "Choice", choices(&["A", "B", "C"], self.choice)),
                Row::new(2, "Flag", toggle("Enable", self.flag)),
            ];
            if self.show_extra {
                rows.push(Row::new(3, "Extra", button("Go")));
            }
            rows.push(Row::new(4, "Name", text(&self.name, "type")));
            rows
        }

        fn form(&mut self) -> &mut FormState<u8> {
            &mut self.state
        }

        fn choose(&mut self, _id: u8, index: usize) {
            self.choice = index;
        }

        fn toggle(&mut self, _id: u8) {
            self.flag = !self.flag;
            self.show_extra = self.flag;
        }

        fn input(&mut self, _id: u8) -> Option<&mut TextInput> {
            Some(&mut self.name)
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn cycles_choices_and_moves_focus_across_appearing_rows() {
        let mut model = Model::default();
        handle_key(&mut model, key(KeyCode::Right));
        assert_eq!(model.choice, 1);
        handle_key(&mut model, key(KeyCode::Left));
        handle_key(&mut model, key(KeyCode::Left));
        assert_eq!(model.choice, 2);

        handle_key(&mut model, key(KeyCode::Down));
        handle_key(&mut model, key(KeyCode::Char(' ')));
        assert!(model.flag);
        handle_key(&mut model, key(KeyCode::Down));
        assert_eq!(model.state.focus, Some(3));
        assert_eq!(
            handle_key(&mut model, key(KeyCode::Enter)),
            FormOutcome::Activated(3)
        );

        handle_key(&mut model, key(KeyCode::Down));
        handle_key(&mut model, key(KeyCode::Char('h')));
        handle_key(&mut model, key(KeyCode::Char('i')));
        assert_eq!(model.name.value(), "hi");
    }

    #[test]
    fn picker_selects_with_enter_and_closes_with_escape() {
        let mut model = Model::default();
        handle_key(&mut model, key(KeyCode::Enter));
        assert!(model.state.popup_open());
        handle_key(&mut model, key(KeyCode::Down));
        handle_key(&mut model, key(KeyCode::Down));
        handle_key(&mut model, key(KeyCode::Enter));
        assert_eq!(model.choice, 2);
        assert!(!model.state.popup_open());

        handle_key(&mut model, key(KeyCode::Enter));
        handle_key(&mut model, key(KeyCode::Esc));
        assert!(!model.state.popup_open());
        assert_eq!(model.choice, 2);
    }

    #[test]
    fn focus_falls_back_when_its_row_disappears() {
        let mut model = Model::default();
        handle_key(&mut model, key(KeyCode::Down));
        handle_key(&mut model, key(KeyCode::Char(' ')));
        handle_key(&mut model, key(KeyCode::Down));
        assert_eq!(model.state.focus, Some(3));
        model.toggle(2);
        handle_key(&mut model, key(KeyCode::Char('x')));
        assert_eq!(model.state.focus, Some(4));
        assert_eq!(model.name.value(), "x");
    }

    #[test]
    fn wraps_help_text_on_word_boundaries() {
        assert_eq!(
            wrap("one two three four five", 10),
            vec!["one two", "three four", "five"]
        );
        assert_eq!(
            wrap("File /home/me/Downloads/clip.webm", 12),
            vec!["File", "/home/me/Dow", "nloads/clip.", "webm"]
        );
    }
}
