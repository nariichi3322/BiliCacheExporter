use crate::storage::ensure_writable;
use crate::tools::{rename_retry, DownloadProgress, Fetch};
use crate::{fail, Cancel, Error, Res};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const REPO: &str = "nariichi3322/BiliCacheExporter";
pub const PROJECT_URL: &str = "https://github.com/nariichi3322/BiliCacheExporter";
pub const RELEASES_URL: &str = "https://github.com/nariichi3322/BiliCacheExporter/releases/latest";
/// Test-only override of the GitHub hosts, used by the packaged end-to-end check.
pub const SOURCE_ENV: &str = "BILI_UPDATE_SOURCE";

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub newer: bool,
}

const ASSET: &str = "BiliCacheExporter-win-x64.zip";
const SUMS: &str = "SHA256.txt";
const EXE_IN_ASSET: &str = "BiliCacheExporter-win-x64/BiliCacheExporter.exe";

/// An override of the GitHub hosts is honoured only for a local test server.
fn local(source: Option<&str>) -> Option<String> {
    source.filter(|s| *s == "http://127.0.0.1" || s.starts_with("http://127.0.0.1:")).map(|s| s.trim_end_matches('/').into())
}

pub fn api_origin(source: Option<&str>) -> String {
    local(source).unwrap_or_else(|| "https://api.github.com".into())
}

fn env_source() -> Option<String> {
    std::env::var(SOURCE_ENV).ok()
}

/// `v2.1.0` or `2.1.0+abc` → (2, 1, 0); anything else is not a release version.
pub fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.trim().trim_start_matches('v').split('+').next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

/// Reads the latest published release (GitHub already excludes drafts and pre-releases).
pub fn check(fetch: &Fetch, current: &str) -> Res<Release> {
    latest(fetch, current).map(|(release, _)| release).map_err(|e| Error::Failed(format!("檢查更新失敗：{e}")))
}

/// The release and the size of its package asset (0 when the release has none).
fn latest(fetch: &Fetch, current: &str) -> Res<(Release, u64)> {
    let url = format!("{}/repos/{REPO}/releases/latest", api_origin(env_source().as_deref()));
    let dest = std::env::temp_dir().join(format!("bili-release-{}.json", uuid::Uuid::new_v4().simple()));
    let result = fetch(&url, &dest, 1 << 20, &Cancel::default(), &mut |_| {}).and_then(|()| read_release(&dest, current));
    let _ = std::fs::remove_file(&dest);
    result
}

fn read_release(path: &Path, current: &str) -> Res<(Release, u64)> {
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?).map_err(|e| Error::Failed(format!("回應無法解析：{e}")))?;
    let tag = json["tag_name"].as_str().unwrap_or_default();
    let (Some(latest), Some(mine)) = (parse(tag), parse(current)) else { return fail(format!("無法辨識版本號：{tag}")) };
    let size = json["assets"].as_array().into_iter().flatten().find(|a| a["name"] == ASSET).and_then(|a| a["size"].as_u64()).unwrap_or(0);
    let release = Release {
        version: format!("{}.{}.{}", latest.0, latest.1, latest.2),
        notes: json["body"].as_str().unwrap_or_default().replace("\r\n", "\n"),
        newer: latest > mine,
    };
    Ok((release, size))
}

/// Where the previous executable waits until the next start removes it.
pub fn old_path(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    exe.with_file_name(name)
}

/// Removes executables left behind by a previous update; one still in use is retried next start.
pub fn clean_old(dir: &Path) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().to_lowercase().ends_with(".exe.old") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Downloads `version`, verifies it against the release's SHA256.txt and swaps it in for `exe`.
/// Only the executable changes; settings, tools and every other file stay as they are.
pub fn install(exe: &Path, version: &str, fetch: &Fetch, cancel: &Cancel, progress: &mut dyn FnMut(DownloadProgress)) -> Res<()> {
    let dir = exe.parent().ok_or_else(|| Error::Failed("找不到程式資料夾。".into()))?;
    ensure_writable(dir)?;
    let staging = dir.join(format!(".update-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&staging)?;
    let result = install_staged(exe, &staging, version, fetch, cancel, progress);
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn install_staged(exe: &Path, staging: &Path, version: &str, fetch: &Fetch, cancel: &Cancel, progress: &mut dyn FnMut(DownloadProgress)) -> Res<()> {
    let (release, size) = latest(fetch, crate::VERSION)?;
    if release.version != version {
        return fail(format!("最新版本已變為 {}，請重新檢查更新。", release.version));
    }
    if size == 0 {
        return fail(format!("版本 {version} 沒有提供 {ASSET}。"));
    }
    let mut report = |stage: &str, bytes: u64| progress(DownloadProgress { name: "update".into(), stage: stage.into(), bytes, total: size });
    let base = format!("{}/{REPO}/releases/download/v{version}", local(env_source().as_deref()).unwrap_or_else(|| "https://github.com".into()));
    report("下載", 0);
    let sums = staging.join(SUMS);
    fetch(&format!("{base}/{SUMS}"), &sums, 64 * 1024, cancel, &mut |_| {})?;
    let archive = staging.join(ASSET);
    fetch(&format!("{base}/{ASSET}"), &archive, size, cancel, &mut |bytes| report("下載", bytes))?;
    cancel.check()?;
    report("檢查完整性", size);
    let expected = std::fs::read_to_string(&sums)?
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            (parts.next()? == ASSET).then(|| hash.to_lowercase())
        })
        .ok_or_else(|| Error::Failed(format!("{SUMS} 沒有 {ASSET} 的雜湊值。")))?;
    let bytes = std::fs::read(&archive)?;
    if format!("{:x}", Sha256::digest(&bytes)) != expected {
        return fail("下載檔案的 SHA-256 與發行記錄不符，已拒絕安裝。請重試或改為手動下載。");
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| Error::Failed(format!("壓縮檔無法讀取：{e}")))?;
    let mut entry = zip.by_name(EXE_IN_ASSET).map_err(|_| Error::Failed(format!("壓縮檔缺少 {EXE_IN_ASSET}。")))?;
    let fresh = staging.join("new.exe");
    let mut file = std::fs::File::create(&fresh)?;
    std::io::copy(&mut entry, &mut file).map_err(|e| Error::Failed(format!("解壓縮寫入失敗（磁碟空間或權限）：{e}")))?;
    file.sync_all()?;
    drop(file);
    cancel.check()?;
    report("替換", size);
    swap(exe, &fresh)
}

/// A running executable can be renamed but not overwritten, so it moves aside first; any failure puts it back.
fn swap(exe: &Path, fresh: &Path) -> Res<()> {
    let old = old_path(exe);
    if old.exists() {
        std::fs::remove_file(&old).map_err(|e| Error::Failed(format!("無法移除上次更新留下的 {}：{e}", old.display())))?;
    }
    rename_retry(exe, &old).map_err(|e| Error::Failed(format!("無法替換執行檔：{e}")))?;
    if let Err(e) = rename_retry(fresh, exe) {
        let _ = rename_retry(&old, exe);
        return fail(format!("無法放入新版本，已保留原本的程式：{e}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically_and_ignore_build_info() {
        assert_eq!(parse("v2.10.0"), Some((2, 10, 0)));
        assert_eq!(parse("2.0.2+4d54bd7"), Some((2, 0, 2)));
        assert!(parse("v2.10.0") > parse("2.9.9+x"));
        assert_eq!(parse("v2.1"), None);
        assert_eq!(parse("v2.1.0-beta"), None);
    }

    #[test]
    fn failed_swap_restores_the_original_executable() {
        let dir = std::env::temp_dir().join(format!("bili-swap-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("BiliCacheExporter.exe");
        std::fs::write(&exe, b"old").unwrap();
        assert!(swap(&exe, &dir.join("missing.exe")).is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
        assert!(!old_path(&exe).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn override_accepts_only_the_local_test_server() {
        assert_eq!(api_origin(None), "https://api.github.com");
        assert_eq!(api_origin(Some("http://127.0.0.1:8123/")), "http://127.0.0.1:8123");
        for bad in ["https://evil.example", "http://127.0.0.1.evil.example", "http://localhost:80", "http://127.0.0.10:1"] {
            assert_eq!(api_origin(Some(bad)), "https://api.github.com", "{bad}");
        }
    }
}
