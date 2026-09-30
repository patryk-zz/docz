use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
}

/// Columns count grapheme clusters, not UTF-8 bytes or terminal cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Buffer {
    pub lines: Vec<String>,
    endings: Vec<String>,
    preferred_ending: String,
}

impl Buffer {
    pub fn from_text(text: &str) -> Self {
        let mut lines = Vec::new();
        let mut endings = Vec::new();
        for part in text.split_inclusive('\n') {
            if let Some(line) = part.strip_suffix('\n') {
                if let Some(line) = line.strip_suffix('\r') {
                    lines.push(line.to_owned());
                    endings.push("\r\n".to_owned());
                } else {
                    lines.push(line.to_owned());
                    endings.push("\n".to_owned());
                }
            } else {
                lines.push(part.to_owned());
            }
        }
        if lines.is_empty() || text.ends_with('\n') {
            lines.push(String::new());
        }
        let preferred_ending = endings.first().cloned().unwrap_or_else(|| "\n".into());
        Self {
            lines,
            endings,
            preferred_ending,
        }
    }

    pub fn text(&self) -> String {
        let mut result = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            result.push_str(line);
            if let Some(ending) = self.endings.get(i) {
                result.push_str(ending);
            }
        }
        result
    }

    pub fn line_len(&self, row: usize) -> usize {
        self.lines[row].graphemes(true).count()
    }

    pub fn byte_at(&self, cursor: Cursor) -> usize {
        self.lines[cursor.row]
            .grapheme_indices(true)
            .nth(cursor.col)
            .map_or(self.lines[cursor.row].len(), |(byte, _)| byte)
    }

    /// A half-open range of grapheme positions, including intervening line endings.
    pub fn range_text(&self, start: Cursor, end: Cursor) -> String {
        if start.row == end.row {
            return self.lines[start.row][self.byte_at(start)..self.byte_at(end)].to_owned();
        }
        let mut text = self.lines[start.row][self.byte_at(start)..].to_owned();
        for row in start.row..end.row {
            text.push_str(&self.endings[row]);
            if row + 1 < end.row {
                text.push_str(&self.lines[row + 1]);
            }
        }
        text.push_str(&self.lines[end.row][..self.byte_at(end)]);
        text
    }

    pub fn delete_range(&mut self, start: Cursor, end: Cursor) -> Cursor {
        let start_byte = self.byte_at(start);
        let end_byte = self.byte_at(end);
        if start.row == end.row {
            self.lines[start.row].replace_range(start_byte..end_byte, "");
        } else {
            let tail = self.lines[end.row][end_byte..].to_owned();
            self.lines[start.row].truncate(start_byte);
            self.lines[start.row].push_str(&tail);
            self.lines.drain(start.row + 1..=end.row);
            self.endings.drain(start.row..end.row);
        }
        // A join can merge Unicode graphemes. Keep the cursor at a valid boundary.
        Cursor {
            row: start.row,
            col: self.lines[start.row]
                .grapheme_indices(true)
                .take_while(|(byte, _)| *byte < start_byte)
                .count(),
        }
    }

    pub fn file_end(&self) -> Cursor {
        let row = self.lines.len() - 1;
        Cursor {
            row,
            col: self.line_len(row),
        }
    }

    pub fn word_starts(&self) -> Vec<Cursor> {
        let mut starts = Vec::new();
        for (row, line) in self.lines.iter().enumerate() {
            let mut in_word = false;
            for (col, grapheme) in line.graphemes(true).enumerate() {
                let whitespace = grapheme.chars().all(char::is_whitespace);
                let word = grapheme.chars().any(|c| c.is_alphanumeric() || c == '_');
                if !whitespace && (!word || !in_word) {
                    starts.push(Cursor { row, col });
                }
                in_word = word;
            }
        }
        starts
    }

    pub fn paragraph_starts(&self) -> Vec<Cursor> {
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(row, line)| {
                (!line.trim().is_empty() && (row == 0 || self.lines[row - 1].trim().is_empty()))
                    .then_some(Cursor { row, col: 0 })
            })
            .collect()
    }

    pub fn insert(&mut self, cursor: &mut Cursor, text: &str) {
        // Normalize pasted line breaks to the file's preferred ending; existing endings survive.
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        for (i, part) in normalized.split('\n').enumerate() {
            if i > 0 {
                self.newline(cursor);
            }
            let byte = self.byte_at(*cursor);
            self.lines[cursor.row].insert_str(byte, part);
            let end_byte = byte + part.len();
            // Inserting a combining character can merge neighboring graphemes.
            cursor.col = self.lines[cursor.row]
                .grapheme_indices(true)
                .take_while(|(start, _)| *start < end_byte)
                .count();
        }
    }

    pub fn newline(&mut self, cursor: &mut Cursor) {
        let byte = self.byte_at(*cursor);
        let tail = self.lines[cursor.row].split_off(byte);
        self.lines.insert(cursor.row + 1, tail);
        self.endings
            .insert(cursor.row, self.preferred_ending.clone());
        cursor.row += 1;
        cursor.col = 0;
    }

    pub fn backspace(&mut self, cursor: &mut Cursor) {
        if cursor.col > 0 {
            let end = self.byte_at(*cursor);
            let start = self.byte_at(Cursor {
                col: cursor.col - 1,
                ..*cursor
            });
            self.lines[cursor.row].replace_range(start..end, "");
            cursor.col -= 1;
            self.clamp(cursor);
        } else if cursor.row > 0 {
            let line = self.lines.remove(cursor.row);
            self.endings.remove(cursor.row - 1);
            cursor.row -= 1;
            let join_byte = self.lines[cursor.row].len();
            self.lines[cursor.row].push_str(&line);
            cursor.col = self.lines[cursor.row]
                .grapheme_indices(true)
                .take_while(|(byte, _)| *byte < join_byte)
                .count();
        }
    }

    pub fn delete(&mut self, cursor: &mut Cursor) {
        if cursor.col < self.line_len(cursor.row) {
            let start = self.byte_at(*cursor);
            let end = self.byte_at(Cursor {
                col: cursor.col + 1,
                ..*cursor
            });
            self.lines[cursor.row].replace_range(start..end, "");
            self.clamp(cursor);
        } else if cursor.row + 1 < self.lines.len() {
            let next = self.lines.remove(cursor.row + 1);
            self.endings.remove(cursor.row);
            self.lines[cursor.row].push_str(&next);
            self.clamp(cursor);
        }
    }

    pub fn clamp(&self, cursor: &mut Cursor) {
        cursor.row = cursor.row.min(self.lines.len() - 1);
        cursor.col = cursor.col.min(self.line_len(cursor.row));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_empty_trailing_and_mixed_line_endings() {
        for text in ["", "hello", "hello\n", "a\r\nb\nc\r\n", "\n\n"] {
            assert_eq!(Buffer::from_text(text).text(), text);
        }
    }

    #[test]
    fn deletes_whole_unicode_graphemes() {
        let mut b = Buffer::from_text("a👩‍💻e\u{301}中");
        let mut c = Cursor { row: 0, col: 3 };
        b.backspace(&mut c);
        assert_eq!(b.text(), "a👩‍💻中");
        c.col = 1;
        b.delete(&mut c);
        assert_eq!(b.text(), "a中");
        b.insert(&mut c, "é");
        assert_eq!(b.text(), "aé中");
    }

    #[test]
    fn split_join_and_paste_preserve_endings() {
        let mut b = Buffer::from_text("ab\r\ncd\n");
        let mut c = Cursor { row: 0, col: 1 };
        b.newline(&mut c);
        assert_eq!(b.text(), "a\r\nb\r\ncd\n");
        b.backspace(&mut c);
        assert_eq!(b.text(), "ab\r\ncd\n");
        b.insert(&mut c, "X\r\nY");
        assert_eq!(b.text(), "aX\r\nYb\r\ncd\n");
    }

    #[test]
    fn combining_insert_keeps_cursor_on_a_boundary() {
        let mut b = Buffer::from_text("e");
        let mut c = Cursor { row: 0, col: 1 };
        b.insert(&mut c, "\u{301}");
        assert_eq!(c.col, 1);
        b.backspace(&mut c);
        assert_eq!(b.text(), "");
    }
}
