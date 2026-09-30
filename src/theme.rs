use ratatui::style::{Color, Style};
use syntect::highlighting::{
    Color as SyntaxColor, FontStyle, StyleModifier, Theme, ThemeItem, ThemeSettings,
};

// Gruvbox Dark, medium contrast: https://github.com/morhetz/gruvbox
pub const BG: Color = Color::Rgb(40, 40, 40);
pub const PANEL: Color = Color::Rgb(60, 56, 54);
pub const SELECTION: Color = Color::Rgb(80, 73, 69);
pub const BORDER: Color = Color::Rgb(102, 92, 84);
pub const FG: Color = Color::Rgb(235, 219, 178);
pub const BRIGHT_FG: Color = Color::Rgb(251, 241, 199);
pub const MUTED: Color = Color::Rgb(168, 153, 132);
pub const GRAY: Color = Color::Rgb(146, 131, 116);
pub const RED: Color = Color::Rgb(251, 73, 52);
pub const GREEN: Color = Color::Rgb(184, 187, 38);
pub const YELLOW: Color = Color::Rgb(250, 189, 47);
pub const BLUE: Color = Color::Rgb(131, 165, 152);
pub const PURPLE: Color = Color::Rgb(211, 134, 155);
pub const AQUA: Color = Color::Rgb(142, 192, 124);
pub const ORANGE: Color = Color::Rgb(254, 128, 25);

pub fn base() -> Style {
    Style::default().fg(FG).bg(BG)
}

pub fn panel() -> Style {
    Style::default().fg(FG).bg(PANEL)
}

pub fn selected() -> Style {
    Style::default().fg(BRIGHT_FG).bg(SELECTION)
}

pub fn secondary_cursor() -> Style {
    Style::default().fg(BG).bg(PURPLE)
}

fn syntax_color(color: Color) -> SyntaxColor {
    let Color::Rgb(r, g, b) = color else {
        unreachable!("Gruvbox colors use RGB")
    };
    SyntaxColor { r, g, b, a: 255 }
}

pub fn syntax_theme() -> Theme {
    let rules = [
        ("comment", GRAY, FontStyle::ITALIC),
        ("string", GREEN, FontStyle::empty()),
        (
            "constant.numeric, constant.language, constant.character",
            PURPLE,
            FontStyle::empty(),
        ),
        ("keyword, storage", RED, FontStyle::empty()),
        ("keyword.operator", ORANGE, FontStyle::empty()),
        ("entity.name.function", GREEN, FontStyle::empty()),
        ("support.function", AQUA, FontStyle::empty()),
        (
            "entity.name.type, entity.name.class, support.type, support.class",
            YELLOW,
            FontStyle::empty(),
        ),
        (
            "variable.parameter, variable.language",
            BLUE,
            FontStyle::empty(),
        ),
        ("entity.name.tag", BLUE, FontStyle::empty()),
        ("entity.other.attribute-name", AQUA, FontStyle::empty()),
        (
            "meta.interpolation, meta.embedded, source.python.embedded",
            FG,
            FontStyle::empty(),
        ),
        (
            "constant.character.escape, punctuation.section.interpolation",
            ORANGE,
            FontStyle::empty(),
        ),
        (
            "entity.name.section, markup.heading",
            YELLOW,
            FontStyle::BOLD,
        ),
        ("markup.bold", FG, FontStyle::BOLD),
        ("markup.italic", FG, FontStyle::ITALIC),
        ("markup.raw, markup.inline.raw", GREEN, FontStyle::empty()),
        ("markup.underline.link", BLUE, FontStyle::UNDERLINE),
        ("markup.inserted", GREEN, FontStyle::empty()),
        ("markup.deleted", RED, FontStyle::empty()),
        ("markup.changed", ORANGE, FontStyle::empty()),
        ("invalid", RED, FontStyle::UNDERLINE),
    ];
    Theme {
        name: Some("Gruvbox Dark".into()),
        settings: ThemeSettings {
            foreground: Some(syntax_color(FG)),
            background: Some(syntax_color(BG)),
            caret: Some(syntax_color(BRIGHT_FG)),
            selection: Some(syntax_color(SELECTION)),
            selection_foreground: Some(syntax_color(BRIGHT_FG)),
            ..ThemeSettings::default()
        },
        scopes: rules
            .into_iter()
            .map(|(scope, foreground, font_style)| ThemeItem {
                scope: scope
                    .parse()
                    .expect("valid built-in Gruvbox scope selector"),
                style: StyleModifier {
                    foreground: Some(syntax_color(foreground)),
                    font_style: Some(font_style),
                    ..StyleModifier::default()
                },
            })
            .collect(),
        ..Theme::default()
    }
}
