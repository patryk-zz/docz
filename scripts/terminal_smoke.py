#!/usr/bin/env python3
"""Exercise the actual binary in a Linux PTY, using only Python's standard library."""
import fcntl
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time


class Session:
    def __init__(self, binary, cwd, *args, enhanced=False, clipboard_env=None):
        self.master, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 28, 100, 0, 0))
        self.before = termios.tcgetattr(self.slave)
        self.enhanced = enhanced
        self.responded = False
        environment = {**os.environ, "TERM": "xterm-256color"}
        # Keep tests off the user's desktop clipboard.
        environment.pop("WAYLAND_DISPLAY", None)
        environment.pop("DISPLAY", None)
        environment.pop("NO_COLOR", None)
        environment.update(clipboard_env or {})

        def controlling_terminal():
            os.setsid()
            fcntl.ioctl(self.slave, termios.TIOCSCTTY, 0)

        self.process = subprocess.Popen(
            [str(binary), *args], cwd=cwd, stdin=self.slave, stdout=self.slave,
            stderr=self.slave, env=environment,
            preexec_fn=controlling_terminal,
        )
        self.transcript = b""
        self.drain(0.3)

    def drain(self, duration=0.1):
        end = time.monotonic() + duration
        while time.monotonic() < end:
            ready, _, _ = select.select([self.master], [], [], max(0, end - time.monotonic()))
            if ready:
                self.transcript += os.read(self.master, 65536)
                if not self.responded and b"\x1b[c" in self.transcript:
                    response = b"\x1b[?0u" if self.enhanced else b""
                    os.write(self.master, response + b"\x1b[?1;2c")
                    self.responded = True

    def send(self, data):
        os.write(self.master, data)
        self.drain()

    def finish(self):
        self.process.wait(timeout=3)
        self.drain()
        assert self.process.returncode == 0, self.transcript.decode(errors="replace")
        after = termios.tcgetattr(self.slave)
        assert after == self.before, "Terminal settings were not restored"
        assert b"\x1b[?1049l" in self.transcript, "Alternate screen was not restored"
        if self.enhanced:
            assert b"\x1b[>5u" in self.transcript, "Enhanced keyboard reporting was not enabled"
            assert b"\x1b[<1u" in self.transcript, "Enhanced keyboard reporting was not restored"

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait()
        os.close(self.master)
        os.close(self.slave)


def main():
    binary = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else (
        Path(__file__).resolve().parent.parent / "target/release/docz"
    )
    with tempfile.TemporaryDirectory(prefix="docz-smoke-") as directory:
        root = Path(directory)
        (root / "sub").mkdir()
        notes = root / "notes.txt"
        notes.write_bytes(b"abcde f\r\nsecond\r\n")

        session = Session(binary, root)
        try:
            assert b"EXPLORE" in session.transcript
            session.send(b"sd")  # Select and enter subdirectory.
            session.send(b"q")   # Back to parent with the new cancel key.
            session.send(b"ss\r")  # Select and open notes.txt.
            assert b"NAVIGATE" in session.transcript
            session.send(b"De")  # Sprint to next word, then edit.
            session.send(b"[ok]")  # Auto-pair brackets and skip the existing closer.
            session.send(b"\x1b")
            session.send(b"\x13")  # Ctrl+S.
            session.send(b"\x11")
            session.finish()
            assert notes.read_bytes() == b"abcde [ok]f\r\nsecond\r\n"
        finally:
            session.close()

        # Highlight Python multiline context while scrolling; preserve original bytes.
        python_file = root / "highlight.py"
        python_text = '@decorator\ndef example(name):\n    """docs\n' + '    continued\n' * 40 + '    """\n    return f"hello {name}" # comment\n'
        python_file.write_bytes(python_text.replace('\n', '\r\n').encode())
        session = Session(binary, root, "highlight.py")
        try:
            assert b"38;2;" in session.transcript, "Syntax foreground colors were not rendered"
            session.send(b"\x1b[6~")  # PageDown into the middle of the docstring.
            session.send(b"e#")
            session.send(b"\x1a\x19\x1a")  # Undo/redo/undo refreshes the highlighting cache.
            session.send(b"\x13\x11")
            session.finish()
            assert python_file.read_bytes() == python_text.replace('\n', '\r\n').encode()
        finally:
            session.close()

        # Typed pairs nest without duplicate closers; empty-pair Backspace and literal paste.
        pairs = root / "pairs.txt"
        session = Session(binary, root, "pairs.txt")
        try:
            session.send('e({["中"]})'.encode())
            session.send(b"{\x7f")  # Insert an empty pair, then delete both symbols.
            session.send(b' "say \\"hi\\""')  # Escaped quotes insert literally.
            session.send(b'\x1b[200~(["{\x1b[201~')
            session.send(b"\x13\x11")
            session.finish()
            assert pairs.read_text() == '({["中"]}) "say \\"hi\\""(["{'
        finally:
            session.close()

        # Each cursor independently chooses between quote pairing and skipping.
        pairs.write_bytes(b'"\r\nx\r\n"')
        session = Session(binary, root, "pairs.txt", enhanced=True)
        try:
            session.send(b'se\x1b[91;5u\x1b[93;5u"X')
            session.send(b"\x13\x11")
            session.finish()
            assert pairs.read_bytes() == b'"X\r\n"X"x\r\n"X'
        finally:
            session.close()

        # Q cancels Selection/help/confirmation, while q remains ordinary Edit text.
        cancel_file = root / "cancel.txt"
        cancel_file.write_text("hello world")
        session = Session(binary, root, "cancel.txt")
        try:
            session.send(b"fDq")
            session.send(b"\x1bOP")  # F1 opens help.
            session.send(b"q")
            session.send(b"eqq")
            session.send(b"\x11")
            session.send(b"q")  # Cancel the unsaved-change prompt.
            assert session.process.poll() is None
            session.send(b"\x13\x11")
            session.finish()
            assert cancel_file.read_text() == "hello qqworld"
        finally:
            session.close()

        # Cursors above and below a middle primary cursor; multiline edits, reset, undo/redo.
        multi = root / "multi.txt"
        multi.write_bytes(b"ab\r\ncd\r\nef")
        session = Session(binary, root, "multi.txt", enhanced=True)
        try:
            session.send(b"sde\x1b[91;5u\x1b[93;5uX")  # Enhanced Ctrl+[ and Ctrl+].
            session.send(b"\x7f")  # Remove X at all three cursors.
            session.send(b"\rq")   # Insert a newline and q at each cursor.
            session.send(b"\x1b[92;5u!")  # Enhanced Ctrl+\\ resets; type only at primary.
            session.send(b"\x1a\x1a")  # Undo the single and multiple cursor edits.
            session.send(b"\x19\x19")  # Redo both.
            session.send(b"\x13\x11")
            session.finish()
            assert multi.read_bytes() == b"a\r\nqb\r\nc\r\nq!d\r\ne\r\nqf"
        finally:
            session.close()

        # Legacy Ctrl+] / Ctrl+\ and Ctrl+Up fallback; plain symbols remain text.
        multi.write_text("a\nb\nc")
        session = Session(binary, root, "multi.txt")
        try:
            session.send(b"se\x1b[1;5A\x1d")  # Ctrl+Up and legacy Ctrl+].
            session.send(b"[]\\")  # Literal symbols at all three cursors.
            session.send(b"\x1c!")  # Legacy Ctrl+\ resets to the middle primary.
            session.send(b"\x13\x11")
            session.finish()
            assert multi.read_text() == "[]\\a\n[]\\!b\n[]\\c"
        finally:
            session.close()

        session = Session(binary, root, "new file.txt")
        try:
            session.send(b"e")
            session.send(b"\x1b[200~" + "wasd\n中".encode() + b"\x1b[201~")
            session.send(b"\x11")  # Ctrl+Q prompts rather than losing changes.
            assert b"Unsaved changes" in session.transcript
            assert session.process.poll() is None
            session.send(b"\x1b")  # Cancel quit.
            session.send(b"\x1b")  # Leave Edit.
            session.send(b"\x1a")  # Undo the complete insertion.
            session.send(b"\x19")  # Redo.
            session.send(b"\x11")
            session.send(b"s")  # Save and quit from confirmation.
            session.finish()
            assert (root / "new file.txt").read_text() == "wasd\n中"
        finally:
            session.close()

        session = Session(binary, root, "new file.txt")
        try:
            session.send(b"eDISCARD")
            session.send(b"\x11")
            session.send(b"d")
            session.finish()
            assert (root / "new file.txt").read_text() == "wasd\n中"
        finally:
            session.close()

        # Internal clipboard plus terminal decoding of Ctrl+Shift+Z (CSI-u).
        selection = root / "selection.txt"
        selection.write_text("hello world\n\nsecond paragraph")
        session = Session(binary, root, "selection.txt", enhanced=True)
        try:
            session.send(b"\x03")  # Ctrl+C without selection must not quit.
            assert session.process.poll() is None
            session.send(b"fD")  # Select first word and following space.
            session.send(b"\x03")  # Copy and keep selection.
            session.send(b"e")  # Retain selection pending replacement.
            session.send(b"bye ")
            session.send(b"\x1b")
            session.send(b"\x1a")  # Undo replacement.
            session.send(b"\x1b[122;6u")  # Ctrl+Shift+Z: redo replacement.
            session.send(b"\x1b[1;5H")  # Ctrl+Home.
            session.send(b"fD")
            session.send(b"\x18")  # Cut "bye ".
            session.send(b"\x16")  # Paste the newly cut "bye ".
            session.send(b"\x13\x11")
            session.finish()
            assert selection.read_text() == "bye world\n\nsecond paragraph"
        finally:
            session.close()

        # Exercise desktop helper integration with a private fake Wayland clipboard.
        helpers = root / "helpers"
        helpers.mkdir()
        clipboard_file = root / "clipboard.data"
        helper = """#!/usr/bin/env python3
import os, sys
from pathlib import Path
path = Path(os.environ['DOCZ_SMOKE_CLIPBOARD'])
if sys.argv[0].endswith('wl-copy'):
    path.write_bytes(sys.stdin.buffer.read())
else:
    sys.stdout.buffer.write(path.read_bytes())
"""
        for name in ["wl-copy", "wl-paste"]:
            path = helpers / name
            path.write_text(helper)
            path.chmod(0o755)
        clipboard_env = {"PATH": str(helpers) + os.pathsep + os.environ["PATH"],
                         "WAYLAND_DISPLAY": "docz-test", "DOCZ_SMOKE_CLIPBOARD": str(clipboard_file)}
        selection.write_text("hello world")
        session = Session(binary, root, "selection.txt", clipboard_env=clipboard_env)
        try:
            session.send(b"fD\x03")
            assert clipboard_file.read_text() == "hello "
            clipboard_file.write_text("external 中\n")
            session.send(b"\x16")  # Paste a simulated external application's clipboard.
            session.send(b"\x13\x11")
            session.finish()
            assert selection.read_text() == "external 中\nworld"
        finally:
            session.close()

        # Paragraph selection + E + bracketed paste replaces the entire anchored range.
        selection.write_text("first\nmore\n\n \t\nsecond\n\nthird")
        session = Session(binary, root, "selection.txt")
        try:
            session.send(b"fSe")
            session.send(b"\x1b[200~replacement\n\n\x1b[201~")
            session.send(b"\x13\x11")
            session.finish()
            assert selection.read_text() == "replacement\n\nsecond\n\nthird"
        finally:
            session.close()

        # Startup diagnostics must never forward a filename's terminal controls.
        osc52 = b"\x1b]52;c;ZG9jei1hdWRpdA==\x07"
        hostile = root / ("hostile-" + osc52.decode() + ".txt")
        hostile.write_bytes(b"\xff")
        session = Session(binary, root, hostile.name)
        try:
            session.process.wait(timeout=3)
            session.drain()
            assert session.process.returncode != 0
            assert osc52 not in session.transcript, "Filename injected terminal controls"
            assert b"\\u{1b}]52;" in session.transcript, "Filename controls were not escaped"
            assert termios.tcgetattr(session.slave) == session.before
        finally:
            session.close()

        # Nonblocking type validation rejects an initial FIFO before reading it.
        fifo = root / "initial-fifo"
        os.mkfifo(fifo)
        session = Session(binary, root, fifo.name)
        try:
            session.process.wait(timeout=3)
            session.drain()
            assert session.process.returncode != 0
            assert b"not a regular file" in session.transcript
            assert termios.tcgetattr(session.slave) == session.before
        finally:
            session.close()

        # A FIFO substituted before save must not block either existing or new files.
        for existing in [True, False]:
            path = root / f"save-fifo-{existing}.txt"
            if existing:
                path.write_text("original")
            session = Session(binary, root, path.name)
            try:
                session.send(b"eX")
                if existing:
                    path.unlink()
                os.mkfifo(path)
                session.send(b"\x13")
                # Ratatui writes only changed cells, so status text need not be
                # contiguous in the PTY stream. Check failure through behavior:
                # unsaved changes remain, quit works, and the FIFO survives.
                session.send(b"\x11")
                assert b"Unsaved changes" in session.transcript
                session.send(b"d")
                session.finish()
                assert path.is_fifo(), "Rejected save replaced the FIFO"
            finally:
                session.close()

    for option in ["--help", "--version"]:
        subprocess.run([str(binary), option], check=True, capture_output=True)
    result = subprocess.run([str(binary), "--unknown"], capture_output=True)
    assert result.returncode != 0
    result = subprocess.run([str(binary), "--bad-" + osc52.decode()], capture_output=True)
    assert result.returncode != 0
    assert osc52 not in result.stderr, "Command-line error injected terminal controls"
    print("PASS: no-argument explorer, folder navigation, file opening, sprint, editing,")
    print("      CRLF save, new file, Unicode paste, undo/redo, cancel/save/discard,")
    print("      anchored selection, E replacement, word/paragraph jumps, copy/cut/paste,")
    print("      private desktop clipboard helpers, enhanced redo, CLI, and terminal restoration.")
    print("      Q cancellation, literal keys, multicursor insertion/deletion/newlines/reset/undo.")
    print("      Automatic pairs, closer skipping, escaped quotes, empty-pair deletion, literal paste.")
    print("      Python syntax colors, multiline viewport context, edit/undo/redo and CRLF preservation.")
    print("      Escaped hostile diagnostics, FIFO open/save rejection, and responsive quit after save errors.")


if __name__ == "__main__":
    main()
