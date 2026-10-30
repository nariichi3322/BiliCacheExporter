//! Tool download/installation: hash, archive safety, missing files, executability, cancel, retry,
//! keeping a working tool on failure, and portable relocation. Network is replaced by local files.
mod common;

use bili_core::log::Logger;
use bili_core::runner::Runner;
use bili_core::tools::{install, Fetch, Package, PACKAGES};
use bili_core::{Cancel, Error};
use common::*;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
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

fn package(bytes: &[u8]) -> Package {
    Package {
        name: "adb",
        title: "測試 ADB",
        version: "1",
        publisher: "test",
        url: "https://example.invalid/adb.zip",
        sha256: Box::leak(format!("{:x}", Sha256::digest(bytes)).into_boxed_str()),
        size: bytes.len() as u64,
        license: "test",
        files: &[("platform-tools/adb.exe", "adb.exe"), ("platform-tools/AdbWinApi.dll", "AdbWinApi.dll")],
    }
}

fn good_zip() -> Vec<u8> {
    zip_bytes(&[("platform-tools/adb.exe", b"new adb"), ("platform-tools/AdbWinApi.dll", b"dll")])
}

/// Serves `bytes`; `cut` stops half way with an error, `cancel` cancels mid download.
fn serve(bytes: Vec<u8>, cut: bool, cancel_mid: bool) -> Box<Fetch> {
    Box::new(move |_: &str, dest: &Path, _: u64, cancel: &Cancel, progress: &mut dyn FnMut(u64)| {
        let half = bytes.len() / 2;
        std::fs::write(dest, &bytes[..half]).unwrap();
        progress(half as u64);
        if cut {
            return Err(Error::Failed("下載中斷：connection reset。請重試。".into()));
        }
        if cancel_mid {
            cancel.cancel();
            cancel.check()?;
        }
        std::fs::write(dest, &bytes).unwrap();
        progress(bytes.len() as u64);
        Ok(())
    })
}

/// The extracted adb.exe "runs" through the runner seam; a file containing `broken` fails to start.
fn tool_runner() -> Runner {
    Runner::with_fake(
        Logger::memory(),
        Arc::new(|exe: &str, _: &[String], _: &Cancel, _: &mut dyn FnMut(&str)| {
            exe.ends_with("adb.exe").then(|| {
                if std::fs::read(exe).unwrap_or_default() == b"broken" {
                    Err(Error::Failed("無法啟動 adb.exe：not a valid application".into()))
                } else {
                    Ok("Android Debug Bridge version 1.0.41\n".into())
                }
            })
        }),
    )
}

fn existing_tool(dir: &Path) {
    std::fs::create_dir_all(dir.join("tools").join("adb")).unwrap();
    std::fs::write(dir.join("tools").join("adb").join("adb.exe"), b"old working adb").unwrap();
}

fn old_tool_intact(dir: &Path) -> bool {
    std::fs::read(dir.join("tools").join("adb").join("adb.exe")).unwrap() == b"old working adb"
}

fn no_staging_left(dir: &Path) -> bool {
    std::fs::read_dir(dir.join("tools")).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with('.'))
}

#[test]
fn successful_install_replaces_tool_only_after_verification() {
    let dir = tempdir();
    existing_tool(&dir);
    let bytes = good_zip();
    let mut stages = Vec::new();
    let target =
        install(&dir, &tool_runner(), &package(&bytes), &*serve(bytes.clone(), false, false), &Cancel::default(), &mut |p| stages.push(p.stage)).unwrap();
    assert_eq!(target, dir.join("tools").join("adb"));
    assert_eq!(std::fs::read(target.join("adb.exe")).unwrap(), b"new adb");
    assert!(target.join("AdbWinApi.dll").exists() && !dir.join("AdbWinApi.dll").exists(), "support DLL stays in the tool folder");
    assert!(no_staging_left(&dir));
    for s in ["下載", "檢查完整性", "解壓縮", "驗證工具可執行", "完成"] {
        assert!(stages.iter().any(|x| x == s), "missing stage {s}");
    }
}

#[test]
fn every_failure_keeps_the_working_tool_and_cleans_staging() {
    let good = good_zip();
    let missing = zip_bytes(&[("platform-tools/adb.exe", b"new adb")]);
    let escape = zip_bytes(&[("../../evil.exe", b"x"), ("platform-tools/adb.exe", b"new adb"), ("platform-tools/AdbWinApi.dll", b"dll")]);
    let broken = zip_bytes(&[("platform-tools/adb.exe", b"broken"), ("platform-tools/AdbWinApi.dll", b"dll")]);
    let mut wrong_hash = package(&good);
    wrong_hash.sha256 = "0000000000000000000000000000000000000000000000000000000000000000";
    let cases: Vec<(&str, Package, Box<Fetch>, &str)> = vec![
        ("錯誤雜湊", wrong_hash, serve(good.clone(), false, false), "SHA-256"),
        ("中斷", package(&good), serve(good.clone(), true, false), "下載中斷"),
        ("取消", package(&good), serve(good.clone(), false, true), "已取消"),
        ("缺檔", package(&missing), serve(missing.clone(), false, false), "缺少必要檔案"),
        ("越界", package(&escape), serve(escape.clone(), false, false), "越界路徑"),
        ("不可執行", package(&broken), serve(broken.clone(), false, false), "ADB 無法執行"),
        ("大小不符", package(&good), serve(good[..good.len() - 1].to_vec(), false, false), "大小不符"),
    ];
    for (name, pkg, fetch, expected) in cases {
        let dir = tempdir();
        existing_tool(&dir);
        let e = install(&dir, &tool_runner(), &pkg, &*fetch, &Cancel::default(), &mut |_| {}).unwrap_err();
        assert!(e.to_string().contains(expected), "{name}: {e}");
        assert!(old_tool_intact(&dir), "{name}: working tool damaged");
        assert!(no_staging_left(&dir), "{name}: staging left behind");
        assert!(!dir.join("evil.exe").exists() && !dir.parent().unwrap().join("evil.exe").exists(), "{name}");
        // Retry after the problem is fixed succeeds.
        let ok = good_zip();
        install(&dir, &tool_runner(), &package(&ok), &*serve(ok.clone(), false, false), &Cancel::default(), &mut |_| {}).unwrap();
    }
}

#[test]
fn unwritable_tools_folder_is_reported() {
    let dir = tempdir();
    std::fs::write(dir.join("tools"), b"a file, not a folder").unwrap();
    let bytes = good_zip();
    let e = install(&dir, &tool_runner(), &package(&bytes), &*serve(bytes.clone(), false, false), &Cancel::default(), &mut |_| {}).unwrap_err();
    assert!(e.to_string().contains("無法寫入"), "{e}");
}

#[test]
fn app_download_failure_leaves_settings_and_offers_retry() {
    let dir = tempdir();
    write_settings(&dir);
    let before = std::fs::read_to_string(dir.join("settings.json")).unwrap();
    let h = harness_in(
        dir.clone(),
        Phone::one_device(),
        Box::new(|_: &str, _: &Path, _: u64, _: &Cancel, _: &mut dyn FnMut(u64)| Err(Error::Failed("下載失敗：offline".into()))),
    );
    let e = h.app.download_tools(vec!["adb".into()]).unwrap_err();
    assert!(e.message.contains("offline"));
    assert_eq!(std::fs::read_to_string(dir.join("settings.json")).unwrap(), before);
    assert!(!h.app.is_busy(), "dialog can retry or switch to manual paths");
    assert!(h.app.download_tools(vec!["unknown".into()]).is_err());
}

#[test]
fn portable_relative_tool_paths_survive_moving_the_folder() {
    let a = tempdir();
    write_settings(&a);
    std::fs::create_dir_all(a.join("tools").join("adb")).unwrap();
    std::fs::write(a.join("tools").join("adb").join("fake-adb.exe"), b"fake").unwrap();
    let h = harness_in(a.clone(), Phone::one_device(), no_network());
    let s = h.app.snapshot().settings;
    let absolute = a.join("tools").join("adb").join("fake-adb.exe").to_string_lossy().into_owned();
    h.app
        .save_settings(bili_core::app::SettingsInput {
            adb: absolute,
            ffmpeg: s.ffmpeg,
            ffprobe: s.ffprobe,
            output: s.output,
            temp: s.temp,
            theme: s.theme,
            accent: s.accent,
            check_updates: true,
            full_decode: false,
        })
        .unwrap();
    assert_eq!(h.app.snapshot().settings.adb, r"tools\adb\fake-adb.exe", "stored relative to the program folder");
    drop(h);
    let b = tempdir().join("moved");
    std::fs::create_dir_all(b.join("tools").join("adb")).unwrap();
    std::fs::copy(a.join("settings.json"), b.join("settings.json")).unwrap();
    std::fs::copy(a.join("tools").join("adb").join("fake-adb.exe"), b.join("tools").join("adb").join("fake-adb.exe")).unwrap();
    std::fs::remove_dir_all(&a).unwrap();
    let moved = harness_in(b.clone(), Phone::one_device(), no_network());
    let adb = &moved.app.check_tools().unwrap()[0];
    assert!(adb.ok && adb.path.starts_with(&*b.to_string_lossy()), "{adb:?}");
    assert_eq!(moved.app.connect(None).unwrap().items.len(), 0, "works offline from the new location");
}

#[test]
fn https_only() {
    let e = bili_core::tools::https_fetch("http://example.com/a.zip", &tempdir().join("a"), 10, &Cancel::default(), &mut |_| {}).unwrap_err();
    assert!(e.to_string().contains("HTTPS"));
    assert!(PACKAGES.iter().all(|p| p.url.starts_with("https://") && p.sha256.len() == 64 && p.size > 0));
}

/// Real sources (network). Run explicitly: cargo test --test tools_download -- --ignored
#[test]
#[ignore]
fn real_pinned_downloads_install_and_run() {
    let dir = tempdir();
    let runner = Runner::new(Logger::memory());
    for package in PACKAGES {
        let target = install(&dir, &runner, package, &bili_core::tools::https_fetch, &Cancel::default(), &mut |_| {}).unwrap();
        for (kind, exe) in bili_core::tools::executables(package.name) {
            let version = bili_core::tools::check_tool(&runner, *kind, &target.join(exe)).unwrap();
            println!("{} -> {version}", package.name);
        }
    }
    let mut total = 0u64;
    for entry in walk(&dir.join("tools")) {
        total += std::fs::metadata(&entry).unwrap().len();
    }
    println!("landed tools bytes: {total}");
    std::fs::remove_dir_all(&dir).unwrap();
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().is_dir() {
            files.extend(walk(&e.path()))
        } else {
            files.push(e.path())
        }
    }
    files
}
