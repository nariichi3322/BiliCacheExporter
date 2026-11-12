//! Update check and install against a fake GitHub release (network replaced by canned responses),
//! plus the related settings.
mod common;

use bili_core::tools::Fetch;
use bili_core::{Cancel, Error};
use common::*;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

fn github(tag: &'static str, body: &'static str) -> Box<Fetch> {
    Box::new(move |url: &str, dest: &Path, _: u64, _: &Cancel, _: &mut dyn FnMut(u64)| {
        assert_eq!(url, "https://api.github.com/repos/nariichi3322/BiliCacheExporter/releases/latest");
        std::fs::write(dest, serde_json::json!({ "tag_name": tag, "body": body }).to_string()).unwrap();
        Ok(())
    })
}

#[test]
fn reports_a_newer_release_with_its_notes() {
    let h = harness_in(tempdir(), Phone::one_device(), github("v99.0.0", "- 新功能\r\n- 修正"));
    let r = h.app.check_update().unwrap();
    assert!(r.newer);
    assert_eq!(r.version, "99.0.0");
    assert_eq!(r.notes, "- 新功能\n- 修正");
}

#[test]
fn same_or_older_release_is_not_newer() {
    let current = bili_core::VERSION.split('+').next().unwrap();
    let tag: &'static str = Box::leak(format!("v{current}").into_boxed_str());
    assert!(!harness_in(tempdir(), Phone::one_device(), github(tag, "")).app.check_update().unwrap().newer, "build info after + is ignored");
    assert!(!harness_in(tempdir(), Phone::one_device(), github("v0.0.1", "")).app.check_update().unwrap().newer);
}

#[test]
fn network_and_format_errors_are_reported() {
    let offline: Box<Fetch> = Box::new(|_: &str, _: &Path, _: u64, _: &Cancel, _: &mut dyn FnMut(u64)| Err(Error::Failed("offline".into())));
    let e = harness_in(tempdir(), Phone::one_device(), offline).app.check_update().unwrap_err();
    assert!(e.message.contains("檢查更新失敗") && e.message.contains("offline"), "{}", e.message);
    let e = harness_in(tempdir(), Phone::one_device(), github("nightly", "")).app.check_update().unwrap_err();
    assert!(e.message.contains("無法辨識版本號"), "{}", e.message);
}

#[test]
fn update_check_setting_defaults_on_and_survives_old_files() {
    let dir = tempdir();
    std::fs::write(dir.join("settings.json"), r#"{"Theme":"light"}"#).unwrap();
    let h = harness_in(dir, Phone::one_device(), no_network());
    assert!(h.app.snapshot().settings.check_updates);
}

#[test]
fn accent_defaults_to_blue_and_unknown_values_fall_back() {
    let dir = tempdir();
    std::fs::write(dir.join("settings.json"), r#"{"Accent":"rainbow"}"#).unwrap();
    let h = harness_in(dir.clone(), Phone::one_device(), no_network());
    assert_eq!(h.app.snapshot().settings.accent, "blue");
    let s = h.app.snapshot().settings;
    let input = |accent: &str| bili_core::app::SettingsInput {
        adb: s.adb.clone(),
        ffmpeg: s.ffmpeg.clone(),
        ffprobe: s.ffprobe.clone(),
        output: s.output.clone(),
        temp: s.temp.clone(),
        theme: s.theme.clone(),
        accent: accent.into(),
        check_updates: true,
        full_decode: false,
    };
    assert_eq!(h.app.save_settings(input("pink")).unwrap().accent, "pink");
    assert!(std::fs::read_to_string(dir.join("settings.json")).unwrap().contains(r#""Accent": "pink""#));
    assert_eq!(h.app.save_settings(input("#123456")).unwrap().accent, "blue");
}

#[test]
fn skipped_version_is_saved_and_defaults_empty() {
    let dir = tempdir();
    let h = harness_in(dir.clone(), Phone::one_device(), no_network());
    assert_eq!(h.app.snapshot().settings.skip_version, "");
    assert_eq!(h.app.skip_version("2.1.0".into()).unwrap().skip_version, "2.1.0");
    drop(h);
    assert_eq!(harness_in(dir, Phone::one_device(), no_network()).app.snapshot().settings.skip_version, "2.1.0");
}

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        for (name, data) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }
    cursor.into_inner()
}

/// Fake GitHub release 99.0.0 serving `package` and a SHA256.txt for `hashed`.
fn release(package: Vec<u8>, hashed: Vec<u8>) -> Box<Fetch> {
    use sha2::{Digest, Sha256};
    let sums = format!("{:X}  BiliCacheExporter-win-x64.zip\n", Sha256::digest(&hashed));
    Box::new(move |url: &str, dest: &Path, max: u64, _: &Cancel, progress: &mut dyn FnMut(u64)| {
        let base = "https://github.com/nariichi3322/BiliCacheExporter/releases/download/v99.0.0/";
        let body = match url {
            u if u.ends_with("/releases/latest") => {
                serde_json::json!({ "tag_name": "v99.0.0", "body": "", "assets": [{ "name": "BiliCacheExporter-win-x64.zip", "size": package.len() }] })
                    .to_string()
                    .into_bytes()
            }
            u if u == format!("{base}SHA256.txt") => sums.clone().into_bytes(),
            u if u == format!("{base}BiliCacheExporter-win-x64.zip") => package.clone(),
            u if u == format!("{base}BiliCacheExporter-win-x64.zip.sig") => include_bytes!("fixtures/update/forward.zip.sig").to_vec(),
            other => panic!("unexpected url {other}"),
        };
        assert!(body.len() as u64 <= max);
        std::fs::write(dest, &body).unwrap();
        progress(body.len() as u64);
        Ok(())
    })
}

fn package(exe: &[u8]) -> Vec<u8> {
    zip_bytes(&[("BiliCacheExporter-win-x64/BiliCacheExporter.exe", exe), ("BiliCacheExporter-win-x64/README.md", b"new readme")])
}

/// A program folder with the running exe, settings and a downloaded tool.
fn installed() -> std::path::PathBuf {
    let dir = tempdir();
    std::fs::write(dir.join("BiliCacheExporter.exe"), b"old exe").unwrap();
    std::fs::write(dir.join("settings.json"), r#"{"Theme":"light","Accent":"pink"}"#).unwrap();
    std::fs::write(dir.join("README.md"), b"old readme").unwrap();
    std::fs::create_dir_all(dir.join("tools").join("adb")).unwrap();
    std::fs::write(dir.join("tools").join("adb").join("adb.exe"), b"adb").unwrap();
    dir
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[test]
fn hash_mismatch_changes_nothing() {
    let dir = installed();
    let before = entries(&dir);
    let h = harness_in(dir.clone(), Phone::one_device(), release(package(b"tampered"), package(b"new exe")));
    let e = h.app.install_update(&dir.join("BiliCacheExporter.exe"), "99.0.0").unwrap_err();
    assert!(e.message.contains("SHA-256"), "{}", e.message);
    assert_eq!(std::fs::read(dir.join("BiliCacheExporter.exe")).unwrap(), b"old exe");
    assert_eq!(entries(&dir), before);
}

#[test]
fn a_different_latest_version_is_not_installed() {
    let dir = installed();
    let before = snapshot(&dir);
    let h = harness_in(dir.clone(), Phone::one_device(), release(package(b"new exe"), package(b"new exe")));
    let e = h.app.install_update(&dir.join("BiliCacheExporter.exe"), "98.0.0").unwrap_err();
    assert!(e.message.contains("99.0.0"), "{}", e.message);
    assert_eq!(std::fs::read(dir.join("BiliCacheExporter.exe")).unwrap(), b"old exe");
    assert_eq!(snapshot(&dir), before);
}

#[test]
fn update_is_refused_while_busy() {
    let dir = installed();
    let (started_tx, started) = std::sync::mpsc::channel::<()>();
    let (release_tx, released) = std::sync::mpsc::channel::<()>();
    let (started_tx, released) = (std::sync::Mutex::new(started_tx), std::sync::Mutex::new(released));
    let blocking: Box<Fetch> = Box::new(move |_: &str, _: &Path, _: u64, _: &Cancel, _: &mut dyn FnMut(u64)| {
        started_tx.lock().unwrap().send(()).unwrap();
        released.lock().unwrap().recv().unwrap();
        Err(Error::Failed("offline".into()))
    });
    let h = harness_in(dir.clone(), Phone::one_device(), blocking);
    std::thread::scope(|scope| {
        let download = scope.spawn(|| h.app.download_tools(vec!["adb".into()]));
        started.recv().unwrap();
        let e = h.app.install_update(&dir.join("BiliCacheExporter.exe"), "99.0.0").unwrap_err();
        assert_eq!(e.message, "匯出完成後才能更新。");
        release_tx.send(()).unwrap();
        assert!(download.join().unwrap().is_err());
    });
    assert_eq!(std::fs::read(dir.join("BiliCacheExporter.exe")).unwrap(), b"old exe");
}

fn snapshot(dir: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    for name in entries(dir) {
        let path = dir.join(name);
        if path.is_dir() {
            files.push((path.clone(), Vec::new()));
            files.extend(snapshot(&path));
        } else {
            files.push((path.clone(), std::fs::read(path).unwrap()));
        }
    }
    files
}

fn owned(dir: &Path) -> std::path::PathBuf {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let path = dir.join(format!(".update-{id}"));
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join(".bili-owned"), id).unwrap();
    std::fs::write(path.join("old.exe"), b"original").unwrap();
    path
}

fn junction(link: &Path, target: &Path) {
    let result = std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(link).arg(target).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stdout));
}

#[test]
fn startup_preserves_unowned_or_unsafe_items() {
    let dir = installed();
    std::fs::write(dir.join("BiliCacheExporter.exe.old"), b"manual backup").unwrap();
    std::fs::write(dir.join("another.exe.old"), b"another backup").unwrap();
    let missing = owned(&dir);
    std::fs::remove_file(missing.join(".bili-owned")).unwrap();
    let mismatching = owned(&dir);
    std::fs::write(mismatching.join(".bili-owned"), b"wrong").unwrap();
    let empty = owned(&dir);
    std::fs::write(empty.join(".bili-owned"), b"").unwrap();
    let oversized = owned(&dir);
    let id = oversized.file_name().unwrap().to_str().unwrap().strip_prefix(".update-").unwrap();
    std::fs::write(oversized.join(".bili-owned"), format!("{id}extra")).unwrap();
    for name in
        [".update-crashed", ".update-ABCDEF0123456789abcdef0123456789", ".update-0123456789abcdef0123456789abcdef0", ".UPDATE-0123456789abcdef0123456789abcdef"]
    {
        let path = dir.join(name);
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("keep"), b"untouched").unwrap();
        std::fs::write(path.join(".bili-owned"), name.trim_start_matches(".update-")).unwrap();
    }
    let nested = owned(&dir);
    std::fs::create_dir(nested.join("child")).unwrap();
    std::fs::write(nested.join("child/keep"), b"nested").unwrap();
    let marker_directory = owned(&dir);
    std::fs::remove_file(marker_directory.join(".bili-owned")).unwrap();
    std::fs::create_dir(marker_directory.join(".bili-owned")).unwrap();
    let target = tempdir();
    let target_owned = owned(&target);
    let link = dir.join(target_owned.file_name().unwrap());
    junction(&link, &target_owned);
    let nested_link = owned(&dir);
    junction(&nested_link.join("link"), &target_owned);
    let before = snapshot(&dir);
    let target_before = snapshot(&target);
    let h = harness_in(dir.clone(), Phone::one_device(), no_network());
    assert_eq!(snapshot(&dir), before);
    assert_eq!(snapshot(&target), target_before);
    drop(h);
    std::fs::remove_dir(link).unwrap();
    std::fs::remove_dir(nested_link.join("link")).unwrap();
}

#[test]
fn failed_download_and_cancellation_change_nothing() {
    for cancelled in [false, true] {
        let dir = installed();
        let before = snapshot(&dir);
        let fetch = release(package(b"new exe"), package(b"new exe"));
        let failing: Box<Fetch> = Box::new(move |url, dest, max, cancel, progress| {
            if url.ends_with("/releases/latest") {
                return fetch(url, dest, max, cancel, progress);
            }
            std::fs::write(dest, b"partial download").unwrap();
            if cancelled {
                cancel.cancel();
                cancel.check()
            } else {
                Err(Error::Failed("offline".into()))
            }
        });
        let h = harness_in(dir.clone(), Phone::one_device(), failing);
        let e = h.app.install_update(&dir.join("BiliCacheExporter.exe"), "99.0.0").unwrap_err();
        assert!(if cancelled { e.cancelled } else { e.message.contains("offline") });
        assert_eq!(snapshot(&dir), before);
    }
}

#[test]
fn invalid_packages_change_nothing() {
    let incomplete = zip_bytes(&[("BiliCacheExporter-win-x64/README.md", b"missing executable")]);
    for (downloaded, hashed) in [(package(b"tampered"), package(b"expected")), (b"not a zip".to_vec(), b"not a zip".to_vec()), (incomplete.clone(), incomplete)]
    {
        let dir = installed();
        let before = snapshot(&dir);
        let h = harness_in(dir.clone(), Phone::one_device(), release(downloaded, hashed));
        assert!(h.app.install_update(&dir.join("BiliCacheExporter.exe"), "99.0.0").is_err());
        assert_eq!(snapshot(&dir), before);
    }
}

fn wait_removed(path: &Path) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while path.exists() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(!path.exists(), "cleanup did not finish: {}", path.display());
}

#[test]
fn startup_retries_an_exclusively_locked_marker() {
    let dir = installed();
    let staging = owned(&dir);
    let lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(staging.join(".bili-owned")).unwrap();
    let h = harness_in(dir.clone(), Phone::one_device(), no_network());
    assert_eq!(std::fs::read(staging.join("old.exe")).unwrap(), b"original");
    assert!(staging.join(".bili-owned").exists());
    assert!(std::fs::read(staging.join(".bili-owned")).is_err());
    drop(lock);
    wait_removed(&staging);
    drop(h);
}

#[test]
fn startup_retries_a_file_without_delete_sharing() {
    let dir = installed();
    let staging = owned(&dir);
    let lock = std::fs::OpenOptions::new().read(true).share_mode(1).open(staging.join("old.exe")).unwrap();
    let h = harness_in(dir.clone(), Phone::one_device(), no_network());
    assert_eq!(std::fs::read(staging.join("old.exe")).unwrap(), b"original");
    assert!(staging.join(".bili-owned").is_file());
    drop(lock);
    wait_removed(&staging);
    drop(h);
}

#[test]
fn startup_removes_only_empty_markerless_staging_with_an_exact_name() {
    let dir = installed();
    let path = dir.join(format!(".update-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir(&path).unwrap();
    let h = harness_in(dir, Phone::one_device(), no_network());
    assert!(!path.exists());
    drop(h);
}

#[test]
fn missing_wrong_empty_or_oversized_signature_changes_nothing() {
    for signature in [None, Some(include_bytes!("fixtures/update/forward.zip.sig").to_vec()), Some(Vec::new()), Some(vec![b'A'; 8193])] {
        let dir = installed();
        let before = snapshot(&dir);
        let bytes = include_bytes!("fixtures/update/forward.zip").to_vec();
        let fetch = release(bytes.clone(), bytes);
        let signed: Box<Fetch> = Box::new(move |url, dest, max, cancel, progress| {
            if url.ends_with(".sig") {
                assert_eq!(max, 8192);
                match &signature {
                    Some(bytes) => {
                        std::fs::write(dest, bytes).unwrap();
                        Ok(())
                    }
                    None => Err(Error::Failed("缺少簽章".into())),
                }
            } else {
                fetch(url, dest, max, cancel, progress)
            }
        });
        let h = harness_in(dir.clone(), Phone::one_device(), signed);
        let e = h.app.install_update(&dir.join("BiliCacheExporter.exe"), "99.0.0").unwrap_err();
        assert!(e.message.contains("簽章"), "{}", e.message);
        assert_eq!(std::fs::read(dir.join("BiliCacheExporter.exe")).unwrap(), b"old exe");
        assert_eq!(snapshot(&dir), before);
    }
}

#[test]
fn install_refuses_a_release_that_is_not_newer() {
    for tag in [bili_core::VERSION.split('+').next().unwrap(), "0.0.1"] {
        let dir = installed();
        let before = snapshot(&dir);
        let tag = Box::leak(format!("v{tag}").into_boxed_str());
        let h = harness_in(dir.clone(), Phone::one_device(), github(tag, ""));
        let e = h.app.install_update(&dir.join("BiliCacheExporter.exe"), tag.trim_start_matches('v')).unwrap_err();
        assert!(e.message.contains("不比目前版本新"), "{}", e.message);
        assert_eq!(snapshot(&dir), before);
    }
}
