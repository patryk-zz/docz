# docz

A personal modal terminal text editor written in Rust, with gamer-style movement,
anchored selections, and familiar editing shortcuts.

## Run

```sh
cargo run --                 # browse the current directory
cargo run -- notes.txt       # edit a file (create it on save if it doesn't exist)
cargo run -- path/to/folder  # browse another directory
```

The development build is installed as `~/.local/bin/docz`, so you can run:

```sh
docz
docz notes.txt
```

To rebuild and update that installation on this system:

```sh
cargo build --release --locked
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/docz "$HOME/.local/bin/docz"
```

`~/.local/bin` is already on your PATH. Alternatively, `cargo install --path .`
installs into Cargo's binary directory (usually `~/.cargo/bin`), which must be on
your PATH if you use that method.
Use `docz --help` for command-line help; `docz -- -filename` handles filenames
starting with a dash. The app requires an interactive terminal.

## Controls and modes

Letter keys below use lowercase unless Shift is specified. The editor starts in
Navigate mode. E enters Edit; F enters Selection; Esc returns to Navigate.

| Context | Key | Action |
| --- | --- | --- |
| Navigate / Selection | W / A / S / D or arrows | Move up / left / down / right |
| Navigate / Selection | Shift+A / Shift+D | Previous / next word start |
| Navigate / Selection | Shift+W / Shift+S | Previous / next paragraph start |
| Navigate / Selection | E | Enter Edit, retaining any selection until input replaces it |
| Navigate | F | Enter Selection at the cursor |
| Edit | Ordinary keys | Type text; WASD, E, and F insert letters |
| Edit | Enter / Tab | Insert newline / literal tab (four-column display stops) |
| Edit | Arrows | Move cursor; Shift jumps words / paragraphs |
| Edit / Selection | Backspace / Delete | Remove selected text, otherwise edit individual characters in Edit |
| Navigate | Delete / Backspace | Delete at cursor / move left |
| Editor | Esc | Clear selection without editing text, return to Navigate |
| Editor | Home / End | Start / end of line |
| Editor | Ctrl+Home / Ctrl+End | Start / end of file |
| Editor | PageUp / PageDown | Move one screen |
| Editor | Ctrl+Z | Undo |
| Editor | Ctrl+Shift+Z | Redo (requires the terminal to distinguish Shift) |
| Editor | Ctrl+Y | Redo fallback |
| Editor | Ctrl+C | Copy selection and keep it highlighted |
| Editor | Ctrl+X | Cut selection and return to Navigate |
| Editor | Ctrl+V | Replace selection, or insert at cursor |
| Editor | Ctrl+S | Save |
| Editor | Ctrl+E | Open explorer in the current file's directory |
| Everywhere | Ctrl+Q | Quit |
| Everywhere | F1 | Show help (F1 / Esc closes it) |
| Explorer | W / S or Up / Down | Select entry |
| Explorer | Shift+W / Shift+S | Move five entries |
| Explorer | D / Enter / Right | Open file or directory |
| Explorer | A / Backspace / Left | Parent directory |
| Explorer | Home / End | First / last entry |
| Explorer | Esc | Quit |
| Unsaved prompt | S / D / Esc | Save and continue / discard / cancel |

### Selection

F fixes the selection anchor at the cursor. Movement extends the range between
that anchor and the cursor, including backward selection and line breaks. Moving
back across the anchor shrinks and reverses the selection. Home/End, file bounds,
page movement, and word/paragraph jumps all extend selection in this mode.

E keeps the selection highlighted in Edit. The first typed character, newline,
tab, or paste replaces it; entering Edit alone never deletes text. Esc cancels
this pending replacement. Moving the cursor in Edit also clears the pending
selection without changing text. Backspace/Delete removes the selection once,
without removing an additional character. Copy/cut with no nonempty selection
does nothing and displays a status message.

### Movement

A/D crosses line boundaries, making it possible to select a newline. Columns
count Unicode grapheme clusters, including combined accents and emoji. Words
contain letters, numbers, and underscores; each punctuation/other symbol is a
separate stop. Word jumps cross lines. Paragraphs are blocks of nonblank lines,
separated by blank or whitespace-only lines. From within a word/paragraph,
backward sprint goes to its start; another backward sprint goes to the previous
start. Forward sprint goes to the next start. Jumps clamp to file boundaries when
there is no further start. Rendering accounts for wide characters and tabs.

### Clipboard and redo compatibility

Clipboard access uses `wl-copy` / `wl-paste` on Wayland, `xclip` on X11, or
`pbcopy` / `pbpaste` on macOS. The Linux helpers are already installed on this
system. An internal clipboard remains available if desktop access fails; the
status bar identifies the clipboard used. Desktop commands have a bounded wait
so a failed helper cannot leave the editor stuck. Windows currently uses the
internal fallback. Bracketed terminal paste uses the same replacement behavior
as Ctrl+V, in every editor mode.

The editor detects and enables enhanced keyboard reporting where supported,
then restores it on exit. Some terminals send Ctrl+Shift+Z as Ctrl+Z, making
them indistinguishable; use Ctrl+Y for redo in those terminals. Ctrl+C copies
rather than quitting, and plain Q no longer quits the editor.

## Current scope

- Directory browser with directories first, hidden files, and navigation into folders.
- One UTF-8 file at a time; Navigate/Edit/Selection modes, scrolling, line numbers and status.
- Undo grouped by Edit session, with cut/paste as separate steps and up to 100 whole-buffer snapshots.
- Saves use a temporary file in the same directory followed by replacement, preserve
  existing permissions and original line endings, and refuse detected external changes.
- Existing symlinks resolve to their target before editing. Atomic replacement changes
  inode identity; hard-link relationships and extended metadata are not preserved.
- Save/discard/cancel before quitting or opening the explorer with unsaved changes.
- Terminal cleanup on normal exit, errors, and Rust panics.

This version keeps text in memory and clones buffers for undo. It targets ordinary
personal text files; a rope and edit-based history can follow if large-file editing
becomes a requirement. Syntax highlighting, search,
multiple buffers, configuration and scripting are not implemented yet.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
python3 scripts/terminal_smoke.py  # Linux PTY integration check
```
