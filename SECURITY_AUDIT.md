# docz security and public-repository audit

Audited on September 29, 2026 (America/Denver).
Baseline: `b4710915ceb0957c7f262bcfb1f3dcf5f951a500`, version `0.1.0`.
Finding locations below refer to this baseline unless stated otherwise.

## Remediation status

Following the audit, findings 1 and 2 were fixed and an MIT `LICENSE` was added.
Diagnostics now escape terminal control characters before reaching stderr.
On Unix, initial reads and save validation open with `O_NONBLOCK | O_NOFOLLOW`,
verify the opened handle is a regular file, and read contents and permissions
from that same handle. Existing symlinks are still resolved when first opened;
replacement symlinks are rejected during save validation.

Regression coverage includes hostile filename/argument diagnostics, initial
FIFOs, FIFO substitution before saving both existing and new files, responsive
quit after rejected saves, and existing/replacement symlink behavior. The
remaining metadata, dependency, and resource-limit findings are still open.

Follow-up verification passed: all 59 unit tests, formatting, Clippy with
warnings denied, the locked release build, and the expanded Linux PTY smoke
suite. The FIFO regressions confirm rejected saves keep unsaved changes,
leave the FIFO intact, and permit quitting with terminal settings restored.
The Unix `libc` dependency uses the version already present in the lockfile;
no third-party package versions were changed. A pre-commit secret-pattern scan
of the eight changed/added files found no matches.

## Assessment

No credentials or sensitive configuration files were detected in the reviewed
working tree or local Git history. There is no detected secret-exposure blocker
to making this history public. Commit author email addresses will become public
along with the history; verify that the existing identity is intentional.

The program has a small attack surface: application code has no network client,
listener, telemetry, scripting, shell evaluation, or `unsafe` blocks. Clipboard
access invokes fixed commands with fixed arguments and passes text through a
temporary file. Opening a source file does not execute its contents.

The two reproduced runtime issues below have been fixed. File metadata handling
and dependency advisories still need resolution or an explicit, documented
acceptance of their limitations before promoting docz as a secure release.
This audit is evidence about the reviewed version, not a guarantee of safety.
The initial audit left application source unchanged. Follow-up remediation adds
the two fixes, regression tests, the license, and README documentation.

## Findings

### 1. Medium, fixed: startup errors emit terminal controls from untrusted filenames

Location: `src/main.rs:171`, with filename-bearing errors from
`src/document.rs:19`, `src/document.rs:30`, and `src/main.rs:85`.

The interactive UI sanitizes control characters, but the top-level error handler
prints the complete error chain directly to stderr. On Unix, filenames can
contain escape sequences. Opening an invalid UTF-8 file with a crafted filename
causes its escape sequences to reach the terminal through the error message.

Reproduced against the release binary in a private PTY: a filename containing
an OSC 52 sequence and a file containing byte `0xff` produced exit code 1 and
emitted the complete OSC 52 sequence unchanged. The sequence was captured as
bytes, not displayed on the user's terminal. Depending on terminal capabilities
and configuration, such output can change the clipboard or manipulate display.
Arbitrary command execution was not demonstrated.

Remediation: the top-level diagnostic formatter now visibly escapes control
characters while retaining diagnostic line breaks and ordinary Unicode.
Unit and PTY tests exercise hostile filenames and command-line arguments.

### 2. Medium, fixed: replacing an open file with a FIFO blocks save indefinitely

Location: `src/document.rs:51`; a related check/read race exists at
`src/document.rs:23` and `src/document.rs:29` during opening.

Save calls `fs::read_to_string` on the current path without verifying the opened
object is still a regular file. Another process able to replace the directory
entry can substitute a FIFO. The blocking open then freezes the event loop,
including quit and unsaved-change handling.

Reproduced against the release binary: open an ordinary file in a temporary
directory, replace it with a FIFO with no writer, send Ctrl+S and then Ctrl+Q.
The editor remained blocked and had to be killed by the test harness. No user
files were involved. This is a local availability issue requiring control of the
file's directory, rather than an attack through ordinary file text.

Remediation: both initial open and save validation use a shared regular-file
reader. Unix opens use nonblocking and no-follow flags, then inspect and read
the same handle. Save permissions also come from that handle, avoiding a
separate path metadata lookup. PTY regressions bound failures to three seconds
and check that save errors retain unsaved changes and allow normal quit.

The content comparison also leaves a separate check-to-replacement window:
a concurrent writer can change a file after comparison but before rename.
Atomic rename makes replacement atomic; it does not make the entire external
change check atomic. This residual race was identified by inspection, not
reproduced. Document the exact conflict-detection guarantee.

### 3. Medium, conditional: atomic saves discard security-relevant metadata

Location: `src/document.rs:62` and `src/document.rs:71`.

Save replaces the original inode with a temporary file and copies only
`std::fs::Permissions`. Ownership, POSIX ACLs, extended attributes, and security
labels are not copied. The README already discloses loss of extended metadata,
but this can affect confidentiality and access control, not just convenience.
For example, a named-user ACL denying access on an otherwise readable file is
lost on replacement; that user may gain access via the remaining mode bits.
Copying an ACL mask as group mode bits can also broaden group access.

This finding is based on the save implementation. The runtime ACL probe could
not be completed because this execution filesystem rejects POSIX ACL creation.
Actual impact depends on filesystem metadata and the original access rules.

Fix: preserve security-relevant metadata with platform-specific handling, or
refuse/warn before saving files whose protection cannot be preserved. Test on
an ACL-capable filesystem. Keep the temporary file private while writing it;
restore intended access only once contents and metadata are ready.

### 4. Dependency advisories: two soundness warnings and two maintenance warnings

All 102 third-party locked package entries were compared with the official
RustSec database. These four advisory/version combinations apply:

| Dependency path | Advisory | Assessment |
| --- | --- | --- |
| `docz -> ratatui 0.29.0 -> lru 0.12.5` | [RUSTSEC-2026-0002](https://rustsec.org/advisories/RUSTSEC-2026-0002.html) | Mutable iterator soundness issue; fixed in `>=0.16.3`. |
| `docz -> ratatui 0.29.0 -> lru 0.12.5` | [RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253.html) | Panic-safety use-after-free in `pop`; fixed in `>=0.18.2`. |
| `docz -> ratatui 0.29.0 -> paste 1.0.15` | [RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436.html) | Unmaintained build-time procedural macro dependency. |
| `docz -> syntect 5.3.0 -> bincode 1.3.3` | [RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141.html) | Unmaintained serialization dependency. |

The `lru` findings are dependency-level warnings, with no demonstrated docz
exploit. Inspection of Ratatui's layout cache found use of `get_or_insert`, not
the affected `IterMut` or `pop` APIs. The second advisory additionally requires
a panicking key destructor and caught unwinding, which were not found here.
Classify these as low apparent exposure in the current integration, while still
planning to remove the affected dependency version.

Syntect deserializes bundled syntax assets through bincode. Docz does not expose
an API to load untrusted serialized syntax dumps. The two unmaintained-crate
warnings do not establish a current exploitable vulnerability.

Fix: upgrade through compatible upstream releases so the resolved graph removes
affected versions, then rerun tests and the audit. A direct `lru` dependency at a
newer version will not necessarily replace Ratatui's incompatible `0.12` version.
If an advisory is accepted temporarily, record the reason and revisit it when
the integration changes. Add recurring dependency audit checks.

### 5. Low: resource use is unbounded for files, paste, and highlighting

Locations: `src/document.rs:29`, `src/clipboard.rs:106`,
`src/clipboard.rs:134`, `src/editor.rs:239`, and `src/syntax.rs:113`.

Files and clipboard contents are read completely. Undo can retain 100 full-buffer
snapshots, and highlighting runs synchronously through the viewport, including
preceding context. Large files, very long lines, many cursors, or large paste
operations can exhaust memory or stall input. Clipboard helpers have a 750 ms
process deadline, but their output file has no byte limit while the helper runs.

This is an architectural limitation found by inspection. Destructive memory or
disk exhaustion tests were not run. The README already describes ordinary-file
scope and the memory-heavy undo design.

Fix: add configurable file, line, paste, and history byte budgets, plus a
highlighting work limit or plain-text fallback. Bound clipboard output while it
is produced, not only after reading it back.

## Public GitHub repository readiness

- No secret-pattern matches in 16 tracked files, 36 Git blobs, five commits, or
  any local tag objects. All 52 local Git objects were inventoried, including
  unreachable objects. Historical tracked filenames were also checked; none
  matched sensitive configuration or key-file patterns.
- `target`, `.aws`, `.codex`, and `.agents` are ignored and absent from tracked
  history. `.gitignore` does not currently exclude `.env`, private-key files,
  or nested copies of sensitive configuration directories. Strengthen these
  rules and use secret scanning before future commits. Ignore rules do not
  remove material already committed.
- Git commit author metadata includes an email address. Existing remote state,
  remote visibility, remote-only history, and account settings were not audited.
- The initial audit found MIT declared in `Cargo.toml` without a root license
  file. Follow-up remediation adds the standard MIT text, with copyright
  `2026 patryk-zz`, matching the repository's existing author identity.
- All resolved packages have license metadata with permissive options. This is
  a metadata check, not a complete review of redistributed notices, bundled
  syntax assets, or binary distribution obligations.
- README installation and PATH statements describe the original machine. Make
  them generic so public users are not told prerequisites are already installed.
- Add `SECURITY.md` with a working vulnerability-reporting route, CI for the
  existing checks, dependency auditing, and dependency update automation.
  These are readiness improvements rather than evidence of credential exposure.

## Verification and coverage

| Check | Result |
| --- | --- |
| `cargo test --locked --offline` | 57 tests passed. |
| `cargo fmt --check` | Passed. |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | Passed. |
| `cargo build --release --locked --offline` | Passed. |
| `python3 scripts/terminal_smoke.py` | Passed, including terminal restoration and private clipboard helpers. |
| Crafted startup filename with terminal controls | Confirmed raw OSC 52 emission. |
| Terminal controls in document text | Tested OSC 52 and SGR content were filtered from rendered output. |
| Regular-file-to-FIFO substitution before save | Confirmed blocked save and unresponsive quit. |
| POSIX ACL preservation probe | Unverified: filesystem rejected ACL creation. |

Source review covered all first-party Rust modules, the PTY script, manifest,
lockfile, README, and ignore rules. Secret scanning used custom patterns for
private-key headers, major service token formats, credential-bearing URLs, and
credential assignments, plus historical filename inspection and manual review.
Neither Gitleaks nor TruffleHog was available; this was not an exhaustive
provider-specific or entropy-based secret scan. Ignored private configuration
contents were not read or uploaded.

`cargo-audit` was unavailable. Instead, a custom stable-version range comparison
parsed 1,257 advisories from the official
[RustSec advisory database](https://github.com/RustSec/advisory-db), selected
17 matching package advisories, and checked patched/unaffected ranges against
the lockfile. The four results above were also checked on RustSec's published
advisory pages. This does not claim a standard `cargo audit` pass.

- RustSec archive SHA-256:
  `597b7a79968ec902a690d3fdcfe22e9766ccdb9568e4541acfb7762d661f0ce7`
- Audited lockfile SHA-256:
  `f3fc32bd74459174354623dffeeb5653b8100e098fb87ab9abf642445e2bf96b`
- Runtime validation platform: Linux, Rust/Cargo 1.92.0.

Windows/macOS runtime behavior, ACL-capable filesystems, extensive fuzzing,
every dependency's implementation, and release binary provenance remain outside
this audit's tested coverage. Recheck advisories and secrets immediately before
publication, since both dependencies and repository contents can change.
