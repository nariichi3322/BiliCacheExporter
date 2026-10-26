//! Update check against a fake GitHub API (network replaced by canned responses) and the related settings.
mod common;

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
