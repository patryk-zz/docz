use crate::{
    app::{Action, App, Screen},
    browser::Browser,
    buffer::Cursor,
    editor::{Editor, Mode},
    syntax::SyntaxSpan,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [header, main, status, hints] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let (mode, color, title) = match &app.screen {
        Screen::Browser(b) => ("EXPLORE", ACCENT, b.directory.display().to_string()),
        Screen::Editor(e) => (
            match e.mode {
                Mode::Navigate => "NAVIGATE",
                Mode::Edit => "EDIT",
                Mode::Selection => "SELECT",
            },
            match e.mode {
                Mode::Navigate => ACCENT,
                Mode::Edit => Color::Green,
                Mode::Selection => Color::Yellow,
            },
            format!(
                "{}{}",
                e.document.path.display(),
                if e.document.dirty { " [+]" } else { "" }
            ),
        ),
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", crate::program_name()),
                Style::default()
                    .fg(Color::Black)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {mode} "),
                Style::default().fg(Color::Black).bg(color),
            ),
            Span::raw(format!(" {}", safe_text(&title))),
        ])),
        header,
    );
    match &mut app.screen {
        Screen::Browser(browser) => draw_browser(frame, main, browser),
        Screen::Editor(editor) => {
            draw_editor(frame, main, editor, !app.help && app.confirmation.is_none())
        }
    }
    let status_text = if !app.message.is_empty() {
        safe_text(&app.message)
    } else {
        match &app.screen {
            Screen::Browser(b) => format!(
                " {} entries · folders first · hidden files included",
                b.entries.len()
            ),
            Screen::Editor(e) => format!(
                " {} · Ln {}, Col {} · {} · UTF-8 · {} lines · {} cursor(s){}",
                if e.document.dirty {
                    "Modified"
                } else {
                    "Saved / unmodified"
                },
                e.cursor.row + 1,
                e.cursor.col + 1,
                e.highlighting.language(),
                e.document.buffer.lines.len(),
                e.cursors().count(),
                if e.selection_range().is_some() {
                    if e.mode == Mode::Edit {
                        " · typing replaces selection"
                    } else {
                        " · selection active"
                    }
                } else {
                    ""
                }
            ),
        }
    };
    frame.render_widget(
        Paragraph::new(status_text).style(Style::default().fg(color)),
        status,
    );
    let hint = match &app.screen {
        Screen::Browser(_) => {
            " W/S: select  Shift: sprint  D/Enter: open  A/Q: parent  Ctrl+Q: quit  F1: help"
        }
        Screen::Editor(e) if e.mode == Mode::Navigate => {
            " WASD: move  Shift: words/paragraphs  E: edit  F: select  Ctrl+S: save  Ctrl+Q: quit  F1: help"
        }
        Screen::Editor(e) if e.mode == Mode::Selection => {
            " WASD: select  Shift: words/paragraphs  Ctrl+C/X/V: copy/cut/paste  E: edit  Q/Esc: cancel"
        }
        Screen::Editor(_) => {
            " Ctrl+[ / ]: add cursors  Ctrl+\\: one cursor  Quotes/brackets: pair  Esc: navigate  F1: help"
        }
    };
    frame.render_widget(Paragraph::new(hint).style(Style::default().fg(DIM)), hints);
    if app.help {
        draw_help(frame);
    }
    if let Some(action) = app.confirmation {
        let area = popup(frame.area(), 64, 7);
        frame.render_widget(Clear, area);
        let verb = match action {
            Action::Quit => "quitting",
            Action::Browse => "opening the file explorer",
        };
        frame.render_widget(Paragraph::new(format!(
            "You have unsaved changes.\nBefore {verb}:\n\ns: save and continue   d: discard   Q/Esc: cancel"
        )).wrap(Wrap { trim: false }).block(Block::bordered().title(" Unsaved changes ")
            .border_style(Style::default().fg(Color::Yellow))), area);
    }
}

fn draw_browser(frame: &mut Frame, area: Rect, browser: &mut Browser) {
    let block = Block::bordered()
        .title(" Choose a file ")
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let height = usize::from(inner.height);
    if height == 0 {
        return;
    }
    if browser.selected < browser.scroll {
        browser.scroll = browser.selected;
    }
    if browser.selected >= browser.scroll + height {
        browser.scroll = browser.selected + 1 - height;
    }
    browser.scroll = browser
        .scroll
        .min(browser.entries.len().saturating_sub(height));
    for (offset, entry) in browser
        .entries
        .iter()
        .skip(browser.scroll)
        .take(height)
        .enumerate()
    {
        let selected = browser.scroll + offset == browser.selected;
        let text = format!(
            " {} {}{}",
            if selected { ">" } else { " " },
            if entry.is_dir { "[dir] " } else { "      " },
            safe_text(&entry.label)
        );
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD)
        } else if entry.is_dir {
            Style::default().fg(ACCENT)
        } else {
            Style::default()
        };
        frame.render_widget(
            Paragraph::new(text).style(style),
            Rect::new(inner.x, inner.y + offset as u16, inner.width, 1),
        );
    }
    if browser.entries.is_empty() {
        frame.render_widget(Paragraph::new(" Empty directory"), inner);
    }
}

fn draw_editor(frame: &mut Frame, area: Rect, editor: &mut Editor, show_cursor: bool) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Buffer ")
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let height = usize::from(inner.height);
    let gutter =
        (editor.document.buffer.lines.len().to_string().len() + 2).min(usize::from(inner.width));
    let width = usize::from(inner.width).saturating_sub(gutter);
    editor.page_height = height.max(1);
    if height == 0 || width == 0 {
        return;
    }
    if editor.cursor.row < editor.top {
        editor.top = editor.cursor.row;
    }
    if editor.cursor.row >= editor.top + height {
        editor.top = editor.cursor.row + 1 - height;
    }
    editor.top = editor
        .top
        .min(editor.document.buffer.lines.len().saturating_sub(height));
    let first = editor.cursors().map(|c| c.row).min().unwrap();
    let last = editor.cursors().map(|c| c.row).max().unwrap();
    if last - first < height {
        if first < editor.top {
            editor.top = first;
        }
        if last >= editor.top + height {
            editor.top = last + 1 - height;
        }
    }
    let line = &editor.document.buffer.lines[editor.cursor.row];
    let byte = editor.document.buffer.byte_at(editor.cursor);
    let cursor_x = display_text(&line[..byte]).width();
    if cursor_x < editor.left {
        editor.left = cursor_x;
    }
    if cursor_x >= editor.left + width {
        editor.left = cursor_x + 1 - width;
    }
    editor
        .highlighting
        .ensure(&editor.document.buffer, editor.top + height);
    for offset in 0..height {
        let row = editor.top + offset;
        let y = inner.y + offset as u16;
        if let Some(text) = editor.document.buffer.lines.get(row) {
            frame.render_widget(
                Paragraph::new(format!("{:>digits$} ", row + 1, digits = gutter - 1)).style(
                    Style::default().fg(if row == editor.cursor.row {
                        ACCENT
                    } else {
                        DIM
                    }),
                ),
                Rect::new(inner.x, y, gutter as u16, 1),
            );
            frame.render_widget(
                Paragraph::new(selected_line(
                    text,
                    row,
                    editor.selection_range(),
                    editor.left,
                    width,
                    editor.highlighting.line(row),
                )),
                Rect::new(inner.x + gutter as u16, y, width as u16, 1),
            );
        } else {
            frame.render_widget(
                Paragraph::new("~").style(Style::default().fg(DIM)),
                Rect::new(inner.x, y, 1, 1),
            );
        }
    }
    if show_cursor {
        for cursor in editor.cursors().skip(1) {
            if cursor.row < editor.top || cursor.row >= editor.top + height {
                continue;
            }
            let text = &editor.document.buffer.lines[cursor.row];
            let byte = editor.document.buffer.byte_at(cursor);
            let x = display_text(&text[..byte]).width();
            if x < editor.left || x >= editor.left + width {
                continue;
            }
            frame.buffer_mut()[(
                inner.x + gutter as u16 + (x - editor.left) as u16,
                inner.y + (cursor.row - editor.top) as u16,
            )]
                .set_style(Style::default().fg(Color::Black).bg(Color::Magenta));
        }
        frame.set_cursor_position((
            inner.x + gutter as u16 + (cursor_x - editor.left) as u16,
            inner.y + (editor.cursor.row - editor.top) as u16,
        ));
    }
}

/// Never send file contents or names containing terminal controls to the terminal.
fn safe_text(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}

fn display_text(text: &str) -> String {
    let mut result = String::new();
    let mut column = 0;
    for grapheme in text.graphemes(true) {
        if grapheme == "\t" {
            let count = 4 - column % 4;
            result.push_str(&" ".repeat(count));
            column += count;
        } else {
            let safe = safe_text(grapheme);
            column += safe.width();
            result.push_str(&safe);
        }
    }
    result
}

fn selected_line(
    text: &str,
    row: usize,
    selection: Option<(Cursor, Cursor)>,
    left: usize,
    width: usize,
    syntax: &[SyntaxSpan],
) -> Line<'static> {
    let mut spans = Vec::new();
    let mut column = 0;
    let right = left + width;
    let graphemes = text.graphemes(true).collect::<Vec<_>>();
    let mut byte = 0;
    let mut token = 0;
    for (col, grapheme) in graphemes
        .iter()
        .copied()
        .chain(std::iter::once(" "))
        .enumerate()
    {
        let position = Cursor { row, col };
        let selected = selection.is_some_and(|(start, end)| start <= position && position < end);
        // The extra cell shows selected line breaks, including on otherwise empty lines.
        if col == graphemes.len() && !selected {
            break;
        }
        let display = if grapheme == "\t" {
            " ".repeat(4 - column % 4)
        } else {
            safe_text(grapheme)
        };
        let end = column + display.width();
        if column >= right {
            break;
        }
        if end > left {
            let visible = if column < left || end > right {
                " ".repeat(end.min(right) - column.max(left))
            } else {
                display
            };
            let style = if selected {
                Style::default().bg(Color::Blue).fg(Color::White)
            } else {
                while token < syntax.len() && syntax[token].bytes.end <= byte {
                    token += 1;
                }
                syntax
                    .get(token)
                    .filter(|span| span.bytes.contains(&byte))
                    .map_or(Style::default(), |span| span.style)
            };
            spans.push(Span::styled(visible, style));
        }
        column = end;
        byte += grapheme.len();
    }
    Line::from(spans)
}

fn popup(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

fn draw_help(frame: &mut Frame) {
    let area = popup(frame.area(), 78, 25);
    frame.render_widget(Clear, area);
    let help = "NAVIGATE   WASD / arrows move · E edits · F selects\n\
        SPRINT     Shift+A/D: words · Shift+W/S: paragraphs\n\n\
        SELECT     Movement extends from the fixed anchor.\n\
                   Ctrl+C copies · Ctrl+X cuts and returns to Navigate\n\
                   Ctrl+V replaces selection · Backspace/Delete removes it\n\
                   E retains selection; first input replaces it.\n\
                   Q/Esc clears selection without changing text.\n\n\
        EDIT       Type normally · Enter/Tab insert newline/tab\n\
                   Ctrl+[ adds above · Ctrl+] adds below\n\
                   Ctrl+\\ returns to the primary cursor\n\
                   Ctrl+Up/Down also add cursors (legacy-terminal fallback).\n\
                   Quotes/brackets pair; closers skip · Backspace deletes pairs\n\
                   Esc returns to Navigate; q types normally.\n\n\
        SHARED     Ctrl+Z undo · Ctrl+Shift+Z redo (Ctrl+Y fallback)\n\
                   Ctrl+S save · Ctrl+Q quit · Ctrl+E file explorer\n\
                   Home/End line bounds · Ctrl+Home/End file bounds\n\
                   PageUp/Down one screen\n\n\
        EXPLORER   W/S selects · D/Enter opens · A/Q/Backspace: parent\n\
        F1, Q or Esc closes help. Unsaved changes prompt before leaving.";
    frame.render_widget(
        Paragraph::new(help).wrap(Wrap { trim: false }).block(
            Block::bordered()
                .title(" docz · Help ")
                .border_style(Style::default().fg(ACCENT)),
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn tabs_unicode_and_partial_wide_glyphs_use_terminal_cells() {
        assert_eq!(display_text("a\t中"), "a   中");
        assert_eq!(display_text("\u{1b}[31m"), "�[31m");
        assert_eq!(
            selected_line("a中bc", 0, None, 2, 3, &[]).to_string(),
            " bc"
        );
        assert_eq!(selected_line("中", 0, None, 0, 1, &[]).to_string(), " ");
    }

    #[test]
    fn syntax_colors_preserve_unicode_clipping_and_selection_cursor_overlays() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.py");
        std::fs::write(&path, "def f():\n\treturn \"中e\u{301}\"\n\treturn 42").unwrap();
        let mut app = App::new(Some(&path)).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let screen = terminal.backend().buffer();
        assert!(matches!(screen[(4, 2)].fg, Color::Rgb(..)));
        assert_ne!(screen[(4, 2)].fg, screen[(8, 2)].fg);
        assert_eq!(screen[(16, 3)].symbol(), "中");
        assert_eq!(screen[(18, 3)].symbol(), "e\u{301}");
        let Screen::Editor(e) = &mut app.screen else {
            panic!()
        };
        let line = &e.document.buffer.lines[1];
        let clipped = selected_line(line, 1, None, 13, 4, e.highlighting.line(1));
        assert_eq!(clipped.to_string(), " e\u{301}\"");
        assert!(
            clipped
                .spans
                .iter()
                .all(|span| matches!(span.style.fg, Some(Color::Rgb(..))))
        );
        e.cursor = Cursor { row: 1, col: 1 };
        e.enter_edit();
        e.add_cursor(false);
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert_eq!(terminal.backend().buffer()[(8, 4)].bg, Color::Magenta);
        let Screen::Editor(e) = &mut app.screen else {
            panic!()
        };
        e.navigate();
        e.enter_selection();
        e.move_cursor(crate::input::Direction::Right, 6);
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let screen = terminal.backend().buffer();
        assert_eq!(screen[(8, 3)].bg, Color::Blue);
        assert_eq!(screen[(8, 3)].fg, Color::White);
        assert!(matches!(screen[(16, 3)].fg, Color::Rgb(..)));
    }

    #[test]
    fn renders_tiny_terminals_and_keeps_scrolled_cursor_visible() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, "a\t中abc\n".repeat(100)).unwrap();
        let mut app = App::new(Some(&path)).unwrap();
        let Screen::Editor(e) = &mut app.screen else {
            panic!()
        };
        e.cursor.row = 90;
        e.cursor.col = 6;
        for (width, height) in [(80, 24), (12, 6), (1, 1), (0, 0)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            app.help = true;
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            app.help = false;
        }
    }

    #[test]
    fn selection_highlights_tabs_wide_characters_and_line_breaks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, "a\t中x\n\nlast").unwrap();
        let mut app = App::new(Some(&path)).unwrap();
        let Screen::Editor(e) = &mut app.screen else {
            panic!()
        };
        e.cursor.col = 1;
        e.enter_selection();
        e.cursor = Cursor { row: 2, col: 1 };
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        assert_ne!(buffer[(4, 2)].bg, Color::Blue); // Unselected "a".
        // Ratatui resets the continuation cell of a wide glyph; the leading cell paints it.
        assert_eq!(buffer[(8, 2)].symbol(), "中");
        for x in [5, 6, 7, 8, 10, 11] {
            assert_eq!(buffer[(x, 2)].bg, Color::Blue);
        }
        assert_eq!(buffer[(4, 3)].bg, Color::Blue); // Selected blank line's newline.
        assert_eq!(buffer[(4, 4)].bg, Color::Blue); // Selected "l".
        assert_ne!(buffer[(5, 4)].bg, Color::Blue); // Unselected "a".
    }

    #[test]
    fn secondary_cursors_are_visible_at_unicode_cell_positions_and_hidden_in_help() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, "\t中x\n\te\u{301}x\n\ta").unwrap();
        let mut app = App::new(Some(&path)).unwrap();
        let Screen::Editor(e) = &mut app.screen else {
            panic!()
        };
        e.cursor.col = 1;
        e.enter_edit();
        e.add_cursor(false);
        e.add_cursor(false);
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(8, 3)].bg, Color::Magenta);
        assert_eq!(buffer[(8, 4)].bg, Color::Magenta);
        assert_eq!(buffer[(8, 3)].symbol(), "e\u{301}");
        app.help = true;
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert!(
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .all(|cell| cell.bg != Color::Magenta)
        );
    }
}
