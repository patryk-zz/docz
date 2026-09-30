mod app;
mod browser;
mod buffer;
mod clipboard;
mod document;
mod editor;
mod input;
mod syntax;
mod ui;

use anyhow::{Context, Result, bail};
use app::App;
use crossterm::{
    cursor::Show,
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    ffi::OsString,
    io::{self, IsTerminal},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

const HELP: &str = "docz — a personal modal terminal editor\n\n\
    Usage: docz [FILE]\n\
           docz [DIRECTORY]\n\n\
    With no argument, explore the current directory.\n\
    A nonexistent file opens an empty buffer and is created when saved.\n\
    Syntax highlighting is detected from filenames and shebangs automatically.\n\
    Use -- before a filename starting with a dash.\n\n\
    Navigate: WASD moves; Shift+A/D jumps words; Shift+W/S jumps paragraphs.\n\
    E enters Edit; F enters Selection; Esc returns to Navigate.\n\
    Q cancels Selection/help/prompts; q types normally in Edit.\n\
    In Edit: Ctrl+[ adds above; Ctrl+] below; Ctrl+\\ resets cursors.\n\
    Quotes/brackets close automatically; typing an existing closer skips it.\n\
    Ctrl+Up/Down also add cursors (Ctrl+[ may arrive as Esc in older terminals).\n\
    Ctrl+C copies; Ctrl+X cuts; Ctrl+V pastes.\n\
    Ctrl+Z undoes; Ctrl+Shift+Z redoes (Ctrl+Y fallback).\n\
    Ctrl+S saves; Ctrl+Q quits; Ctrl+E opens the explorer; F1 shows help.\n\n\
    Options: -h, --help     Show help\n\
             -V, --version  Show version";

enum Cli {
    Run(Option<PathBuf>),
    Help,
    Version,
}

fn program_name() -> &'static str {
    if std::env::args_os().next().is_some_and(|arg| {
        std::path::Path::new(&arg)
            .file_name()
            .is_some_and(|name| name == "docz-dev")
    }) {
        "docz-dev"
    } else {
        "docz"
    }
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Cli> {
    let mut filename = None;
    let mut literal = false;
    for arg in args {
        if !literal {
            if arg == "--" {
                literal = true;
                continue;
            }
            if arg == "--help" || arg == "-h" {
                return Ok(Cli::Help);
            }
            if arg == "--version" || arg == "-V" {
                return Ok(Cli::Version);
            }
            if arg.to_string_lossy().starts_with('-') {
                bail!(
                    "Unknown option: {}\nUse docz --help for usage",
                    arg.to_string_lossy()
                );
            }
        }
        if filename.is_some() {
            bail!("Open one file or directory at a time\nUse docz --help for usage");
        }
        filename = Some(PathBuf::from(arg));
    }
    Ok(Cli::Run(filename))
}

static ENHANCED_KEYBOARD: AtomicBool = AtomicBool::new(false);

fn restore_terminal() {
    if ENHANCED_KEYBOARD.swap(false, Ordering::Relaxed) {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = terminal::disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    );
}

struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode().context("Cannot enable terminal raw mode")?;
        let guard = Self;
        execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)
            .context("Cannot initialize terminal")?;
        if terminal::supports_keyboard_enhancement().unwrap_or(false) {
            execute!(
                io::stdout(),
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS,
                )
            )?;
            ENHANCED_KEYBOARD.store(true, Ordering::Relaxed);
        }
        Ok(guard)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn run() -> Result<()> {
    let path = match parse_args(std::env::args_os().skip(1))? {
        Cli::Help => {
            println!("{}", HELP.replace("docz", program_name()));
            return Ok(());
        }
        Cli::Version => {
            println!("{} {}", program_name(), env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Cli::Run(path) => path,
    };
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("docz needs an interactive terminal (try docz --help)");
    }
    let mut app = App::new(path.as_deref())?;
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        original_hook(info);
    }));
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    while app.running {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        app.handle(event::read()?);
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{}: {error:#}", program_name());
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_accepts_optional_paths_and_literal_dashes() {
        assert!(matches!(
            parse_args(Vec::<OsString>::new()).unwrap(),
            Cli::Run(None)
        ));
        assert!(matches!(
            parse_args(["a b.txt".into()]).unwrap(),
            Cli::Run(Some(_))
        ));
        assert!(matches!(
            parse_args(["--".into(), "-file".into()]).unwrap(),
            Cli::Run(Some(_))
        ));
        assert!(matches!(parse_args(["--help".into()]).unwrap(), Cli::Help));
        assert!(parse_args(["--unknown".into()]).is_err());
        assert!(parse_args(["one".into(), "two".into()]).is_err());
    }
}
