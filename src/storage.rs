use crate::device::{Endpoint, Equalizer};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Library {
    pub last_speaker: Option<Endpoint>,
    pub presets: Vec<Equalizer>,
}

#[cfg(windows)]
fn path() -> Option<PathBuf> {
    let root = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(root).join("OpenConneX").join("library.json"))
}

#[cfg(target_os = "linux")]
fn path() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".local/share"))
        })?;
    Some(root.join("OpenConneX").join("library.json"))
}

impl Library {
    pub fn load() -> Result<Self, String> {
        let Some(path) = path() else {
            return Ok(Self::default());
        };
        match fs::read(&path) {
            Ok(bytes) => {
                let library: Self = serde_json::from_slice(&bytes)
                    .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
                for preset in &library.presets {
                    preset.validate()?;
                }
                Ok(library)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = path().ok_or("Could not find the local app data directory.")?;
        let parent = path.parent().ok_or("Invalid local app data path.")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        // Preserve the original library if loading revealed a corrupt or unsupported file.
        if path.exists() {
            Self::load()?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        let tmp = parent.join(format!("library-{}.tmp", std::process::id()));
        fs::write(&tmp, bytes).map_err(|error| error.to_string())?;
        fs::rename(tmp, path).map_err(|error| error.to_string())
    }
}
