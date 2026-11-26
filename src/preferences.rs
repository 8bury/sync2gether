use crate::i18n::Language;
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
}

impl Preferences {
    pub fn load() -> Self {
        config_dir()
            .map(|dir| Self::load_from(&dir.join(FILE)))
            .unwrap_or_default()
    }

    pub fn load_from(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        let dir = config_dir().ok_or_else(|| io::Error::other("config directory"))?;
        self.save_to(&dir.join(FILE))
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let partial = path.with_extension("json.partial");
        std::fs::write(
            &partial,
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?,
        )?;
        std::fs::rename(partial, path)
    }
}

const FILE: &str = "preferences.json";

pub fn config_dir() -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    #[cfg(target_os = "windows")]
    let base = var("APPDATA");
    #[cfg(target_os = "macos")]
    let base = var("HOME").map(|home| home.join("Library").join("Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|home| home.join(".config")));
    base.map(|base| base.join("sync2gether"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_round_trips_and_missing_or_broken_files_fall_back() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/preferences-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(FILE);
        assert_eq!(Preferences::load_from(&path), Preferences::default());
        let saved = Preferences {
            language: Some(Language::PtBr),
        };
        saved.save_to(&path).unwrap();
        assert_eq!(Preferences::load_from(&path), saved);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("\"pt-BR\"")
        );
        std::fs::write(&path, b"{ broken").unwrap();
        assert_eq!(Preferences::load_from(&path), Preferences::default());
        std::fs::write(&path, br#"{"language":"xx","future":1}"#).unwrap();
        assert_eq!(Preferences::load_from(&path), Preferences::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
