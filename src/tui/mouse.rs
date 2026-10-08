//! Mouse support. Views record the clickable areas they draw each frame;
//! a click is matched against the areas of the topmost open layer, so
//! nothing under a dialog can be clicked.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect};

use super::format::DownloadFilter;

/// What a click does. Most actions reuse the key that does the same thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Click {
    Key(KeyEvent),
    NewDownload,
    Filter(DownloadFilter),
    Settings,
    Job(String),
    /// A confirmation dialog button: true confirms.
    Confirm(bool),
}

impl Click {
    pub fn key(code: KeyCode) -> Self {
        Self::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    pub fn char(character: char) -> Self {
        Self::key(KeyCode::Char(character))
    }

    pub fn ctrl(character: char) -> Self {
        Self::Key(KeyEvent::new(
            KeyCode::Char(character),
            KeyModifiers::CONTROL,
        ))
    }

    /// The click for a key hint such as "Ctrl+D" or "n", if it is one key.
    pub fn for_hint(key: &str) -> Option<Self> {
        Some(match key {
            "Enter" => Self::key(KeyCode::Enter),
            "Esc" => Self::key(KeyCode::Esc),
            "Ctrl+D" => Self::ctrl('d'),
            "Ctrl+Q" => Self::ctrl('q'),
            "Ctrl+S" => Self::ctrl('s'),
            key if key.chars().count() == 1 => Self::char(key.chars().next()?),
            _ => return None,
        })
    }
}

/// Drawing order, bottom to top. Only the topmost open layer (plus the key
/// hint bar under a form) takes clicks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Base,
    Bar,
    Modal,
    Confirm,
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub area: Rect,
    pub click: Click,
    pub layer: Layer,
    /// Underlined while the pointer is over it.
    pub hover: bool,
}

/// Clickable areas and scroll regions recorded while drawing one frame.
#[derive(Debug, Clone, Default)]
pub struct Hits {
    pub items: Vec<Hit>,
    pub list: Option<Rect>,
    pub details: Option<Rect>,
}

impl Hits {
    pub fn push(&mut self, area: Rect, click: Click, layer: Layer) {
        self.items.push(Hit {
            area,
            click,
            layer,
            hover: true,
        });
    }

    /// An area that reacts to clicks but isn't highlighted, such as a list row.
    pub fn push_plain(&mut self, area: Rect, click: Click, layer: Layer) {
        self.items.push(Hit {
            area,
            click,
            layer,
            hover: false,
        });
    }

    pub fn at(&self, column: u16, row: u16, layers: &[Layer]) -> Option<&Hit> {
        self.items.iter().rev().find(|hit| {
            layers.contains(&hit.layer) && hit.area.contains(Position::new(column, row))
        })
    }
}

/// A one-line area starting at `x` on line `y`, clipped to `within`.
pub fn span_area(x: u16, y: u16, width: u16, within: Rect) -> Rect {
    let right = (x + width).min(within.right());
    Rect {
        x,
        y,
        width: right.saturating_sub(x),
        height: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_map_to_the_keys_they_name() {
        assert_eq!(Click::for_hint("n"), Some(Click::char('n')));
        assert_eq!(Click::for_hint("Ctrl+D"), Some(Click::ctrl('d')));
        assert_eq!(Click::for_hint("Esc"), Some(Click::key(KeyCode::Esc)));
        assert_eq!(Click::for_hint("↑↓"), None);
        assert_eq!(Click::for_hint("K/J"), None);
    }

    #[test]
    fn the_topmost_hit_of_an_open_layer_wins() {
        let mut hits = Hits::default();
        let area = Rect::new(0, 0, 10, 1);
        hits.push(area, Click::char('a'), Layer::Base);
        hits.push(area, Click::char('b'), Layer::Modal);
        assert_eq!(
            hits.at(3, 0, &[Layer::Base]).unwrap().click,
            Click::char('a')
        );
        assert_eq!(
            hits.at(3, 0, &[Layer::Base, Layer::Modal]).unwrap().click,
            Click::char('b')
        );
        assert!(hits.at(3, 1, &[Layer::Base]).is_none());
    }

    #[test]
    fn spans_are_clipped_to_their_area() {
        let within = Rect::new(0, 0, 20, 5);
        assert_eq!(span_area(15, 2, 10, within), Rect::new(15, 2, 5, 1));
    }
}
