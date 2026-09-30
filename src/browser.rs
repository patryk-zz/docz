use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct Entry {
    pub path: PathBuf,
    pub label: String,
    pub is_dir: bool,
}

pub struct Browser {
    pub directory: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub scroll: usize,
}

impl Browser {
    pub fn open(directory: &Path) -> Result<Self> {
        let directory = fs::canonicalize(directory).context("Cannot resolve directory")?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(&directory).context("Cannot read directory")? {
            let entry = entry.context("Cannot read directory entry")?;
            let path = entry.path();
            // Skip sockets/devices and broken links; follow links to files/directories.
            if !(path.is_file() || path.is_dir()) {
                continue;
            }
            entries.push(Entry {
                is_dir: path.is_dir(),
                path,
                label: entry.file_name().to_string_lossy().into_owned(),
            });
        }
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
                .then_with(|| a.label.cmp(&b.label))
        });
        if let Some(parent) = directory.parent() {
            entries.insert(
                0,
                Entry {
                    path: parent.to_owned(),
                    label: "..".into(),
                    is_dir: true,
                },
            );
        }
        Ok(Self {
            directory,
            entries,
            selected: 0,
            scroll: 0,
        })
    }

    pub fn move_selection(&mut self, amount: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(amount)
            .min(self.entries.len().saturating_sub(1));
    }

    pub fn parent(&mut self) -> Result<()> {
        if let Some(parent) = self.directory.parent() {
            *self = Self::open(parent)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directories_first_including_hidden_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "").unwrap();
        fs::write(dir.path().join(".hidden"), "").unwrap();
        fs::create_dir(dir.path().join("z-dir")).unwrap();
        let b = Browser::open(dir.path()).unwrap();
        assert_eq!(
            b.entries
                .iter()
                .map(|e| e.label.as_str())
                .collect::<Vec<_>>(),
            vec!["..", "z-dir", ".hidden", "a.txt"]
        );
    }
}
