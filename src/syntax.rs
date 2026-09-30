use crate::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::OnceLock,
};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, HighlightState, Theme, ThemeSet},
    parsing::{ParseState, SyntaxReference, SyntaxSet},
};

const CHECKPOINT_LINES: usize = 64;
type State = (HighlightState, ParseState);

struct Assets {
    syntaxes: SyntaxSet,
    theme: Theme,
}

fn assets() -> &'static Assets {
    static ASSETS: OnceLock<Assets> = OnceLock::new();
    ASSETS.get_or_init(|| Assets {
        syntaxes: SyntaxSet::load_defaults_newlines(),
        theme: ThemeSet::load_defaults()
            .themes
            .remove("base16-ocean.dark")
            .unwrap(),
    })
}

fn detect(path: &Path, first_line: &str) -> &'static SyntaxReference {
    let syntaxes = &assets().syntaxes;
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| syntaxes.find_syntax_by_extension(name))
        .or_else(|| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .and_then(|ext| syntaxes.find_syntax_by_extension(ext))
        })
        .or_else(|| syntaxes.find_syntax_by_first_line(first_line))
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text())
}

#[derive(Clone, Debug, PartialEq)]
pub struct SyntaxSpan {
    pub bytes: Range<usize>,
    pub style: Style,
}

/// Display-only cache; source text and undo snapshots contain no color data.
pub struct HighlightCache {
    path: PathBuf,
    syntax: &'static SyntaxReference,
    lines: Vec<Vec<SyntaxSpan>>,
    // State before rows 0, 64, 128, ...; retained across partial invalidations.
    checkpoints: Vec<State>,
    state: State,
    detect_again: bool,
    #[cfg(test)]
    parsed_lines: usize,
}

impl HighlightCache {
    pub fn new(path: &Path, buffer: &Buffer) -> Self {
        let syntax = detect(path, &buffer.lines[0]);
        let state = HighlightLines::new(syntax, &assets().theme).state();
        Self {
            path: path.to_owned(),
            syntax,
            lines: Vec::new(),
            checkpoints: vec![state.clone()],
            state,
            detect_again: false,
            #[cfg(test)]
            parsed_lines: 0,
        }
    }

    pub fn language(&self) -> &str {
        &self.syntax.name
    }

    pub fn line(&self, row: usize) -> &[SyntaxSpan] {
        self.lines.get(row).map_or(&[], Vec::as_slice)
    }

    pub fn invalidate_from(&mut self, row: usize) {
        self.detect_again |= row == 0;
        if row >= self.lines.len() {
            return;
        }
        let checkpoint = row / CHECKPOINT_LINES;
        self.lines.truncate(checkpoint * CHECKPOINT_LINES);
        self.checkpoints.truncate(checkpoint + 1);
        self.state = self.checkpoints[checkpoint].clone();
    }

    /// Parse through the viewport's bottom, including off-screen multiline context.
    pub fn ensure(&mut self, buffer: &Buffer, end: usize) {
        if self.detect_again {
            let syntax = detect(&self.path, &buffer.lines[0]);
            if !std::ptr::eq(syntax, self.syntax) {
                *self = Self::new(&self.path, buffer);
            }
            self.detect_again = false;
        }
        let end = end.min(buffer.lines.len());
        if self.lines.len() >= end || self.language() == "Plain Text" {
            return;
        }
        let (highlight, parse) = self.state.clone();
        let mut highlighter = HighlightLines::from_state(&assets().theme, highlight, parse);
        for row in self.lines.len()..end {
            let source = &buffer.lines[row];
            // Normalize only the parser input; the document retains its exact endings.
            let input = format!("{source}\n");
            let mut spans = Vec::new();
            let mut byte = 0;
            if let Ok(tokens) = highlighter.highlight_line(&input, &assets().syntaxes) {
                for (token_style, token) in tokens {
                    let next = byte + token.len();
                    if byte < source.len() {
                        let mut style = Style::default().fg(Color::Rgb(
                            token_style.foreground.r,
                            token_style.foreground.g,
                            token_style.foreground.b,
                        ));
                        if token_style.font_style.contains(FontStyle::BOLD) {
                            style = style.add_modifier(Modifier::BOLD);
                        }
                        if token_style.font_style.contains(FontStyle::ITALIC) {
                            style = style.add_modifier(Modifier::ITALIC);
                        }
                        if token_style.font_style.contains(FontStyle::UNDERLINE) {
                            style = style.add_modifier(Modifier::UNDERLINED);
                        }
                        spans.push(SyntaxSpan {
                            bytes: byte..next.min(source.len()),
                            style,
                        });
                    }
                    byte = next;
                }
            }
            self.lines.push(spans);
            #[cfg(test)]
            {
                self.parsed_lines += 1;
            }
            if (row + 1) % CHECKPOINT_LINES == 0 {
                let state = highlighter.state();
                self.checkpoints.push(state.clone());
                highlighter = HighlightLines::from_state(&assets().theme, state.0, state.1);
            }
        }
        self.state = highlighter.state();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn highlighted(path: &str, text: &str) -> (Buffer, HighlightCache) {
        let buffer = Buffer::from_text(text);
        let mut cache = HighlightCache::new(Path::new(path), &buffer);
        cache.ensure(&buffer, buffer.lines.len());
        (buffer, cache)
    }

    fn style_at(cache: &HighlightCache, buffer: &Buffer, row: usize, needle: &str) -> Style {
        let byte = buffer.lines[row].find(needle).unwrap();
        cache
            .line(row)
            .iter()
            .find(|span| span.bytes.contains(&byte))
            .unwrap()
            .style
    }

    #[test]
    fn detects_common_languages_shebangs_and_plain_text() {
        for extension in [
            "py", "rs", "js", "json", "sh", "html", "css", "c", "cpp", "md",
        ] {
            let (_, cache) = highlighted(&format!("test.{extension}"), "");
            assert_ne!(cache.language(), "Plain Text", "{extension}");
        }
        let (buffer, cache) = highlighted("script", "#!/usr/bin/env python3\nprint(42)");
        assert_eq!(cache.language(), "Python");
        assert_ne!(style_at(&cache, &buffer, 1, "42").fg, None);
        let (_, cache) = highlighted("notes.unknown", "plain text");
        assert_eq!(cache.language(), "Plain Text");
        assert!(cache.line(0).is_empty());
        let (_, cache) = highlighted("Makefile", "all:\n\techo ok");
        assert_ne!(cache.language(), "Plain Text");
    }

    #[test]
    fn python_multiline_strings_fstrings_comments_and_unfinished_code() {
        let (buffer, cache) = highlighted(
            "test.py",
            "@decorator\nasync def greet(name):\n    \"\"\"docs\n    continued\n    \"\"\"\n    text = f\"hello {name.upper()}\"\n    return 42 # comment\n    incomplete = \"unterminated",
        );
        assert_eq!(cache.language(), "Python");
        assert_eq!(
            style_at(&cache, &buffer, 2, "docs"),
            style_at(&cache, &buffer, 3, "continued")
        );
        assert_ne!(
            style_at(&cache, &buffer, 6, "return"),
            style_at(&cache, &buffer, 6, "42")
        );
        assert_ne!(
            style_at(&cache, &buffer, 6, "42"),
            style_at(&cache, &buffer, 6, "comment")
        );
        assert_ne!(
            style_at(&cache, &buffer, 5, "hello"),
            style_at(&cache, &buffer, 5, "name")
        );
        assert!(!cache.line(7).is_empty());
    }

    #[test]
    fn viewport_cache_resumes_multiline_state_and_reuses_unchanged_lines() {
        let mut buffer = Buffer::from_text(&format!(
            "\"\"\"start\n{}\"\"\"\nreturn 42\n",
            "inside\n".repeat(150)
        ));
        let mut cache = HighlightCache::new(Path::new("test.py"), &buffer);
        cache.ensure(&buffer, 80);
        cache.ensure(&buffer, 160);
        let count = cache.parsed_lines;
        cache.ensure(&buffer, 100);
        cache.ensure(&buffer, 160);
        assert_eq!(cache.parsed_lines, count);
        assert_eq!(
            style_at(&cache, &buffer, 1, "inside"),
            style_at(&cache, &buffer, 140, "inside")
        );
        buffer.lines[70] = "\"\"\"".into();
        cache.invalidate_from(70);
        cache.ensure(&buffer, buffer.lines.len());
        assert_eq!(cache.parsed_lines - count, buffer.lines.len() - 64);
        let (_, fresh) = highlighted("test.py", &buffer.text());
        assert_eq!(cache.lines, fresh.lines);

        buffer.lines.splice(
            63..67,
            ["def inserted():".to_owned(), "    return 1".to_owned()],
        );
        cache.invalidate_from(63);
        cache.ensure(&buffer, buffer.lines.len());
        let (_, fresh) = highlighted("test.py", &buffer.lines.join("\n"));
        assert_eq!(cache.lines, fresh.lines);
    }

    #[test]
    fn first_line_edits_redetect_language_and_crlf_has_identical_colors() {
        let (_, lf) = highlighted("test.py", "\"\"\"doc\ncontinued\n\"\"\"\nprint(42)");
        let (_, crlf) = highlighted("test.py", "\"\"\"doc\r\ncontinued\r\n\"\"\"\r\nprint(42)");
        assert_eq!(lf.lines, crlf.lines);
        let mut buffer = Buffer::from_text("plain\nprint(42)");
        let mut cache = HighlightCache::new(Path::new("script"), &buffer);
        cache.ensure(&buffer, 2);
        assert_eq!(cache.language(), "Plain Text");
        buffer.lines[0] = "#!/usr/bin/env python3".into();
        cache.invalidate_from(0);
        cache.ensure(&buffer, 2);
        assert_eq!(cache.language(), "Python");
        assert!(!cache.line(1).is_empty());
        buffer.lines[0] = "plain".into();
        cache.invalidate_from(0);
        cache.ensure(&buffer, 2);
        assert_eq!(cache.language(), "Plain Text");
        assert!(cache.line(1).is_empty());
    }
}
