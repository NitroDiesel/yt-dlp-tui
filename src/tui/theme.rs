use ratatui::style::{Color, Modifier, Style};

/// Semantic colors. The terminal's own background and foreground are kept;
/// only roles that carry meaning get a color, and every state also has words.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub faint: Color,
    pub selection: Color,
    /// `NO_COLOR` is set: rely on bold, dim, and reverse only.
    pub monochrome: bool,
}

impl Theme {
    pub fn new(accent: &str) -> Self {
        let monochrome = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        Self {
            accent: match accent {
                "violet" => Color::Indexed(141),
                "pink" => Color::Indexed(211),
                "graphite" => Color::Indexed(250),
                _ => Color::Indexed(75),
            },
            success: Color::Indexed(114),
            warning: Color::Indexed(179),
            error: Color::Indexed(203),
            faint: Color::Indexed(245),
            selection: Color::Indexed(237),
            monochrome,
        }
    }

    fn fg(&self, color: Color) -> Style {
        if self.monochrome {
            Style::default()
        } else {
            Style::default().fg(color)
        }
    }

    pub fn text(&self) -> Style {
        Style::default()
    }

    pub fn strong(&self) -> Style {
        Style::default().add_modifier(Modifier::BOLD)
    }

    pub fn faint(&self) -> Style {
        if self.monochrome {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            self.fg(self.faint)
        }
    }

    pub fn accent(&self) -> Style {
        if self.monochrome {
            self.strong()
        } else {
            self.fg(self.accent)
        }
    }

    pub fn success(&self) -> Style {
        self.fg(self.success)
    }

    pub fn warning(&self) -> Style {
        self.fg(self.warning)
    }

    pub fn error(&self) -> Style {
        if self.monochrome {
            self.strong()
        } else {
            self.fg(self.error)
        }
    }

    /// The highlighted row in a list or form.
    pub fn selected(&self) -> Style {
        if self.monochrome {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().bg(self.selection)
        }
    }

    /// A key name inside a hint, such as the `n` in "n new".
    pub fn key(&self) -> Style {
        self.accent().add_modifier(Modifier::BOLD)
    }
}
