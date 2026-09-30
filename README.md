# docz

A personal modal terminal text editor written in Rust, with gamer-style movement,
anchored selections, and familiar editing shortcuts.

## Run

```sh
cargo run --                 # browse the current directory
cargo run -- notes.txt       # edit a file (create it on save if it doesn't exist)
cargo run -- path/to/folder  # browse another directory
```

The stable build is installed as `~/.local/bin/docz`, so you can run:

```sh
docz
docz notes.txt
```

The `dev` development branch is installed separately as
`~/.local/bin/docz-dev`:

```sh
docz-dev
docz-dev script.py
```

To rebuild and update the development installation:

```sh
git switch dev
cargo build --release --locked
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/docz "$HOME/.local/bin/docz-dev"
```

`~/.local/bin` is already on your PATH. Alternatively, `cargo install --path .`
installs into Cargo's binary directory (usually `~/.cargo/bin`), which must be on
your PATH if you use that method.
Use `docz --help` for command-line help; `docz -- -filename` handles filenames
starting with a dash. The app requires an interactive terminal.

## Syntax highlighting

Highlighting is automatic, using Syntect's bundled Sublime syntax definitions
and the Gruvbox Dark theme. Language detection checks the filename or
extension, then the first line for a shebang; unknown files remain plain text.
Supported bundled languages include Python, Rust, JavaScript, JSON, shell,
HTML, CSS, C/C++, Markdown, and Makefiles. The status bar shows the detected
language when no transient message is active.

Python highlighting handles multiline triple-quoted strings, f-string
expressions, comments, and incomplete code while editing. Syntax colors are
applied before selection and secondary-cursor highlights, preserving their
contrast. Tabs, Unicode graphemes, and horizontal scrolling retain their usual
layout. Highlighting does not change saved text or line endings.

Colored spans are cached, with parser checkpoints every 64 lines. Cursor
movement reuses cached colors; edits, paste, undo, and redo invalidate from the
affected checkpoint onward. Parsing stops at the bottom of the viewport, while
retaining context from preceding lines. A first jump deep into a large file
still parses the preceding text synchronously and may pause. Theme selection,
manual language overrides, and language-server features are future work.

## Theme

The entire editor uses [Gruvbox Dark](https://github.com/morhetz/gruvbox) with
medium contrast: a `#282828` background, warm cream text, and muted accents.
The file explorer, buffer, gutters, headers, status bars, help, and unsaved-change
prompts share the same palette. Selections use a lighter brown background;
secondary cursors use purple. Syntax colors use red keywords, green strings and
function names, purple constants, aqua built-ins, and gray comments.

## Controls and modes

Letter keys below use lowercase unless Shift is specified. The editor starts in
Navigate mode. E enters Edit; F enters Selection; Q cancels Selection, help,
and confirmation prompts. Esc returns from Edit to Navigate; q types normally
in Edit. Ctrl+Q remains the quit shortcut.

| Context | Key | Action |
| --- | --- | --- |
| Navigate / Selection | W / A / S / D or arrows | Move up / left / down / right |
| Navigate / Selection | Shift+A / Shift+D | Previous / next word start |
| Navigate / Selection | Shift+W / Shift+S | Previous / next paragraph start |
| Navigate / Selection | E | Enter Edit, retaining any selection until input replaces it |
| Navigate | F | Enter Selection at the cursor |
| Edit | Ordinary keys | Type text; WASD, E, and F insert letters |
| Edit | Enter / Tab | Insert newline / literal tab (four-column display stops) |
| Edit | Arrows | Move all cursors; Shift jumps words / paragraphs |
| Edit | Ctrl+`[` / Ctrl+`]` | Add a cursor above / below the existing cursor set |
| Edit | Ctrl+Up / Ctrl+Down | Alternate cursor-above / cursor-below bindings |
| Edit | Ctrl+`\` | Reset to the original primary cursor |
| Edit | `"`, `(`, `[`, `{` | Insert a matching pair, with the cursor inside |
| Edit | `"`, `)`, `]`, `}` | Skip the same closing symbol under the cursor |
| Edit / Selection | Backspace / Delete | Remove selected text, otherwise edit individual characters in Edit |
| Navigate | Delete / Backspace | Delete at cursor / move left |
| Selection | Q | Cancel selection and return to Navigate |
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
| Everywhere | F1 | Show help (F1 / Q / Esc closes it) |
| Explorer | W / S or Up / Down | Select entry |
| Explorer | Shift+W / Shift+S | Move five entries |
| Explorer | D / Enter / Right | Open file or directory |
| Explorer | A / Q / Backspace / Left | Parent directory |
| Explorer | Home / End | First / last entry |
| Explorer | Esc | Quit |
| Unsaved prompt | S / D / Q or Esc | Save and continue / discard / cancel |

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

### Multiple cursors

In Edit, Ctrl+`[` adds a cursor above the highest cursor and Ctrl+`]` adds one below the
lowest. Repeating the key extends the set one line at a time, stopping at file
boundaries. New cursors use the primary cursor's grapheme column, clamped to the
end of shorter lines; extending past a short line retains the original column.
Adding cursors clears any pending selection without deleting its text.

Typing, Enter, Tab, Backspace, Delete, and paste apply simultaneously at every
cursor. Arrow movement, word/paragraph sprint, and Home/End move all cursors;
coincident cursors merge so text is inserted or deleted only once. Ctrl+Home/End
converges the cursors at the file boundary. Secondary cursors are highlighted in
purple and the status bar shows the count. The viewport shows the whole set
when it fits; otherwise it keeps the primary cursor visible.

Ctrl+`\` returns to the original primary cursor. Leaving Edit also resets the set.
Undo/redo groups edits across all cursors; while in Edit, it also restores their
positions. Changing the cursor set starts a new undo group. Clipboard paste
repeats the whole clipboard at each cursor, including any newlines.

Unmodified brackets insert text with automatic pairing; `\` types normally.
No literal-entry prefix is needed. Bracketed terminal paste inserts its contents
literally.

Ctrl+`[` requires a terminal that distinguishes it from Esc (enhanced keyboard
reporting is enabled automatically where supported). In legacy terminals it
arrives as Esc and cancels Edit; use Ctrl+Up to add above instead. Ctrl+Down
also adds below. Ctrl+`]` and Ctrl+`\` support their legacy terminal encodings.

### Automatic pairs

Typing `"`, `(`, `[`, or `{` inserts `""`, `()`, `[]`, or `{}` and places the cursor
between the symbols. Typing a closing symbol already under the cursor moves past
it, so typing a complete expression does not duplicate closers. This also works
with existing text and independently at each cursor. Backspace between an empty
pair removes both symbols. A quote preceded by an odd number of backslashes types
literally, allowing escaped quotes inside strings.

Typing an opener over a selection replaces it with an empty pair. Clipboard and
bracketed terminal paste always insert their exact contents without adding pairs.

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
- One UTF-8 file at a time; Navigate/Edit/Selection modes, multiple cursors, syntax highlighting, scrolling, line numbers and status.
- Undo grouped by Edit session, with cut/paste as separate steps and up to 100 whole-buffer snapshots.
- Saves use a temporary file in the same directory followed by replacement, preserve
  existing permissions and original line endings, and refuse detected external changes.
- Existing symlinks resolve to their target before editing. Atomic replacement changes
  inode identity; hard-link relationships and extended metadata are not preserved.
- Save/discard/cancel before quitting or opening the explorer with unsaved changes.
- Terminal cleanup on normal exit, errors, and Rust panics.

This version keeps text in memory and clones buffers for undo. It targets ordinary
personal text files; a rope and edit-based history can follow if large-file editing
becomes a requirement. Search,
multiple buffers, configuration and scripting are not implemented yet.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
python3 scripts/terminal_smoke.py  # Linux PTY integration check
```
