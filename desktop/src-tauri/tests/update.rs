//! Update check and install against a fake GitHub release (network replaced by canned responses),
//! plus the related settings.
mod common;

use bili_core::app::Event;
use bili_core::tools::Fetch;
use bili_core::{Cancel, Error};
use common::*;
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
fn update_replaces_only_the_executable() {
    let dir = installed();
    let h = harness_in(dir.clone(), Phone::one_device(), release(package(b"new exe"), package(b"new exe")));
    let settings = std::fs::read(dir.join("settings.json")).unwrap();
    h.app.install_update(&dir.join("BiliCacheExporter.exe"), "99.0.0").unwrap();
    assert_eq!(std::fs::read(dir.join("BiliCacheExporter.exe")).unwrap(), b"new exe");
    assert_eq!(std::fs::read(dir.join("BiliCacheExporter.exe.old")).unwrap(), b"old exe");
    assert_eq!(std::fs::read(dir.join("settings.json")).unwrap(), settings);
    assert_eq!(std::fs::read(dir.join("README.md")).unwrap(), b"old readme");
    assert_eq!(std::fs::read(dir.join("tools").join("adb").join("adb.exe")).unwrap(), b"adb");
    assert_eq!(entries(&dir), ["BiliCacheExporter.exe", "BiliCacheExporter.exe.old", "README.md", "settings.json", "tools"], "no staging left");
    let stages: Vec<String> = h
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            Event::Download { progress } if progress.name == "update" => Some(progress.stage.clone()),
            _ => None,
        })
        .collect();
    assert!(stages.contains(&"下載".to_string()) && stages.last().unwrap() == "替換", "{stages:?}");
    assert!(!h.app.is_busy());
    drop(h);
    harness_in(dir.clone(), Phone::one_device(), no_network());
    assert!(!dir.join("BiliCacheExporter.exe.old").exists(), "the next start removes the old executable");
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
    let h = harness_in(dir.clone(), Phone::one_device(), release(package(b"new exe"), package(b"new exe")));
    let e = h.app.install_update(&dir.join("BiliCacheExporter.exe"), "98.0.0").unwrap_err();
    assert!(e.message.contains("99.0.0"), "{}", e.message);
    assert_eq!(std::fs::read(dir.join("BiliCacheExporter.exe")).unwrap(), b"old exe");
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
