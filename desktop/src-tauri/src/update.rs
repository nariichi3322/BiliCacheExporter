use crate::log::Logger;
use crate::storage::{ensure_writable, guard};
use crate::tools::{rename_retry, DownloadProgress, Fetch};
use crate::{fail, Cancel, Error, Res};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

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

const MARKER: &str = ".bili-owned";

/// Removes only completed, owned staging folders; returns false while one is still in use.
fn remove_old(dir: &Path) -> bool {
    let mut done = true;
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|n| n.strip_prefix(".update-")) else { continue };
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            continue;
        }
        let path = entry.path();
        if !entry.file_type().is_ok_and(|t| t.is_dir()) || guard(&path).is_err() {
            continue;
        }
        done &= clean_staging(&path, id);
    }
    done
}

fn clean_staging(path: &Path, id: &str) -> bool {
    let marker = path.join(MARKER);
    match std::fs::symlink_metadata(&marker) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let _ = std::fs::remove_dir(path);
            return true;
        }
        Ok(meta) if meta.is_file() && guard(&marker).is_ok() => {}
        _ => return true,
    }
    let Ok(mut lock) = OpenOptions::new().read(true).share_mode(0).open(&marker) else { return false };
    let mut contents = Vec::new();
    if (&mut lock).take(33).read_to_end(&mut contents).is_err() || contents != id.as_bytes() {
        return true;
    }
    let Ok(files) = std::fs::read_dir(path).and_then(|items| items.collect::<std::io::Result<Vec<_>>>()) else { return false };
    if files.iter().any(|e| !e.file_type().is_ok_and(|t| t.is_file()) || guard(&e.path()).is_err()) {
        return true;
    }
    let mut done = true;
    for file in files.iter().filter(|e| e.file_name() != MARKER) {
        done &= std::fs::remove_file(file.path()).is_ok();
    }
    let only_marker = std::fs::read_dir(path)
        .and_then(|items| items.collect::<std::io::Result<Vec<_>>>())
        .is_ok_and(|items| items.len() == 1 && items[0].file_name() == MARKER);
    if !done || !only_marker {
        return false;
    }
    drop(lock);
    std::fs::remove_file(marker).is_ok() && std::fs::remove_dir(path).is_ok()
}

/// Right after an update the previous process may still be exiting, so a locked file is retried for a while.
pub fn clean_old(dir: &Path) {
    if remove_old(dir) {
        return;
    }
    let dir = dir.to_path_buf();
    std::thread::spawn(move || {
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if remove_old(&dir) {
                break;
            }
        }
    });
}

/// Downloads `version`, verifies it against the release's SHA256.txt and swaps it in for `exe`.
/// Only the executable changes; settings, tools and every other file stay as they are.
pub fn install(exe: &Path, version: &str, fetch: &Fetch, log: &Logger, cancel: &Cancel, progress: &mut dyn FnMut(DownloadProgress)) -> Res<()> {
    let dir = exe.parent().ok_or_else(|| Error::Failed("找不到程式資料夾。".into()))?;
    ensure_writable(dir)?;
    with_staging(dir, log, |staging| install_staged(exe, staging, version, fetch, cancel, progress))
}

/// Runs `operation` in a new staging folder whose marker stays locked and empty until the operation has succeeded.
/// A staging folder that holds `old.exe` is never removed here: only the next start may, and only with a published marker.
fn with_staging(dir: &Path, log: &Logger, operation: impl FnOnce(&Path) -> Res<()>) -> Res<()> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let staging = dir.join(format!(".update-{id}"));
    std::fs::create_dir(&staging)?;
    let mut marker = match OpenOptions::new().write(true).create_new(true).share_mode(0).open(staging.join(MARKER)) {
        Ok(file) => file,
        Err(e) => {
            let _ = std::fs::remove_dir(&staging);
            return Err(e.into());
        }
    };
    let result = operation(&staging);
    if result.is_ok() {
        if let Err(e) = marker.write_all(id.as_bytes()).and_then(|()| marker.sync_all()) {
            // A failed publication must not authorize cleanup of the original executable.
            let _ = marker.set_len(0);
            log.log(&format!("更新成功，但暫存資料夾 {} 的標記寫入失敗，之後不會自動清理：{e}", staging.display()));
        }
        drop(marker);
    } else {
        drop(marker);
        if std::fs::symlink_metadata(staging.join("old.exe")).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
            let _ = std::fs::remove_dir_all(&staging);
        } else if let Err(e) = &result {
            log.log(&e.to_string());
        }
    }
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
    let expected = hash_for(&std::fs::read_to_string(&sums)?).ok_or_else(|| Error::Failed(format!("{SUMS} 沒有 {ASSET} 的雜湊值。")))?;
    let bytes = std::fs::read(&archive)?;
    if format!("{:x}", Sha256::digest(&bytes)) != expected {
        return fail("下載檔案的 SHA-256 與發行記錄不符，已拒絕安裝。請重試或改為手動下載。");
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| Error::Failed(format!("壓縮檔無法讀取：{e}")))?;
    // Windows PowerShell 5's Compress-Archive writes `\` separators; pwsh (the release workflow) writes `/`.
    let index = (0..zip.len())
        .find(|&i| zip.name_for_index(i).is_some_and(|n| n.is_ok_and(|n| n.replace('\\', "/") == EXE_IN_ASSET)))
        .ok_or_else(|| Error::Failed(format!("壓縮檔缺少 {EXE_IN_ASSET}。")))?;
    let mut entry = zip.by_index(index).map_err(|e| Error::Failed(format!("壓縮檔無法讀取：{e}")))?;
    let fresh = staging.join("new.exe");
    let mut file = std::fs::File::create(&fresh)?;
    std::io::copy(&mut entry, &mut file).map_err(|e| Error::Failed(format!("解壓縮寫入失敗（磁碟空間或權限）：{e}")))?;
    file.sync_all()?;
    drop(file);
    cancel.check()?;
    report("替換", size);
    swap(exe, &fresh, rename_retry)
}

/// The package's hash from SHA256.txt; Windows PowerShell 5 writes UTF-8 with a BOM.
fn hash_for(sums: &str) -> Option<String> {
    sums.trim_start_matches('\u{feff}').lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        (parts.next()? == ASSET).then(|| hash.to_lowercase())
    })
}

/// Moves the original into staging first and reports a failed rollback with its recovery path.
fn swap(exe: &Path, fresh: &Path, rename: impl Fn(&Path, &Path) -> std::io::Result<()>) -> Res<()> {
    let old = fresh.parent().unwrap().join("old.exe");
    rename(exe, &old).map_err(|e| Error::Failed(format!("無法替換執行檔：{e}")))?;
    if let Err(e) = rename(fresh, exe) {
        if let Err(rollback) = rename(&old, exe) {
            return fail(format!("無法放入新版本：{e}；回復失敗：{rollback}；原本的程式保留在 {}", old.display()));
        }
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
    fn sums_are_read_with_or_without_a_bom() {
        let line = "04E72BCA  BiliCacheExporter-win-x64.zip\r\n";
        assert_eq!(hash_for(line).as_deref(), Some("04e72bca"));
        assert_eq!(hash_for(&format!("\u{feff}{line}")).as_deref(), Some("04e72bca"));
        assert_eq!(hash_for("04E72BCA  other.zip\n"), None);
    }

    #[test]
    fn failed_swap_restores_the_original_executable() {
        let dir = std::env::temp_dir().join(format!("bili-swap-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("BiliCacheExporter.exe");
        std::fs::write(&exe, b"old").unwrap();
        assert!(with_staging(&dir, &Logger::memory(), |staging| swap(&exe, &staging.join("missing.exe"), rename_retry)).is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
        assert!(!std::fs::read_dir(&dir).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().starts_with(".update-")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failed_rollback_preserves_the_original_and_empty_marker_after_restart() {
        let dir = std::env::temp_dir().join(format!("bili-rollback-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&dir).unwrap();
        let exe = dir.join("BiliCacheExporter.exe");
        std::fs::write(&exe, b"original").unwrap();
        let mut staging_path = None;
        let log = Logger::memory();
        let error =
            with_staging(&dir, &log, |staging| {
                staging_path = Some(staging.to_path_buf());
                std::fs::write(staging.join("new.exe"), b"new").unwrap();
                swap(&exe, &staging.join("new.exe"), |from, to| {
                    if from == exe {
                        std::fs::rename(from, to)
                    } else {
                        Err(std::io::Error::other("injected rename failure"))
                    }
                })
            })
            .unwrap_err();
        let staging = staging_path.unwrap();
        let old = staging.join("old.exe");
        assert!(error.to_string().contains(&old.display().to_string()), "{error}");
        assert_eq!(std::fs::read(&old).unwrap(), b"original");
        assert_eq!(std::fs::read(staging.join(MARKER)).unwrap(), b"");
        assert!(!exe.exists());
        assert!(log.lines().iter().any(|l| l.contains("old.exe") && l.contains("injected rename failure")), "{:?}", log.lines());
        clean_old(&dir);
        assert_eq!(std::fs::read(old).unwrap(), b"original");
        assert_eq!(std::fs::read(staging.join(MARKER)).unwrap(), b"");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn first_rename_failure_removes_staging_and_keeps_the_original() {
        let dir = std::env::temp_dir().join(format!("bili-first-rename-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&dir).unwrap();
        let exe = dir.join("BiliCacheExporter.exe");
        std::fs::write(&exe, b"original").unwrap();
        let result = with_staging(&dir, &Logger::memory(), |staging| swap(&exe, &staging.join("new.exe"), |_, _| Err(std::io::Error::other("injected"))));
        assert!(result.is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn active_install_is_locked_and_publishes_only_after_success() {
        let dir = std::env::temp_dir().join(format!("bili-active-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&dir).unwrap();
        with_staging(&dir, &Logger::memory(), |staging| {
            std::fs::write(staging.join("old.exe"), b"original").unwrap();
            assert!(!remove_old(&dir));
            assert_eq!(std::fs::read(staging.join("old.exe")).unwrap(), b"original");
            assert!(std::fs::read(staging.join(MARKER)).is_err());
            Ok(())
        })
        .unwrap();
        assert!(remove_old(&dir));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir(dir).unwrap();
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
