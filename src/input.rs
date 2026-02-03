use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
}

impl TextInput {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.len();
        Self { value, cursor }
    }

    pub fn insert(&mut self, ch: char) {
        self.value.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.value[..self.cursor].chars().last();
        if let Some(ch) = prev {
            let new_cursor = self.cursor - ch.len_utf8();
            self.value.replace_range(new_cursor..self.cursor, "");
            self.cursor = new_cursor;
        }
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        let ch = self.value[self.cursor..].chars().next();
        if let Some(ch) = ch {
            let end = self.cursor + ch.len_utf8();
            self.value.replace_range(self.cursor..end, "");
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some(ch) = self.value[..self.cursor].chars().last() {
            self.cursor -= ch.len_utf8();
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        if let Some(ch) = self.value[self.cursor..].chars().next() {
            self.cursor += ch.len_utf8();
        }
    }

    pub fn cursor_display_offset(&self) -> usize {
        UnicodeWidthStr::width(&self.value[..self.cursor])
    }
}

#[cfg(test)]
mod tests {
    use super::TextInput;

    #[test]
    fn insert_and_backspace() {
        let mut input = TextInput::new("");
        input.insert('a');
        input.insert('b');
        input.insert('c');
        assert_eq!(input.value, "abc");
        input.backspace();
        assert_eq!(input.value, "ab");
        assert_eq!(input.cursor, 2);
    }

    #[test]
    fn delete_and_cursor_moves() {
        let mut input = TextInput::new("abcd");
        input.cursor = 2;
        input.delete();
        assert_eq!(input.value, "abd");
        input.move_left();
        assert_eq!(input.cursor, 1);
        input.move_right();
        assert_eq!(input.cursor, 2);
    }

    #[test]
    fn cursor_display_offset_matches_ascii_len() {
        let mut input = TextInput::new("hello");
        input.cursor = 3;
        assert_eq!(input.cursor_display_offset(), 3);
    }
}
