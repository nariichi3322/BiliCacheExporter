//! Signed updates use the local test trust root in this test process only.
mod common;

use bili_core::app::Event;
use bili_core::tools::Fetch;
use bili_core::Cancel;
use common::*;
use std::path::Path;

fn release(package: Vec<u8>, hashed: Vec<u8>) -> Box<Fetch> {
    use sha2::{Digest, Sha256};
    let sums = format!("{:X}  BiliCacheExporter-win-x64.zip\n", Sha256::digest(&hashed));
    Box::new(move |url: &str, dest: &Path, max: u64, _: &Cancel, progress: &mut dyn FnMut(u64)| {
        let base = "http://127.0.0.1:1/nariichi3322/BiliCacheExporter/releases/download/v99.0.0/";
        let body = match url {
            "http://127.0.0.1:1/repos/nariichi3322/BiliCacheExporter/releases/latest" => {
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

fn installed() -> std::path::PathBuf {
    static SOURCE: std::sync::Once = std::sync::Once::new();
    SOURCE.call_once(|| std::env::set_var(bili_core::update::SOURCE_ENV, "http://127.0.0.1:1"));
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

#[test]
fn update_replaces_only_the_executable() {
    let dir = installed();
    let h = harness_in(
        dir.clone(),
        Phone::one_device(),
        release(include_bytes!("fixtures/update/forward.zip").to_vec(), include_bytes!("fixtures/update/forward.zip").to_vec()),
    );
    let settings = std::fs::read(dir.join("settings.json")).unwrap();
    h.app.install_update(&dir.join("BiliCacheExporter.exe"), "99.0.0").unwrap();
    assert_eq!(std::fs::read(dir.join("BiliCacheExporter.exe")).unwrap(), b"new exe");
    let staging_name = entries(&dir).into_iter().find(|n| n.starts_with(".update-")).unwrap();
    let staging = dir.join(&staging_name);
    assert_eq!(std::fs::read(staging.join("old.exe")).unwrap(), b"old exe");
    assert_eq!(std::fs::read_to_string(staging.join(".bili-owned")).unwrap(), staging_name.strip_prefix(".update-").unwrap());
    assert!(!dir.join("BiliCacheExporter.exe.old").exists());
    assert_eq!(std::fs::read(dir.join("settings.json")).unwrap(), settings);
    assert_eq!(std::fs::read(dir.join("README.md")).unwrap(), b"old readme");
    assert_eq!(std::fs::read(dir.join("tools").join("adb").join("adb.exe")).unwrap(), b"adb");
    assert_eq!(entries(&dir), [&staging_name, "BiliCacheExporter.exe", "README.md", "settings.json", "tools"]);
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
    std::fs::create_dir_all(dir.join(".update-crashed")).unwrap();
    std::fs::write(dir.join(".update-crashed").join("BiliCacheExporter-win-x64.zip"), b"partial").unwrap();
    harness_in(dir.clone(), Phone::one_device(), no_network());
    assert!(!staging.exists(), "the next start removes the owned staging folder including the old executable");
    assert_eq!(std::fs::read(dir.join(".update-crashed").join("BiliCacheExporter-win-x64.zip")).unwrap(), b"partial", "unowned staging is preserved");
}

#[test]
fn update_preserves_a_manual_executable_backup() {
    let dir = installed();
    let backup = dir.join("BiliCacheExporter.exe.old");
    std::fs::write(&backup, b"manual backup").unwrap();
    let h = harness_in(
        dir.clone(),
        Phone::one_device(),
        release(include_bytes!("fixtures/update/forward.zip").to_vec(), include_bytes!("fixtures/update/forward.zip").to_vec()),
    );
    h.app.install_update(&dir.join("BiliCacheExporter.exe"), "99.0.0").unwrap();
    assert_eq!(std::fs::read(backup).unwrap(), b"manual backup");
    assert_eq!(std::fs::read(dir.join("BiliCacheExporter.exe")).unwrap(), b"new exe");
}

#[test]
fn packages_zipped_with_backslash_separators_also_install() {
    let dir = installed();
    let zipped = include_bytes!("fixtures/update/backslash.zip").to_vec();
    let fetch = release(zipped.clone(), zipped);
    let signed: Box<Fetch> = Box::new(move |url, dest, max, cancel, progress| {
        if url.ends_with(".sig") {
            std::fs::write(dest, include_bytes!("fixtures/update/backslash.zip.sig")).unwrap();
            Ok(())
        } else {
            fetch(url, dest, max, cancel, progress)
        }
    });
    let h = harness_in(dir.clone(), Phone::one_device(), signed);
    h.app.install_update(&dir.join("BiliCacheExporter.exe"), "99.0.0").unwrap();
    assert_eq!(std::fs::read(dir.join("BiliCacheExporter.exe")).unwrap(), b"new exe");
}

#[test]
fn tampered_zip_with_matching_hash_is_refused_by_signature() {
    let dir = installed();
    let before = snapshot(&dir);
    let mut bytes = include_bytes!("fixtures/update/forward.zip").to_vec();
    bytes[0] ^= 1;
    let h = harness_in(dir.clone(), Phone::one_device(), release(bytes.clone(), bytes));
    let e = h.app.install_update(&dir.join("BiliCacheExporter.exe"), "99.0.0").unwrap_err();
    assert!(e.message.contains("簽章"), "{}", e.message);
    assert_eq!(std::fs::read(dir.join("BiliCacheExporter.exe")).unwrap(), b"old exe");
    assert_eq!(snapshot(&dir), before);
}
