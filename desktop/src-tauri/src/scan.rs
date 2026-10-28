use crate::runner::{strings, Runner};
use crate::{fail, Cancel, Error, Res};
use base64::Engine;
use serde::Serialize;
use serde_json::Value;

pub const ROOT: &str = "/sdcard/Android/data/com.bilibili.app.in/download/";
pub const CHINA_ROOT: &str = "/sdcard/Android/data/tv.danmaku.bili/download/";
/// Supported apps: (package, cache root, label). Both share the same cache layout.
pub const APPS: [(&str, &str, &str); 2] = [("com.bilibili.app.in", ROOT, "國際版"), ("tv.danmaku.bili", CHINA_ROOT, "中國版")];

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Device {
    pub serial: String,
    pub state: String,
    pub detail: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    pub video: String,
    pub audio: String,
    pub video_size: u64,
    pub audio_size: u64,
    /// Full metadata for 快取詳情, e.g. "80（快取目錄） · bandwidth=…, width=1920".
    pub quality: String,
    /// Short form for the row, e.g. "1920×1080"; the quality menu prefixes the cache directory name.
    pub label: String,
    pub issue: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub path: String,
    pub size: Option<u64>,
    pub error: String,
}

/// One part/episode row. `issue` non-empty means the row cannot be exported as a whole;
/// a multi-quality row instead has `needs_choice` and per-variant issues.
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CacheItem {
    pub id: usize,
    pub title: String,
    pub remote: String,
    pub variants: Vec<Variant>,
    /// Index of the only usable variant, chosen automatically.
    pub auto_variant: Option<usize>,
    pub needs_choice: bool,
    pub issue: String,
    pub quality: String,
    pub cover: Option<String>,
    pub has_cover: bool,
    /// Drawable comments in `remote`'s danmaku.xml; 0 when there is none, it is disabled or unreadable.
    pub danmaku: usize,
    pub inventory: Vec<FileInfo>,
    /// Which app's cache the row came from, e.g. "中國版（tv.danmaku.bili）".
    pub source: String,
    /// The output folder already holds `<title>.mp4` at scan time (name match only).
    pub exported: bool,
}

impl CacheItem {
    pub fn exportable_variant(&self, choice: Option<usize>) -> Res<&Variant> {
        if !self.issue.is_empty() {
            return fail(self.issue.clone());
        }
        let index = match (self.auto_variant, choice) {
            (Some(auto), None) => auto,
            (_, Some(chosen)) => chosen,
            (None, None) => return fail("請先為此項目選擇畫質版本。"),
        };
        match self.variants.get(index) {
            Some(v) if v.issue.is_empty() => Ok(v),
            _ => fail("此畫質版本無法匯出，請選擇完整的版本。"),
        }
    }
}

pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub fn is_child_path(path: &str, parent: &str) -> bool {
    path.starts_with(&format!("{}/", parent.trim_end_matches('/'))) && !path.chars().any(char::is_control) && !path.split('/').any(|p| p == "." || p == "..")
}

pub fn parse_devices(text: &str) -> Vec<Device> {
    text.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with("List of") && !s.starts_with('*'))
        .filter_map(|s| {
            let parts: Vec<&str> = s.split_whitespace().collect();
            (parts.len() >= 2).then(|| Device { serial: parts[0].into(), state: parts[1].into(), detail: parts[2..].join(" ") })
        })
        .collect()
}

pub struct Adb<'a> {
    pub runner: &'a Runner,
    pub exe: String,
}

impl Adb<'_> {
    pub fn devices(&self, cancel: &Cancel) -> Res<Vec<Device>> {
        Ok(parse_devices(&self.runner.run(&self.exe, &strings(&["devices", "-l"]), cancel, &mut |_| {}, 15)?))
    }

    pub fn shell(&self, serial: &str, command: &str, cancel: &Cancel) -> Res<String> {
        self.runner.run(&self.exe, &strings(&["-s", serial, "shell", command]), cancel, &mut |_| {}, 180)
    }

    pub fn pull(&self, serial: &str, remote: &str, local: &str, cancel: &Cancel, progress: &mut dyn FnMut(&str)) -> Res<String> {
        self.runner.run(&self.exe, &strings(&["-s", serial, "pull", remote, local]), cancel, progress, 7200)
    }

    fn size(&self, serial: &str, path: &str, cancel: &Cancel) -> Res<u64> {
        let value = self.shell(serial, &format!("stat -c %s {}", quote(path)), cancel)?;
        value.trim().parse::<u64>().map_err(|_| Error::Failed(format!("無法取得檔案大小：{path}")))
    }

    /// Every file under `dir` with its size, in one adb call. If that call fails (e.g. a file vanished mid-scan),
    /// falls back to one stat per file so a single bad file only marks its own inventory line.
    fn list_sizes(&self, serial: &str, dir: &str, cancel: &Cancel) -> Res<Vec<(String, Result<u64, String>)>> {
        let mut files: Vec<(String, Result<u64, String>)> = Vec::new();
        match self.shell(serial, &format!("find {} -type f -exec stat -c '%s %n' {{}} +", quote(dir)), cancel) {
            Ok(out) => {
                for line in out.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.is_empty()) {
                    let Some((size, path)) = line.split_once(' ') else { return fail("手機回傳無法解析的檔案清單。") };
                    if !files.iter().any(|(f, _)| f == path) {
                        files.push((path.into(), size.parse().map_err(|_| format!("無法取得檔案大小：{path}"))));
                    }
                }
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                self.runner.log(&format!("一次盤點失敗，改為逐檔查詢大小：{e}"));
                let listing = self.shell(serial, &format!("find {} -type f", quote(dir)), cancel)?;
                for f in listing.lines().map(|p| p.trim_end_matches('\r')).filter(|p| !p.is_empty()) {
                    if !files.iter().any(|(x, _)| x == f) {
                        let size = match self.size(serial, f, cancel) {
                            Err(Error::Cancelled) => return Err(Error::Cancelled),
                            r => r.map_err(|e| e.to_string()),
                        };
                        files.push((f.into(), size));
                    }
                }
            }
        }
        Ok(files)
    }

    /// Read-only scan of every installed supported app's cache. Problems stay on their own row.
    pub fn scan(&self, serial: &str, cancel: &Cancel, found: &mut dyn FnMut(&CacheItem)) -> Res<Vec<CacheItem>> {
        let mut installed = Vec::new();
        for app in APPS {
            let package = self.shell(serial, &format!("pm list packages --user 0 {}", app.0), cancel)?;
            if package.lines().any(|s| s.trim() == format!("package:{}", app.0)) {
                installed.push(app);
            }
        }
        if installed.is_empty() {
            return fail("Android user 0 未找到 Bilibili 國際版 com.bilibili.app.in 或中國版 tv.danmaku.bili。");
        }
        let mut items: Vec<CacheItem> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for &(package, root, label) in &installed {
            let paths = self.shell(serial, &format!("find {} -type f -name entry.json", quote(root)), cancel)?;
            for entry in paths.lines().map(|p| p.trim_end_matches('\r')).filter(|p| !p.is_empty()) {
                if !seen.insert(entry.to_string()) {
                    continue;
                }
                cancel.check()?;
                let mut item = self.scan_entry(serial, entry, root, cancel)?;
                item.source = format!("{label}（{package}）");
                // The same part cached by both apps has the same `<id>/c_<cid>` path, so it becomes one row.
                let same = relative(&item.remote).and_then(|key| items.iter().position(|i| relative(&i.remote) == Some(key)));
                if let Some(index) = same {
                    merge(&mut items[index], item, label);
                    found(&items[index]);
                    continue;
                }
                item.id = items.len();
                found(&item);
                items.push(item);
            }
        }
        Ok(items)
    }

    fn scan_entry(&self, serial: &str, entry: &str, root: &str, cancel: &Cancel) -> Res<CacheItem> {
        Ok(if !is_child_path(entry, root) || !entry.ends_with("/entry.json") {
            // Untrusted paths are never kept as a source; the row only reports the problem.
            self.runner.log(&format!("略過不合法的快取路徑：{}", serde_json::to_string(entry).unwrap()));
            CacheItem { title: "不合法的快取路徑".into(), issue: "掃描失敗：手機回傳不合法的快取路徑。".into(), ..Default::default() }
        } else {
            let remote = &entry[..entry.rfind('/').unwrap()];
            let mut item = CacheItem { remote: remote.into(), title: remote[remote.rfind('/').unwrap() + 1..].into(), ..Default::default() };
            match self.scan_item(serial, entry, &mut item, cancel) {
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => {
                    item.issue = format!("掃描失敗：{e}");
                    self.runner.log(&item.issue);
                }
                Ok(()) => {}
            }
            item
        })
    }

    fn scan_item(&self, serial: &str, entry: &str, item: &mut CacheItem, cancel: &Cancel) -> Res<()> {
        let remote = item.remote.clone();
        let json = self.shell(serial, &format!("cat {}", quote(entry)), cancel)?;
        match serde_json::from_str::<Value>(&json) {
            Ok(doc) => {
                item.title = title(&doc, &item.title);
                if doc.get("is_completed").is_some_and(|c| *c == Value::Bool(false) || *c == 0 || *c == "0") {
                    item.issue = "下載尚未完成".into();
                }
            }
            Err(e) => {
                item.issue = format!("掃描失敗：entry.json 無法解析：{e}");
                self.runner.log(&item.issue);
            }
        }
        let sized = self.list_sizes(serial, &remote, cancel)?;
        if sized.iter().any(|(p, _)| !is_child_path(p, &remote)) {
            return fail("手機回傳不合法的項目檔案路徑。");
        }
        let files: Vec<&String> = sized.iter().map(|(p, _)| p).collect();
        let has = |p: &str| files.iter().any(|f| *f == p);
        let size = |p: &str| sized.iter().find(|(f, _)| f == p).map(|(_, s)| s.clone().map_err(Error::Failed)).unwrap();
        let mut quality_dirs: Vec<&str> = Vec::new();
        for f in &files {
            if f.ends_with("/video.m4s") || f.ends_with("/audio.m4s") {
                let dir = &f[..f.rfind('/').unwrap()];
                if !quality_dirs.contains(&dir) {
                    quality_dirs.push(dir);
                }
            }
        }
        for dir in quality_dirs {
            let name = &dir[dir.rfind('/').unwrap() + 1..];
            let mut variant =
                Variant { issue: item.issue.clone(), quality: format!("{name}（快取目錄）"), label: "解析度未知".into(), ..Default::default() };
            let result: Res<()> = (|| {
                variant.video = if has(&format!("{dir}/video.m4s")) { format!("{dir}/video.m4s") } else { String::new() };
                variant.audio = if has(&format!("{dir}/audio.m4s")) { format!("{dir}/audio.m4s") } else { String::new() };
                if variant.video.is_empty() || variant.audio.is_empty() {
                    variant.issue = "缺少 video.m4s 或 audio.m4s".into();
                } else {
                    variant.video_size = size(&variant.video)?;
                    variant.audio_size = size(&variant.audio)?;
                    if variant.video_size == 0 || variant.audio_size == 0 {
                        variant.issue = "影音檔案為空".into();
                    }
                }
                if has(&format!("{dir}/index.json")) {
                    // Optional quality metadata: failure degrades the label but never blocks complete media.
                    match self.shell(serial, &format!("cat {}", quote(&format!("{dir}/index.json"))), cancel).and_then(|j| Ok((quality(&j)?, resolution(&j)))) {
                        Ok((q, r)) => {
                            variant.quality += &format!(" · {q}");
                            if let Some(r) = r {
                                variant.label = r;
                            }
                        }
                        Err(Error::Cancelled) => return Err(Error::Cancelled),
                        Err(e) => {
                            self.runner.log(&format!("index.json 讀取／解析失敗：{e}"));
                            variant.quality += " · 畫質資料無法讀取／解析";
                            variant.label = "畫質資料無法讀取".into();
                        }
                    }
                }
                Ok(())
            })();
            match result {
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => {
                    variant.issue = format!("版本掃描失敗：{e}");
                    self.runner.log(&variant.issue);
                }
                Ok(()) => {}
            }
            if !item.issue.is_empty() && variant.issue != item.issue {
                variant.issue = format!("{}{}", item.issue, if variant.issue.is_empty() { String::new() } else { format!("；{}", variant.issue) });
            }
            item.variants.push(variant);
        }
        for (file, size) in &sized {
            match size {
                Ok(size) => item.inventory.push(FileInfo { path: file[remote.len() + 1..].into(), size: Some(*size), error: String::new() }),
                Err(e) => {
                    self.runner.log(&format!("盤點大小失敗：{file}：{e}"));
                    item.inventory.push(FileInfo { path: file[remote.len() + 1..].into(), size: None, error: e.clone() });
                }
            }
        }
        settle(item);
        item.has_cover = has(&format!("{remote}/cover.jpg"));
        if item.has_cover {
            match self.read_cover(serial, &format!("{remote}/cover.jpg"), cancel) {
                Ok(cover) => item.cover = cover,
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => self.runner.log(&format!("封面讀取失敗：{e}")),
            }
        }
        if has(&format!("{remote}/danmaku.xml")) {
            match self.shell(serial, &format!("cat {}", quote(&format!("{remote}/danmaku.xml"))), cancel) {
                Ok(xml) => item.danmaku = crate::danmaku::count(&xml),
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => self.runner.log(&format!("彈幕讀取失敗：{e}")),
            }
        }
        Ok(())
    }

    fn read_cover(&self, serial: &str, remote: &str, cancel: &Cancel) -> Res<Option<String>> {
        let temp = std::env::temp_dir().join(format!("BiliCover-{}.jpg", uuid::Uuid::new_v4().simple()));
        let result = self.pull(serial, remote, &temp.to_string_lossy(), cancel, &mut |_| {}).and_then(|_| Ok(std::fs::read(&temp)?));
        if temp.exists() {
            if let Err(e) = std::fs::remove_file(&temp) {
                self.runner.log(&format!("封面暫存清理失敗：{e}"));
            }
        }
        let bytes = result?;
        let mime = if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            "image/jpeg"
        } else if bytes.starts_with(b"\x89PNG") {
            "image/png"
        } else {
            return fail("封面不是 JPEG／PNG 圖片");
        };
        // shortcut: list thumbnails use the original bytes; downscale if very large covers make scans slow.
        if bytes.len() > 4 * 1024 * 1024 {
            return fail("封面過大，略過清單縮圖");
        }
        Ok(Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
    }
}

/// Path below the supported app's cache root, e.g. `<id>/c_<cid>`; `None` outside every root.
pub fn relative(path: &str) -> Option<&str> {
    APPS.iter().find(|app| is_child_path(path, app.1)).map(|app| &path[app.1.len()..])
}

/// Derives the row's choice and issue from its variants.
fn settle(item: &mut CacheItem) {
    let usable: Vec<usize> = (0..item.variants.len()).filter(|&i| item.variants[i].issue.is_empty()).collect();
    if usable.len() == 1 {
        item.auto_variant = Some(usable[0]);
        item.quality = item.variants[usable[0]].label.clone();
    } else if item.variants.len() > 1 {
        item.quality = format!("{} 個版本（需選擇畫質）", item.variants.len());
        if item.issue.is_empty() {
            if usable.is_empty() {
                item.issue = "沒有完整可匯出的畫質版本".into();
            } else {
                item.needs_choice = true;
            }
        }
    } else if item.variants.len() == 1 {
        item.issue = item.variants[0].issue.clone();
        item.quality = item.variants[0].label.clone();
    } else if item.issue.is_empty() {
        item.issue = "缺少分離影音；此格式待辨識／不支援".into();
    }
}

/// Folds the same part from another app into `into`: every distinct quality joins its menu,
/// and for a quality both apps hold, a complete copy wins.
fn merge(into: &mut CacheItem, other: CacheItem, label: &str) {
    // Quality labels start with the cache directory name, e.g. "80（快取目錄）…".
    let dir = |v: &Variant| v.quality.split('（').next().unwrap_or_default().to_string();
    for v in other.variants {
        match into.variants.iter().position(|x| dir(x) == dir(&v)) {
            Some(i) if !into.variants[i].issue.is_empty() && v.issue.is_empty() => into.variants[i] = v,
            Some(_) => {}
            None => into.variants.push(v),
        }
    }
    into.inventory.extend(other.inventory.into_iter().map(|f| FileInfo { path: format!("{label}：{}", f.path), ..f }));
    into.source = format!("{}、{}", into.source, other.source);
    if !into.has_cover && other.has_cover {
        // Export reads the cover from `remote`, so point it at the app that has one.
        (into.remote, into.cover, into.has_cover, into.danmaku) = (other.remote, other.cover, true, other.danmaku);
    }
    if into.variants.iter().any(|v| v.issue.is_empty()) {
        into.issue.clear();
    } else if into.issue.is_empty() {
        into.issue = other.issue;
    }
    (into.auto_variant, into.needs_choice, into.quality) = (None, false, String::new());
    settle(into);
}

fn field(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

pub fn title(entry: &Value, fallback: &str) -> String {
    let mut title = field(entry, "title");
    if title.trim().is_empty() {
        title = fallback.into();
    }
    let part = entry.get("page_data").filter(|p| p.is_object()).map(|p| field(p, "part")).unwrap_or_default();
    if part.trim().is_empty() || part == title {
        title
    } else {
        format!("{title} — {part}")
    }
}

pub fn quality(json: &str) -> Res<String> {
    let doc: Value = serde_json::from_str(json).map_err(|e| Error::Failed(e.to_string()))?;
    let mut result: Vec<String> = Vec::new();
    fn walk(e: &Value, result: &mut Vec<String>) {
        match e {
            Value::Object(map) => {
                for (k, v) in map {
                    if ["quality", "description", "width", "height", "codecs", "mimeType", "bandwidth"].contains(&k.as_str()) && !v.is_object() && !v.is_array()
                    {
                        let text = format!("{k}={}", v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()));
                        if !result.contains(&text) {
                            result.push(text);
                        }
                    } else {
                        walk(v, result);
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|c| walk(c, result)),
            _ => {}
        }
    }
    walk(&doc, &mut result);
    Ok(if result.is_empty() { "index.json 未提供可辨識畫質欄位".into() } else { result.join(", ") })
}

/// Largest video resolution in index.json, e.g. "1920×1080"; audio streams report 0×0 and never win.
pub fn resolution(json: &str) -> Option<String> {
    fn walk(e: &Value, best: &mut (u64, u64)) {
        match e {
            Value::Object(map) => {
                if let (Some(w), Some(h)) = (map.get("width").and_then(Value::as_u64), map.get("height").and_then(Value::as_u64)) {
                    if w.saturating_mul(h) > best.0.saturating_mul(best.1) {
                        *best = (w, h);
                    }
                }
                map.values().for_each(|v| walk(v, best));
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, best)),
            _ => {}
        }
    }
    let mut best = (0, 0);
    walk(&serde_json::from_str(json).ok()?, &mut best);
    (best.0 > 0 && best.1 > 0).then(|| format!("{}×{}", best.0, best.1))
}

/// Windows-safe output name: illegal characters, reserved device names, trailing dots and length.
pub fn file_name(title: &str) -> String {
    let mut name: String = title.chars().map(|c| if "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20 { '_' } else { c }).collect();
    name = name.trim().trim_end_matches(['.', ' ']).to_string();
    if name.chars().count() > 110 {
        name = name.chars().take(110).collect();
    }
    name = name.trim_end_matches(['.', ' ']).to_string();
    if name.is_empty() {
        name = "video".into();
    }
    let stem = name.split('.').next().unwrap_or("").to_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.chars().count() == 4 && "0123456789¹²³".contains(stem.chars().nth(3).unwrap()));
    if reserved {
        name = format!("_{name}");
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_paths() {
        assert_eq!(file_name("中文:影片 — 第一集"), "中文_影片 — 第一集");
        assert_eq!(file_name("con.txt"), "_con.txt");
        assert_eq!(file_name("COM¹"), "_COM¹");
        assert_eq!(file_name(" .. "), "video");
        assert_eq!(file_name(&"字".repeat(200)).chars().count(), 110);
        assert!(is_child_path(&format!("{ROOT}1/c_1/entry.json"), ROOT));
        assert!(!is_child_path(&format!("{ROOT}1/../x/entry.json"), ROOT));
        assert!(!is_child_path("/sdcard/other/entry.json", ROOT));
        assert!(is_child_path(&format!("{CHINA_ROOT}1/c_1/entry.json"), CHINA_ROOT));
        assert!(!is_child_path(&format!("{CHINA_ROOT}1/c_1/entry.json"), ROOT));
        assert_eq!(quote("a'b"), "'a'\"'\"'b'");
        assert_eq!(parse_devices("List of devices attached\nA device model:X\nB unauthorized\n* daemon\n").len(), 2);
    }

    #[test]
    fn resolution_skips_audio_and_takes_the_largest_video() {
        let index = r#"{"video":[{"width":1280,"height":720},{"width":1920,"height":1080}],"audio":[{"bandwidth":1,"width":0,"height":0}]}"#;
        assert_eq!(resolution(index).as_deref(), Some("1920×1080"));
        assert_eq!(resolution(r#"{"audio":[{"width":0,"height":0}]}"#), None);
        assert_eq!(resolution("{ corrupt"), None);
    }
}
