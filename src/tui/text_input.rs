use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthChar;

/// A single-line editable value with a cursor, using readline-style keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    value: String,
    /// Cursor position in characters, from 0 to the value's length.
    cursor: usize,
}

impl TextInput {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self { value, cursor }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn trimmed(&self) -> Option<String> {
        let value = self.value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    }

    pub fn set(&mut self, value: impl Into<String>) {
        *self = Self::new(value);
    }

    pub fn cursor_at_end(&self) -> bool {
        self.cursor == self.value.chars().count()
    }

    /// Inserts pasted or typed text; line breaks and control characters become spaces.
    pub fn insert_str(&mut self, text: &str) {
        let clean = text
            .chars()
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .collect::<String>();
        let index = self.byte_index(self.cursor);
        self.value.insert_str(index, &clean);
        self.cursor += clean.chars().count();
    }

    /// Applies an editing key. Returns true when the value changed.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let before = self.value.clone();
        match key.code {
            KeyCode::Char(character) if !control && !alt => self.insert_str(&character.to_string()),
            KeyCode::Char('a') if control => self.cursor = 0,
            KeyCode::Char('e') if control => self.cursor = self.len(),
            KeyCode::Char('u') if control => {
                let index = self.byte_index(self.cursor);
                self.value.drain(..index);
                self.cursor = 0;
            }
            KeyCode::Char('k') if control => {
                let index = self.byte_index(self.cursor);
                self.value.truncate(index);
            }
            KeyCode::Char('w') if control => self.delete_word_before(),
            KeyCode::Backspace if alt || control => self.delete_word_before(),
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    let start = self.byte_index(self.cursor - 1);
                    let end = self.byte_index(self.cursor);
                    self.value.drain(start..end);
                    self.cursor -= 1;
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.len() {
                    let start = self.byte_index(self.cursor);
                    let end = self.byte_index(self.cursor + 1);
                    self.value.drain(start..end);
                }
            }
            KeyCode::Left if control || alt => self.cursor = self.word_start_before(),
            KeyCode::Right if control || alt => self.cursor = self.word_end_after(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.len(),
            _ => {}
        }
        self.value != before
    }

    /// Whether `handle_key` would consume this key, so callers can let
    /// unrelated keys (Tab, arrows up and down, Enter) move focus instead.
    pub fn accepts(key: &KeyEvent) -> bool {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char(_) if !control && !alt => true,
            KeyCode::Char('a' | 'e' | 'u' | 'k' | 'w') if control => true,
            KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End => true,
            _ => false,
        }
    }

    /// The visible slice of the value for `width` columns, keeping the
    /// cursor in view, and the cursor's column within that slice.
    pub fn window(&self, width: usize) -> (String, usize) {
        let characters = self.value.chars().collect::<Vec<_>>();
        let width_of =
            |range: &[char]| -> usize { range.iter().map(|c| c.width().unwrap_or(0)).sum() };
        let width = width.max(1);
        let mut start = 0;
        while start < self.cursor && width_of(&characters[start..self.cursor]) >= width {
            start += 1;
        }
        let mut visible = String::new();
        let mut used = 0;
        for character in &characters[start..] {
            let character_width = character.width().unwrap_or(0);
            if used + character_width > width {
                break;
            }
            used += character_width;
            visible.push(*character);
        }
        (visible, width_of(&characters[start..self.cursor]))
    }

    fn len(&self) -> usize {
        self.value.chars().count()
    }

    fn byte_index(&self, character_index: usize) -> usize {
        self.value
            .char_indices()
            .nth(character_index)
            .map_or(self.value.len(), |(index, _)| index)
    }

    fn word_start_before(&self) -> usize {
        let characters = self.value.chars().collect::<Vec<_>>();
        let mut index = self.cursor;
        while index > 0 && !is_word(characters[index - 1]) {
            index -= 1;
        }
        while index > 0 && is_word(characters[index - 1]) {
            index -= 1;
        }
        index
    }

    fn word_end_after(&self) -> usize {
        let characters = self.value.chars().collect::<Vec<_>>();
        let mut index = self.cursor;
        while index < characters.len() && !is_word(characters[index]) {
            index += 1;
        }
        while index < characters.len() && is_word(characters[index]) {
            index += 1;
        }
        index
    }

    fn delete_word_before(&mut self) {
        let start = self.word_start_before();
        let (from, to) = (self.byte_index(start), self.byte_index(self.cursor));
        self.value.drain(from..to);
        self.cursor = start;
    }
}

fn is_word(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn control(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
    }

    #[test]
    fn edits_at_the_cursor_with_unicode() {
        let mut input = TextInput::new("café");
        input.handle_key(key(KeyCode::Left));
        input.handle_key(key(KeyCode::Backspace));
        assert_eq!(input.value(), "caé");
        input.handle_key(key(KeyCode::Char('f')));
        assert_eq!(input.value(), "café");
        input.handle_key(key(KeyCode::Delete));
        assert_eq!(input.value(), "caf");
        assert!(input.cursor_at_end());
    }

    #[test]
    fn supports_readline_shortcuts() {
        let mut input = TextInput::new("/home/me/Videos");
        input.handle_key(control('w'));
        assert_eq!(input.value(), "/home/me/");
        input.handle_key(control('a'));
        input.handle_key(control('k'));
        assert_eq!(input.value(), "");
        input.insert_str("one\ntwo");
        assert_eq!(input.value(), "one two");
        input.handle_key(control('u'));
        assert_eq!(input.value(), "");
    }

    #[test]
    fn keeps_the_cursor_visible_in_a_narrow_window() {
        let input = TextInput::new("abcdefghij");
        let (visible, cursor) = input.window(4);
        assert_eq!(visible, "hij");
        assert_eq!(cursor, 3);

        let mut start = input.clone();
        start.handle_key(key(KeyCode::Home));
        assert_eq!(start.window(4), ("abcd".to_string(), 0));
    }
}
