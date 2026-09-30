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

    pub fn offset(&self, cursor: Cursor) -> usize {
        self.lines
            .iter()
            .zip(&self.endings)
            .take(cursor.row)
            .map(|(line, ending)| line.len() + ending.len())
            .sum::<usize>()
            + self.byte_at(cursor)
    }

    pub fn cursor_at_offset(&self, offset: usize) -> Cursor {
        let mut base = 0;
        for (row, line) in self.lines.iter().enumerate() {
            if offset <= base + line.len() {
                return Cursor {
                    row,
                    col: line
                        .grapheme_indices(true)
                        .take_while(|(byte, _)| *byte < offset.saturating_sub(base))
                        .count(),
                };
            }
            base += line.len() + self.endings.get(row).map_or(0, String::len);
        }
        self.file_end()
    }

    pub fn previous_cursor(&self, cursor: Cursor) -> Cursor {
        if cursor.col > 0 {
            Cursor {
                col: cursor.col - 1,
                ..cursor
            }
        } else if cursor.row > 0 {
            Cursor {
                row: cursor.row - 1,
                col: self.line_len(cursor.row - 1),
            }
        } else {
            cursor
        }
    }

    pub fn next_cursor(&self, cursor: Cursor) -> Cursor {
        if cursor.col < self.line_len(cursor.row) {
            Cursor {
                col: cursor.col + 1,
                ..cursor
            }
        } else if cursor.row + 1 < self.lines.len() {
            Cursor {
                row: cursor.row + 1,
                col: 0,
            }
        } else {
            cursor
        }
    }

    pub fn normalize_insert(&self, text: &str) -> String {
        text.replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', &self.preferred_ending)
    }

    pub fn replace_text(&mut self, text: &str) {
        let preferred = self.preferred_ending.clone();
        *self = Self::from_text(text);
        self.preferred_ending = preferred;
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
    fn offsets_round_trip_unicode_and_mixed_line_endings() {
        let b = Buffer::from_text("a👩‍💻e\u{301}中\r\nlast\n");
        for row in 0..b.lines.len() {
            for col in 0..=b.line_len(row) {
                let cursor = Cursor { row, col };
                assert_eq!(b.cursor_at_offset(b.offset(cursor)), cursor);
            }
        }
        assert_eq!(
            b.range_text(Cursor { row: 0, col: 1 }, Cursor { row: 1, col: 1 }),
            "👩‍💻e\u{301}中\r\nl"
        );
    }

    #[test]
    fn replacement_keeps_preferred_endings_even_if_all_lines_are_deleted() {
        let mut b = Buffer::from_text("a\r\nb\n");
        b.replace_text("");
        assert_eq!(b.normalize_insert("X\nY\r\nZ"), "X\r\nY\r\nZ");
    }
}
