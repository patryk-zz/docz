use crate::buffer::Buffer;
use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub struct Document {
    pub path: PathBuf,
    pub buffer: Buffer,
    original: Option<String>,
    pub dirty: bool,
}

impl Document {
    pub fn open(path: &Path) -> Result<Self> {
        let path = if path.exists() {
            fs::canonicalize(path).with_context(|| format!("Cannot resolve {}", path.display()))?
        } else {
            std::env::current_dir()?.join(path)
        };
        let original = match fs::metadata(&path) {
            Ok(meta) => {
                if !meta.is_file() {
                    bail!("{} is not a regular file", path.display());
                }
                Some(
                    fs::read_to_string(&path)
                        .with_context(|| format!("Cannot read {} as UTF-8 text", path.display()))?,
                )
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).context("Cannot inspect file"),
        };
        let buffer = Buffer::from_text(original.as_deref().unwrap_or(""));
        Ok(Self {
            path,
            buffer,
            original,
            dirty: false,
        })
    }

    pub fn refresh_dirty(&mut self) {
        self.dirty = self.buffer.text() != self.original.as_deref().unwrap_or("");
    }

    pub fn save(&mut self) -> Result<()> {
        // Refuse to overwrite a file changed by another program since opening/saving.
        let disk = match fs::read_to_string(&self.path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).context("Cannot check file before saving"),
        };
        if disk != self.original {
            bail!("File changed on disk; reopen it before saving");
        }
        let parent = self.path.parent().context("File has no parent directory")?;
        let mut temp =
            tempfile::NamedTempFile::new_in(parent).context("Cannot create save file")?;
        if self.original.is_some() {
            temp.as_file()
                .set_permissions(fs::metadata(&self.path)?.permissions())?;
        }
        let text = self.buffer.text();
        temp.write_all(text.as_bytes())
            .context("Cannot write file")?;
        temp.as_file().sync_all().context("Cannot flush file")?;
        if self.original.is_some() {
            temp.persist(&self.path)
                .map_err(|e| e.error)
                .context("Cannot replace file")?;
        } else {
            temp.persist_noclobber(&self.path)
                .map_err(|e| e.error)
                .context("Cannot create file")?;
        }
        self.original = Some(text);
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Cursor;

    #[test]
    fn saves_new_file_and_preserves_original_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.txt");
        let mut doc = Document::open(&path).unwrap();
        doc.buffer.insert(&mut Cursor::default(), "hello\nworld");
        doc.refresh_dirty();
        assert!(doc.dirty);
        doc.save().unwrap();
        assert!(!doc.dirty);
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\nworld");
        fs::write(&path, "a\r\nb\n").unwrap();
        let mut doc = Document::open(&path).unwrap();
        doc.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "a\r\nb\n");
    }

    #[test]
    fn external_change_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        fs::write(&path, "original").unwrap();
        let mut doc = Document::open(&path).unwrap();
        fs::write(&path, "external change").unwrap();
        assert!(doc.save().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "external change");
    }

    #[test]
    fn rejects_non_text_and_directories() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Document::open(dir.path()).is_err());
        let path = dir.path().join("binary");
        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(Document::open(&path).is_err());
    }
}
