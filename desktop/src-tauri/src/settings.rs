use crate::{fail, Res};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub const FILE: &str = "settings.json";
const VERSION: u32 = 2;

/// PascalCase keys; unknown keys survive saves.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Settings {
    #[serde(default)]
    pub adb: String,
    #[serde(default)]
    pub ffmpeg: String,
    #[serde(default)]
    pub ffprobe: String,
    #[serde(default)]
    pub output: String,
    #[serde(default)]
    pub temp: String,
    #[serde(default = "dark")]
    pub theme: String,
    #[serde(default = "blue")]
    pub accent: String,
    #[serde(default = "yes")]
    pub check_updates: bool,
    #[serde(default)]
    pub skip_version: String,
    #[serde(default)]
    pub settings_version: u32,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn dark() -> String {
    "dark".into()
}

/// Preset accent colours; the palette itself lives in the stylesheet.
pub const ACCENTS: &[&str] = &["blue", "pink", "green", "purple", "orange", "teal"];

fn blue() -> String {
    "blue".into()
}

/// Unknown or missing values fall back to the default blue.
pub fn accent(value: &str) -> String {
    if ACCENTS.contains(&value) {
        value.into()
    } else {
        blue()
    }
}

fn yes() -> bool {
    true
}

impl Settings {
    pub fn defaults() -> Self {
        let videos = std::env::var("USERPROFILE").map(|p| Path::new(&p).join("Videos").join("BiliExports").to_string_lossy().into_owned()).unwrap_or_default();
        Settings {
            adb: String::new(),
            ffmpeg: String::new(),
            ffprobe: String::new(),
            output: videos,
            temp: String::new(),
            theme: dark(),
            accent: blue(),
            check_updates: true,
            skip_version: String::new(),
            settings_version: VERSION,
            extra: Map::new(),
        }
    }
}

/// Loads settings from the portable folder. A corrupt file is reported, never replaced silently.
pub fn load(dir: &Path) -> Res<Settings> {
    let path = dir.join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::defaults()),
        Err(e) => return fail(format!("無法讀取設定檔 {}：{e}", path.display())),
    };
    let mut settings: Settings = match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
        Ok(Value::Object(map)) => serde_json::from_value(Value::Object(map)).map_err(|e| crate::Error::Failed(corrupt(&path, &e.to_string())))?,
        Ok(_) => return fail(corrupt(&path, "內容不是設定物件")),
        Err(e) => return fail(corrupt(&path, &e.to_string())),
    };
    if settings.theme != "light" {
        settings.theme = dark();
    }
    settings.accent = accent(&settings.accent);
    Ok(settings)
}

fn corrupt(path: &Path, detail: &str) -> String {
    format!("設定檔損壞，無法讀取：{}（{detail}）。原檔保持未修改；可修正檔案，或在設定頁按「備份並重設」。", path.display())
}

/// Writes atomically.
pub fn save(dir: &Path, settings: &Settings) -> Res<()> {
    let path = dir.join(FILE);
    let unwritable = |e: std::io::Error| crate::Error::Failed(format!("設定無法儲存到 {}：{e}。請將整個程式資料夾移到可寫的位置。", dir.display()));
    let mut settings = settings.clone();
    settings.settings_version = VERSION;
    let temporary = dir.join(format!("{FILE}.tmp"));
    std::fs::write(&temporary, serde_json::to_string_pretty(&settings).unwrap()).map_err(unwritable)?;
    std::fs::rename(&temporary, &path).map_err(unwritable)
}

/// Moves a corrupt settings file aside (never deletes it) so the user can start from defaults.
pub fn backup_and_reset(dir: &Path) -> Res<Settings> {
    let path = dir.join(FILE);
    if path.exists() {
        let aside = dir.join(format!("settings.corrupt-{}.json", uuid::Uuid::new_v4().simple()));
        std::fs::rename(&path, &aside).map_err(|e| crate::Error::Failed(format!("無法備份損壞的設定檔：{e}")))?;
    }
    let settings = Settings::defaults();
    save(dir, &settings)?;
    Ok(settings)
}

/// Relative tool paths are portable: they resolve against the program folder, never PATH.
pub fn resolve(dir: &Path, value: &str) -> PathBuf {
    let path = Path::new(value.trim());
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    }
}

/// Stores paths inside the program folder relative to it, so moving the folder keeps tools found.
pub fn portable(dir: &Path, value: &str) -> String {
    let path = Path::new(value.trim());
    let base = dir.to_string_lossy().to_lowercase();
    let full = path.to_string_lossy();
    if path.is_absolute() && full.to_lowercase().starts_with(&(base.trim_end_matches('\\').to_string() + "\\")) {
        return full[base.trim_end_matches('\\').len() + 1..].to_string();
    }
    value.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_paths_round_trip() {
        let dir = Path::new(r"D:\Apps\Bili");
        assert_eq!(portable(dir, r"d:\apps\bili\tools\adb\adb.exe"), r"tools\adb\adb.exe");
        assert_eq!(portable(dir, r"D:\Apps\BiliOther\adb.exe"), r"D:\Apps\BiliOther\adb.exe");
        assert_eq!(resolve(dir, r"tools\adb\adb.exe"), Path::new(r"D:\Apps\Bili\tools\adb\adb.exe"));
    }
}
