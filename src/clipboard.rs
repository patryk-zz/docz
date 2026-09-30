use anyhow::{Result, bail};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
enum Backend {
    Wayland,
    X11,
    Mac,
}

pub struct Clipboard {
    memory: Option<String>,
    backends: Vec<Backend>,
}

impl Clipboard {
    pub fn new() -> Self {
        let mut backends = Vec::new();
        if cfg!(target_os = "macos") {
            backends.push(Backend::Mac);
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            backends.push(Backend::Wayland);
        }
        if std::env::var_os("DISPLAY").is_some() {
            backends.push(Backend::X11);
        }
        Self {
            memory: None,
            backends,
        }
    }

    #[cfg(test)]
    pub fn internal() -> Self {
        Self {
            memory: None,
            backends: Vec::new(),
        }
    }

    /// Always preserve an internal copy even when desktop access is unavailable.
    pub fn copy(&mut self, text: &str) -> bool {
        self.memory = Some(text.to_owned());
        self.backends.iter().any(|backend| {
            let mut command = match backend {
                Backend::Wayland => {
                    let mut c = Command::new("wl-copy");
                    c.args(["--type", "text/plain;charset=utf-8"]);
                    c
                }
                Backend::X11 => {
                    let mut c = Command::new("xclip");
                    c.args(["-selection", "clipboard", "-in"]);
                    c
                }
                Backend::Mac => Command::new("pbcopy"),
            };
            transfer(&mut command, Some(text)).is_ok()
        })
    }

    pub fn paste(&mut self) -> Result<(String, bool)> {
        for backend in &self.backends {
            let mut command = match backend {
                Backend::Wayland => {
                    let mut c = Command::new("wl-paste");
                    c.args(["--no-newline", "--type", "text"]);
                    c
                }
                Backend::X11 => {
                    let mut c = Command::new("xclip");
                    c.args(["-selection", "clipboard", "-out"]);
                    c
                }
                Backend::Mac => Command::new("pbpaste"),
            };
            if let Ok(text) = transfer(&mut command, None) {
                self.memory = Some(text.clone());
                return Ok((text, true));
            }
        }
        self.memory
            .clone()
            .map(|text| (text, false))
            .ok_or_else(|| anyhow::anyhow!("Clipboard unavailable; copy some text in docz first"))
    }
}

/// Regular files avoid pipe deadlocks and allow a bounded wait on clipboard helpers.
fn transfer(command: &mut Command, input: Option<&str>) -> Result<String> {
    let mut data = tempfile::tempfile()?;
    if let Some(text) = input {
        data.write_all(text.as_bytes())?;
        data.seek(SeekFrom::Start(0))?;
        command
            .stdin(Stdio::from(data.try_clone()?))
            .stdout(Stdio::null());
    } else {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::from(data.try_clone()?));
    }
    let mut child = command.stderr(Stdio::null()).spawn()?;
    let deadline = Instant::now() + Duration::from_millis(750);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    bail!("Clipboard helper failed");
                }
                break;
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                if let Err(error) = result {
                    return Err(error.into());
                }
                bail!("Clipboard helper timed out");
            }
        }
    }
    if input.is_some() {
        return Ok(String::new());
    }
    data.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    data.read_to_string(&mut text)?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_fallback_preserves_unicode_and_exact_newlines() {
        let mut clipboard = Clipboard::internal();
        assert!(clipboard.paste().is_err());
        assert!(!clipboard.copy("é中\r\nlast\n"));
        assert_eq!(
            clipboard.paste().unwrap(),
            ("é中\r\nlast\n".to_owned(), false)
        );
    }

    #[cfg(unix)]
    #[test]
    fn helper_transfer_does_not_use_shell_interpolation_or_add_newlines() {
        let mut copy = Command::new("cat");
        assert!(transfer(&mut copy, Some("`x` $(x) é\n")).is_ok());
        let mut paste = Command::new("printf");
        paste.arg("é中\n");
        assert_eq!(transfer(&mut paste, None).unwrap(), "é中\n");
        assert!(transfer(&mut Command::new("false"), None).is_err());
    }
}
