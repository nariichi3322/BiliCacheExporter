//! Application-level flows: settings, connect/scan, quality choice, export, cover, batch and recovery.
//! ADB is a fake phone; FFmpeg/FFprobe are real (BILI_TEST_FFMPEG).
mod common;

use bili_core::app::{App, Event, Selection, SettingsInput};
use bili_core::batch::{Outcome, Summary};
use bili_core::export::{export, Request};
use bili_core::log::Logger;
use bili_core::scan::{Adb, ROOT};
use bili_core::storage::Storage;
use bili_core::Cancel;
use common::*;
use std::path::Path;

fn sel(id: usize) -> Selection {
    Selection { id, variant: None }
}

fn input(h: &Harness) -> SettingsInput {
    let s = h.app.snapshot().settings;
    SettingsInput { adb: s.adb, ffmpeg: s.ffmpeg, ffprobe: s.ffprobe, output: s.output, temp: s.temp, theme: s.theme }
}

fn results(h: &Harness) -> Vec<(usize, Outcome, String)> {
    h.events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| if let Event::Result { result } = e { Some((result.id, result.outcome, result.message.clone())) } else { None })
        .collect()
}

// ---------- 01: settings, tools, logs, no startup query ----------

#[test]
fn v1_settings_upgrade_keeps_values_and_original() {
    let dir = tempdir();
    let v1 = r#"{"Adb":"D:\\old\\adb.exe","Ffmpeg":"D:\\ff\\bin\\ffmpeg.exe","Output":"E:\\Videos","Temp":"F:\\tmp","Custom":42}"#;
    std::fs::write(dir.join("settings.json"), v1).unwrap();
    let h = harness_in(dir.clone(), Phone::one_device(), no_network());
    let s = h.app.snapshot().settings;
    assert_eq!(
        (s.adb.as_str(), s.ffmpeg.as_str(), s.output.as_str(), s.temp.as_str(), s.theme.as_str()),
        (r"D:\old\adb.exe", r"D:\ff\bin\ffmpeg.exe", r"E:\Videos", r"F:\tmp", "dark")
    );
    assert_eq!(s.ffprobe, r"D:\ff\bin\ffprobe.exe", "v1 implied ffprobe beside ffmpeg");
    let mut i = input(&h);
    i.theme = "light".into();
    h.app.save_settings(i).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("settings.v1-backup.json")).unwrap(), v1, "original kept byte for byte");
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(saved["Custom"], 42, "unknown values are not dropped");
    assert_eq!(saved["Output"], r"E:\Videos");
    // Restart: theme persists.
    let again = harness_in(dir, Phone::one_device(), no_network());
    assert_eq!(again.app.snapshot().settings.theme, "light");
}

#[test]
fn corrupt_settings_are_reported_not_replaced() {
    let dir = tempdir();
    std::fs::write(dir.join("settings.json"), "{ broken").unwrap();
    let h = harness_in(dir.clone(), Phone::one_device(), no_network());
    let snap = h.app.snapshot();
    assert!(snap.settings_error.unwrap().contains("設定檔損壞"));
    let i =
        SettingsInput { adb: String::new(), ffmpeg: String::new(), ffprobe: String::new(), output: "C:\\x".into(), temp: String::new(), theme: "dark".into() };
    assert!(h.app.save_settings(i).is_err());
    assert_eq!(std::fs::read_to_string(dir.join("settings.json")).unwrap(), "{ broken", "corrupt file untouched");
    h.app.reset_settings().unwrap();
    let aside: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("settings.corrupt-")).collect();
    assert_eq!(std::fs::read_to_string(aside[0].path()).unwrap(), "{ broken", "reset moves the corrupt file aside");
    assert!(h.app.snapshot().settings_error.is_none());
}

#[test]
fn unwritable_folder_blocks_with_actionable_message() {
    let dir = tempdir();
    write_settings(&dir);
    let phone = std::sync::Arc::new(std::sync::Mutex::new(Phone::one_device()));
    let app = App::new(dir.clone(), runner(&phone, Logger::memory()), no_network(), std::sync::Arc::new(|_| {}), Some("程式資料夾無法寫入".into()));
    let s = app.snapshot().settings;
    let err = app
        .save_settings(SettingsInput { adb: s.adb, ffmpeg: s.ffmpeg, ffprobe: s.ffprobe, output: s.output, temp: s.temp, theme: "light".into() })
        .unwrap_err();
    assert!(err.message.contains("可寫的位置"));
    assert!(app.connect(None).unwrap_err().message.contains("可寫的位置"));
    // A real non-writable target: the "folder" is a file.
    std::fs::write(dir.join("blocked"), b"x").unwrap();
    assert!(bili_core::storage::ensure_writable(&dir.join("blocked")).unwrap_err().to_string().contains("無法寫入"));
}

#[test]
fn tools_are_validated_by_running_them_not_by_path() {
    let h = harness(Phone::one_device());
    let tools = h.app.check_tools().unwrap();
    assert!(tools.iter().all(|t| t.ok), "{tools:?}");
    assert!(tools[1].message.starts_with("ffmpeg version"));
    let mut i = input(&h);
    i.ffmpeg = r"C:\Windows\System32\where.exe".into();
    i.ffprobe = r"Z:\missing\ffprobe.exe".into();
    i.adb = String::new();
    h.app.save_settings(i).unwrap();
    let tools = h.app.check_tools().unwrap();
    assert!(tools[0].message.contains("尚未設定 ADB"));
    assert!(!tools[1].ok && tools[1].message.contains("FFmpeg"), "{}", tools[1].message);
    assert!(tools[2].message.contains("找不到 FFprobe"));
    let missing = h.app.connect(None).unwrap_err();
    assert_eq!(missing.missing, vec!["adb"], "missing tool offers the download");
}

#[test]
fn startup_and_idle_never_query_devices() {
    let h = harness(Phone::one_device());
    std::thread::sleep(std::time::Duration::from_millis(400));
    h.app.snapshot();
    h.app.check_tools().unwrap();
    h.app.save_settings(input(&h)).unwrap();
    assert_eq!(h.phone.lock().unwrap().device_queries(), 0);
    h.app.connect(None).unwrap();
    assert_eq!(h.phone.lock().unwrap().device_queries(), 1, "one listing per user action");
}

#[test]
fn logs_go_to_log_folder_redacted_and_keep_old_logs() {
    let dir = tempdir();
    std::fs::write(dir.join("session-old.log"), "old root log").unwrap();
    std::fs::create_dir_all(dir.join("log")).unwrap();
    std::fs::write(dir.join("log").join("session-previous.log"), "previous").unwrap();
    let log = Logger::open(&dir).unwrap();
    let profile = std::env::var("USERPROFILE").unwrap();
    log.log(&format!(r#"> adb "-s" "{SERIAL}" "pull" "https://x.y/v.m4s?token=secret" "{profile}\Videos\a.mp4""#));
    log.log(&format!("{SERIAL}\tdevice model:X"));
    let text = std::fs::read_to_string(log.path.as_ref().unwrap()).unwrap();
    assert!(log.path.as_ref().unwrap().starts_with(dir.join("log")));
    assert!(!text.contains(SERIAL) && !text.contains("secret") && !text.to_lowercase().contains(&profile.to_lowercase()), "{text}");
    assert!(text.contains("[device]") && text.contains("[redacted]") && text.contains("[user]"));
    assert_eq!(std::fs::read_to_string(dir.join("session-old.log")).unwrap(), "old root log");
    assert!(dir.join("log").join("session-previous.log").exists());
}

// ---------- 02: manual connect and scan ----------

#[test]
fn device_problems_have_distinct_messages() {
    let cases = [
        ("List of devices attached\n", "找不到 Android 裝置"),
        (&*format!("List of devices attached\n{SERIAL}\tunauthorized usb:1\n"), "尚未授權"),
        (&*format!("List of devices attached\n{SERIAL}\toffline\n"), "離線"),
    ];
    for (devices, expected) in cases {
        let h = harness(Phone { devices: devices.into(), package: true, ..Default::default() });
        let e = h.app.connect(None).unwrap_err();
        assert!(e.message.contains(expected), "{} vs {expected}", e.message);
        assert!(h.app.snapshot().serial.is_none() && h.app.snapshot().items.is_empty());
    }
    let h = harness(Phone { devices_timeout: true, ..Phone::one_device() });
    assert!(h.app.connect(None).unwrap_err().message.contains("逾時"));
    h.phone.lock().unwrap().devices_timeout = false;
    h.phone.lock().unwrap().add("1/c_1", "影片", &["80"], true);
    assert_eq!(h.app.connect(None).unwrap().items.len(), 1, "after fixing, connect works again");
    // ADB that cannot start (not an executable).
    let mut i = input(&h);
    std::fs::write(h.dir.join("broken.exe"), b"not a program").unwrap();
    i.adb = h.dir.join("broken.exe").to_string_lossy().into();
    h.app.save_settings(i).unwrap();
    assert!(h.app.connect(None).unwrap_err().message.contains("無法啟動"));
}

#[test]
fn multiple_devices_need_an_explicit_choice() {
    let mut phone = Phone::one_device();
    phone.devices.push_str("OTHER-2\tdevice model:Pixel\n");
    phone.add("1/c_1", "影片", &["80"], false);
    let h = harness(phone);
    let r = h.app.connect(None).unwrap();
    assert!(r.serial.is_none() && r.devices.len() == 2 && r.items.is_empty());
    assert!(h.phone.lock().unwrap().calls.iter().all(|c| c[0] == "devices"), "nothing read from any phone yet");
    let r = h.app.connect(Some(SERIAL.into())).unwrap();
    assert_eq!(r.serial.as_deref(), Some(SERIAL));
    assert_eq!(r.items.len(), 1);
    assert!(h.phone.lock().unwrap().calls.iter().filter(|c| c[0] == "-s").all(|c| c[1] == SERIAL), "bound to chosen serial");
}

#[test]
fn scan_is_read_only_shows_real_metadata_and_isolates_bad_rows() {
    let mut phone = Phone::one_device();
    let good = phone.add("100/c_1", "中文標題：測試 🎬", &["80"], true);
    let bad = phone.add("200/c_2", "壞資料", &["64"], false);
    phone.files.insert(format!("{bad}/entry.json"), b"{ not json".to_vec());
    phone.extra_entries.push(format!("{ROOT}300/../../evil/entry.json"));
    let h = harness(phone);
    let r = h.app.connect(None).unwrap();
    assert_eq!(r.items.len(), 3);
    let item = r.items.iter().find(|i| i.remote == good).unwrap();
    assert_eq!(item.title, "中文標題：測試 🎬 — 第一集");
    assert!(item.cover.as_ref().unwrap().starts_with("data:image/jpeg;base64,"));
    assert_eq!(item.auto_variant, Some(0), "single complete quality is the source");
    assert!(item.quality.contains("80（快取目錄）") && item.quality.contains("width=160"));
    let broken = r.items.iter().find(|i| i.remote == bad).unwrap();
    assert!(broken.issue.contains("entry.json 無法解析"));
    let evil = r.items.iter().find(|i| i.title == "不合法的快取路徑").unwrap();
    assert!(evil.remote.is_empty() && !evil.issue.is_empty());
    let phone = h.phone.lock().unwrap();
    assert!(phone.pulls.iter().all(|p| p.ends_with("/cover.jpg")), "no media pulled during scan: {:?}", phone.pulls);
    assert!(phone
        .calls
        .iter()
        .filter(|c| c[0] == "-s" && c[2] == "shell")
        .all(|c| ["pm list", "find ", "stat -c", "cat "].iter().any(|p| c[3].starts_with(p))));
    assert!(h.log.lines().iter().all(|l| !l.contains(SERIAL)), "serial redacted in logs");
    assert!(h.events.lock().unwrap().iter().filter(|e| matches!(e, Event::Item { .. })).count() == 3, "rows stream as found");
}

#[test]
fn empty_cache_and_missing_app() {
    let h = harness(Phone::one_device());
    assert!(h.app.connect(None).unwrap().items.is_empty());
    let h = harness(Phone { package: false, ..Phone::one_device() });
    assert!(h.app.connect(None).unwrap_err().message.contains("國際版"));
    assert!(h.app.snapshot().serial.is_none(), "failed scan leaves no usable state");
}

#[test]
fn rescan_rechecks_device_and_switching_invalidates_list() {
    let mut phone = Phone::one_device();
    phone.add("1/c_1", "A", &["80"], false);
    phone.devices.push_str("OTHER-2\tdevice model:Pixel\n");
    let h = harness(phone);
    let first = h.app.connect(Some(SERIAL.into())).unwrap();
    let again = h.app.scan().unwrap();
    assert!(again.generation > first.generation);
    let switched = h.app.connect(Some("OTHER-2".into())).unwrap();
    assert_eq!(switched.serial.as_deref(), Some("OTHER-2"));
    assert!(switched.generation > again.generation, "switching device starts a new list");
    let e = h.app.export(first.generation, vec![sel(0)]).unwrap_err();
    assert!(e.message.contains("清單已更新"), "old phone's selection rejected");
    h.app.connect(Some(SERIAL.into())).unwrap();
    h.phone.lock().unwrap().devices = "List of devices attached\n".into();
    let e = h.app.scan().unwrap_err();
    assert!(e.message.contains("已拔除"));
    assert!(h.app.snapshot().items.is_empty());
}

#[test]
fn disconnect_during_scan_leaves_nothing_exportable() {
    let mut phone = Phone::one_device();
    phone.add("1/c_1", "A", &["80"], true);
    phone.add("2/c_2", "B", &["80"], true);
    phone.disconnect_after_pull = Some("1/c_1/cover.jpg".into());
    let h = harness(phone);
    let r = h.app.connect(None).unwrap();
    let b = r.items.iter().find(|i| i.remote.ends_with("2/c_2")).unwrap();
    assert!(b.issue.contains("掃描失敗"), "{}", b.issue);
    let e = h.app.export(r.generation, vec![sel(0)]).unwrap_err();
    assert!(e.message.contains("已離線或已拔除"), "{}", e.message);
    assert!(h.phone.lock().unwrap().pulls.iter().all(|p| p.ends_with("cover.jpg")));
}

#[test]
fn overlapping_operations_are_refused() {
    let mut phone = Phone::one_device();
    phone.add("1/c_1", "A", &["80"], false);
    let h = std::sync::Arc::new(harness(phone));
    let gen = h.app.connect(None).unwrap().generation;
    let worker = {
        let h = h.clone();
        std::thread::spawn(move || h.app.export(gen, vec![sel(0)]))
    };
    while !h.app.is_busy() {
        std::thread::yield_now();
    }
    assert!(h.app.scan().unwrap_err().message.contains("作業進行中"));
    assert!(h.app.export(gen, vec![sel(0)]).unwrap_err().message.contains("作業進行中"));
    worker.join().unwrap().unwrap();
}

// ---------- 03: qualities and non-exportable rows ----------

#[test]
fn multi_quality_requires_explicit_valid_choice() {
    let mut phone = Phone::one_device();
    let remote = phone.add("1/c_1", "多畫質", &["80", "116"], false);
    phone.files.insert(format!("{remote}/116/index.json"), b"{ corrupt".to_vec());
    let one = phone.add("2/c_2", "只有一個完整", &["80", "64"], false);
    phone.files.remove(&format!("{one}/64/audio.m4s"));
    let h = harness(phone);
    let r = h.app.connect(None).unwrap();
    let multi = r.items.iter().find(|i| i.remote == remote).unwrap();
    assert!(multi.needs_choice && multi.auto_variant.is_none() && multi.variants.len() == 2, "one row, two candidates");
    let q116 = multi.variants.iter().position(|v| v.quality.starts_with("116")).unwrap();
    assert!(multi.variants[q116].quality.contains("畫質資料無法讀取") && multi.variants[q116].issue.is_empty(), "bad optional metadata does not block media");
    let single = r.items.iter().find(|i| i.remote == one).unwrap();
    let broken = single.variants.iter().position(|v| v.quality.starts_with("64")).unwrap();
    assert_eq!(single.auto_variant, Some(1 - broken));
    assert!(single.variants[broken].issue.contains("缺少"));
    let e = h.app.export(r.generation, vec![sel(multi.id)]).unwrap_err();
    assert!(e.message.contains("選擇畫質"));
    assert!(h.app.export(r.generation, vec![Selection { id: single.id, variant: Some(broken) }]).unwrap_err().message.contains("無法匯出"));
    assert!(h.app.export(r.generation, vec![Selection { id: multi.id, variant: Some(9) }]).is_err());
    assert!(h.phone.lock().unwrap().pulls.is_empty(), "rejected before any transfer");
    let summary = h.app.export(r.generation, vec![Selection { id: multi.id, variant: Some(q116) }]).unwrap();
    assert_eq!(summary.partial, 1);
    assert_eq!(h.phone.lock().unwrap().pulls, vec![format!("{remote}/116/video.m4s"), format!("{remote}/116/audio.m4s")], "only the chosen quality is pulled");
}

#[test]
fn incomplete_missing_empty_and_unknown_items_stay_visible_with_reasons() {
    let mut phone = Phone::one_device();
    let incomplete = phone.add("1/c_1", "未完成", &["80"], false);
    phone.files.insert(format!("{incomplete}/entry.json"), r#"{"title":"未完成","is_completed":false}"#.as_bytes().to_vec());
    let missing = phone.add("2/c_2", "缺檔", &["80"], false);
    phone.files.remove(&format!("{missing}/80/video.m4s"));
    let empty = phone.add("3/c_3", "空檔", &["80"], false);
    phone.files.insert(format!("{empty}/80/audio.m4s"), Vec::new());
    let unknown = phone.add("4/c_4", "未知格式", &[], false);
    phone.files.insert(format!("{unknown}/lua.flv.bili2api.80/0.blv"), b"x".to_vec());
    let h = harness(phone);
    let r = h.app.connect(None).unwrap();
    let issue = |remote: &str| r.items.iter().find(|i| i.remote == remote).unwrap().issue.clone();
    assert!(issue(&incomplete).contains("下載尚未完成"));
    assert!(issue(&missing).contains("缺少 video.m4s 或 audio.m4s"));
    assert!(issue(&empty).contains("影音檔案為空"));
    assert!(issue(&unknown).contains("待辨識／不支援"));
    let u = r.items.iter().find(|i| i.remote == unknown).unwrap();
    assert!(u.inventory.iter().any(|f| f.path == "lua.flv.bili2api.80/0.blv" && f.size == Some(1)), "read-only inventory");
    for item in &r.items {
        assert!(h.app.export(r.generation, vec![sel(item.id)]).is_err());
    }
}

// ---------- 04/05: single export, cover, safety ----------

fn exported(h: &Harness) -> Vec<String> {
    mp4s(&h.dir.join("output")).into_iter().filter(|n| n.ends_with(".mp4") && !n.starts_with(".bili")).collect()
}

#[test]
fn export_embeds_original_cover_and_verifies() {
    let mut phone = Phone::one_device();
    let remote = phone.add("1/c_1", "中文:影片", &["80"], true);
    phone.add("2/c_2", "未選", &["80"], true);
    let h = harness(phone);
    let r = h.app.connect(None).unwrap();
    let id = r.items.iter().find(|i| i.remote == remote).unwrap().id;
    h.phone.lock().unwrap().pulls.clear();
    let summary = h.app.export(r.generation, vec![sel(id)]).unwrap();
    assert_eq!(summary, Summary { total: 1, successful: 1, ..Default::default() });
    assert_eq!(exported(&h), vec!["中文_影片 — 第一集.mp4"]);
    let target = h.dir.join("output").join("中文_影片 — 第一集.mp4");
    let streams = probe(&target)["streams"].as_array().unwrap().clone();
    assert!(streams.iter().any(|s| s["codec_name"] == "h264" && s["width"] == 160 && s["disposition"]["attached_pic"] == 0));
    assert!(streams.iter().any(|s| s["codec_name"] == "aac"));
    assert!(streams.iter().any(|s| s["codec_name"] == "mjpeg" && s["disposition"]["attached_pic"] == 1 && s["width"] == 320));
    let extracted = h.dir.join("extracted.jpg");
    real(&[
        "-hide_banner",
        "-nostdin",
        "-v",
        "error",
        "-i",
        &target.to_string_lossy(),
        "-map",
        "0:v:1",
        "-c",
        "copy",
        "-f",
        "image2",
        &extracted.to_string_lossy(),
    ]);
    assert_eq!(std::fs::read(&extracted).unwrap(), media().cover, "embedded cover is the original bytes");
    assert_eq!(
        h.phone.lock().unwrap().pulls,
        vec![format!("{remote}/80/video.m4s"), format!("{remote}/80/audio.m4s"), format!("{remote}/cover.jpg")],
        "unselected item not transferred"
    );
    assert!(
        !h.dir.join("output").join("cover.jpg").exists() && !h.dir.join("output").join(".bili-temp").read_dir().unwrap().any(|_| true),
        "no stray cover or temp"
    );
    // Same title again: numbered, earlier file untouched.
    let before = std::fs::read(&target).unwrap();
    h.app.export(r.generation, vec![sel(id)]).unwrap();
    assert_eq!(exported(&h), vec!["中文_影片 — 第一集 (1).mp4", "中文_影片 — 第一集.mp4"]);
    assert_eq!(std::fs::read(&target).unwrap(), before);
    let progress: Vec<String> =
        h.events.lock().unwrap().iter().filter_map(|e| if let Event::Progress { progress } = e { Some(progress.stage.clone()) } else { None }).collect();
    for stage in ["傳輸 video.m4s", "檢查串流", "合併 MP4", "嵌入原始封面", "驗證影音完整性", "完成"] {
        assert!(progress.iter().any(|p| p == stage), "missing stage {stage}");
    }
}

#[test]
fn cover_problems_keep_valid_video_as_partial() {
    let mut phone = Phone::one_device();
    let none = phone.add("1/c_1", "沒有封面", &["80"], false);
    let broken = phone.add("2/c_2", "壞封面", &["80"], true);
    phone.files.insert(format!("{broken}/cover.jpg"), b"not an image at all".to_vec());
    let h = harness(phone);
    let r = h.app.connect(None).unwrap();
    let ids: Vec<Selection> = r.items.iter().map(|i| sel(i.id)).collect();
    let summary = h.app.export(r.generation, ids).unwrap();
    assert_eq!(summary.partial, 2, "{:?}", results(&h));
    for name in exported(&h) {
        let streams = probe(&h.dir.join("output").join(&name))["streams"].as_array().unwrap().clone();
        assert!(streams.iter().any(|s| s["codec_name"] == "h264") && streams.iter().all(|s| s["disposition"]["attached_pic"] == 0));
    }
    let messages = results(&h);
    assert!(messages.iter().any(|(_, o, m)| *o == Outcome::Partial && m.contains("沒有封面圖片")));
    assert!(messages.iter().any(|(_, o, m)| *o == Outcome::Partial && m.contains("JPEG／PNG")));
    let _ = none;
}

fn direct_export(
    phone: Phone,
    remote: &str,
    output: &Path,
    space: Option<Box<bili_core::storage::Space>>,
) -> Result<bili_core::export::ExportResult, bili_core::Error> {
    let shared = std::sync::Arc::new(std::sync::Mutex::new(phone));
    let log = Logger::memory();
    let runner = runner(&shared, log.clone());
    let adb = Adb { runner: &runner, exe: "fake-adb.exe".into() };
    let state = tempdir();
    let storage = match space {
        Some(space) => Storage::with_space(&state, log, space).unwrap(),
        None => Storage::new(&state, log).unwrap(),
    };
    let items = adb.scan(SERIAL, &Cancel::default(), &mut |_| {}).unwrap();
    let item = items.iter().find(|i| i.remote == remote).unwrap();
    let variant = item.exportable_variant(None).unwrap();
    let request = Request { serial: SERIAL, item, variant, ffmpeg: &ffmpeg(), ffprobe: &ffprobe(), output: &output.to_string_lossy(), temp: "" };
    export(&runner, &adb, &storage, &request, &Cancel::default(), &mut |_, _| {})
}

#[test]
fn bad_media_never_reported_as_success() {
    for (name, data, expected) in [
        ("封面冒充", media().cover_only.clone(), "正片 video"),
        ("全零", vec![0u8; media().video.len()], "ffprobe"),
        ("損壞封包", media().damaged.clone(), "video"),
    ] {
        let mut phone = Phone::one_device();
        let remote = phone.add("1/c_1", name, &["80"], true);
        phone.files.insert(format!("{remote}/80/video.m4s"), data);
        let out = tempdir();
        let e = direct_export(phone, &remote, &out, None).unwrap_err();
        assert!(e.to_string().contains(expected), "{name}: {e}");
        assert!(mp4s(&out).iter().all(|n| !n.ends_with(".mp4")), "{name}: no MP4 left: {:?}", mp4s(&out));
    }
}

#[test]
fn changed_source_space_and_unwritable_output_fail_safely() {
    let mut phone = Phone::one_device();
    let remote = phone.add("1/c_1", "變動", &["80"], true);
    let shared = std::sync::Arc::new(std::sync::Mutex::new(phone));
    let log = Logger::memory();
    let runner = runner(&shared, log.clone());
    let adb = Adb { runner: &runner, exe: "fake-adb.exe".into() };
    let items = adb.scan(SERIAL, &Cancel::default(), &mut |_| {}).unwrap();
    // Source changed after scan: size check catches it.
    shared.lock().unwrap().files.get_mut(&format!("{remote}/80/video.m4s")).unwrap().push(0);
    let out = tempdir();
    let storage = Storage::new(&tempdir(), log.clone()).unwrap();
    let request = Request {
        serial: SERIAL,
        item: &items[0],
        variant: &items[0].variants[0],
        ffmpeg: &ffmpeg(),
        ffprobe: &ffprobe(),
        output: &out.to_string_lossy(),
        temp: "",
    };
    let e = export(&runner, &adb, &storage, &request, &Cancel::default(), &mut |_, _| {}).unwrap_err();
    assert!(e.to_string().contains("傳輸大小與掃描時不同"));
    assert!(std::fs::read_dir(out.join(".bili-temp")).unwrap().next().is_none(), "temp cleaned");
    // Not enough space: refused before transfer.
    let pulls = shared.lock().unwrap().pulls.len();
    let tight = Storage::with_space(&tempdir(), log.clone(), Box::new(|_| Ok(("C:\\".into(), 1024)))).unwrap();
    let e = export(&runner, &adb, &tight, &request, &Cancel::default(), &mut |_, _| {}).unwrap_err();
    assert!(e.to_string().contains("空間不足"));
    assert_eq!(shared.lock().unwrap().pulls.len(), pulls);
    // Output "folder" is a file.
    std::fs::write(out.join("file"), b"x").unwrap();
    let file_output = out.join("file").to_string_lossy().into_owned();
    let request = Request { output: &file_output, ..request };
    assert!(export(&runner, &adb, &storage, &request, &Cancel::default(), &mut |_, _| {}).unwrap_err().to_string().contains("無法寫入"));
    // Separate temp drive path works (different folder, same machine).
    let temp = tempdir();
    let ok_out = tempdir();
    shared.lock().unwrap().files.get_mut(&format!("{remote}/80/video.m4s")).unwrap().pop();
    let (ok_out, temp_text) = (ok_out.to_string_lossy().into_owned(), temp.to_string_lossy().into_owned());
    let request = Request { output: &ok_out, temp: &temp_text, ..request };
    export(&runner, &adb, &storage, &request, &Cancel::default(), &mut |_, _| {}).unwrap();
    assert!(std::fs::read_dir(&temp).unwrap().next().is_none(), "custom temp cleaned");
}

#[test]
fn reserved_and_long_names_are_safe() {
    let mut phone = Phone::one_device();
    let remote = phone.add("1/c_1", "CON", &["80"], true);
    phone.files.insert(format!("{remote}/entry.json"), br#"{"title":"CON"}"#.to_vec());
    let out = tempdir();
    let r = direct_export(phone, &remote, &out, None).unwrap();
    assert!(r.path.ends_with("_CON.mp4"));
    let mut phone = Phone::one_device();
    let long = "長".repeat(180) + "?*";
    let remote = phone.add("1/c_1", &long, &["80"], true);
    let r = direct_export(phone, &remote, &out, None).unwrap();
    assert!(Path::new(&r.path).file_stem().unwrap().to_string_lossy().chars().count() <= 110);
}

// ---------- 04/06: disconnect, cancel, batch, retry, recovery ----------

#[test]
fn disconnect_before_and_during_export() {
    let mut phone = Phone::one_device();
    phone.add("1/c_1", "A", &["80"], true);
    phone.add("2/c_2", "B", &["80"], true);
    let h = harness(phone);
    let r = h.app.connect(None).unwrap();
    h.phone.lock().unwrap().pulls.clear();
    h.phone.lock().unwrap().devices = "List of devices attached\n".into();
    let e = h.app.export(r.generation, vec![sel(0)]).unwrap_err();
    assert!(e.message.contains("尚未開始匯出") && h.phone.lock().unwrap().pulls.is_empty());
    // Reconnect does not resume; then unplug mid-transfer.
    let devices = Phone::one_device().devices;
    h.phone.lock().unwrap().devices = devices;
    let r = h.app.connect(None).unwrap();
    assert!(exported(&h).is_empty(), "no auto resume");
    h.phone.lock().unwrap().fail_pull = Some("1/c_1/80/audio.m4s".into());
    let s = h.app.export(r.generation, vec![sel(0), sel(1)]).unwrap();
    assert_eq!((s.failed, s.successful), (1, 1), "failure continues with next item");
    assert!(results(&h).iter().any(|(_, o, m)| *o == Outcome::Failed && m.contains("offline")));
    assert_eq!(exported(&h).len(), 1);
    assert!(std::fs::read_dir(h.dir.join("output").join(".bili-temp")).unwrap().next().is_none());
}

#[test]
fn cancel_stops_queue_keeps_finished_and_retry_works() {
    let mut phone = Phone::one_device();
    for (id, t) in [("1/c_1", "第一部"), ("2/c_2", "第二部"), ("3/c_3", "第三部")] {
        phone.add(id, t, &["80"], true);
    }
    phone.cancel_pull = Some("2/c_2/80/video.m4s".into());
    let h = harness(phone);
    let r = h.app.connect(None).unwrap();
    let all: Vec<Selection> = r.items.iter().map(|i| sel(i.id)).collect();
    let s = h.app.export(r.generation, all).unwrap();
    assert_eq!((s.successful, s.cancelled, s.not_started), (1, 1, 1));
    assert_eq!(exported(&h), vec!["第一部 — 第一集.mp4"]);
    assert!(std::fs::read_dir(h.dir.join("output").join(".bili-temp")).unwrap().next().is_none(), "cancelled job cleaned");
    assert!(mp4s(&h.dir.join("output")).iter().all(|n| !n.contains("partial")));
    // Manual retry of the unfinished items re-transfers them; it never reuses leftovers.
    h.phone.lock().unwrap().cancel_pull = None;
    h.phone.lock().unwrap().pulls.clear();
    let s = h.app.export(r.generation, vec![sel(1), sel(2)]).unwrap();
    assert_eq!(s.successful, 2);
    assert_eq!(h.phone.lock().unwrap().pulls.iter().filter(|p| p.ends_with("video.m4s")).count(), 2);
    assert_eq!(exported(&h).len(), 3);
}

#[test]
fn partial_retry_adds_numbered_file_without_overwriting() {
    let mut phone = Phone::one_device();
    let remote = phone.add("1/c_1", "封面稍後", &["80"], true);
    phone.fail_pull = Some("cover.jpg".into());
    let h = harness(phone);
    // Scan pulls covers too; let that one succeed.
    h.phone.lock().unwrap().fail_pull = None;
    let r = h.app.connect(None).unwrap();
    h.phone.lock().unwrap().fail_pull = Some(format!("{remote}/cover.jpg"));
    assert_eq!(h.app.export(r.generation, vec![sel(0)]).unwrap().partial, 1);
    let first = std::fs::read(h.dir.join("output").join("封面稍後 — 第一集.mp4")).unwrap();
    h.phone.lock().unwrap().fail_pull = None;
    assert_eq!(h.app.export(r.generation, vec![sel(0)]).unwrap().successful, 1);
    assert_eq!(exported(&h), vec!["封面稍後 — 第一集 (1).mp4", "封面稍後 — 第一集.mp4"]);
    assert_eq!(std::fs::read(h.dir.join("output").join("封面稍後 — 第一集.mp4")).unwrap(), first);
}

#[test]
fn cancel_during_cover_stage_leaves_no_output() {
    let mut phone = Phone::one_device();
    let remote = phone.add("1/c_1", "封面時取消", &["80"], true);
    let h = harness(phone);
    let r = h.app.connect(None).unwrap();
    h.phone.lock().unwrap().cancel_pull = Some(format!("{remote}/cover.jpg"));
    let s = h.app.export(r.generation, vec![sel(0)]).unwrap();
    assert_eq!(s.cancelled, 1);
    assert!(exported(&h).is_empty() && mp4s(&h.dir.join("output")).iter().all(|n| !n.contains("partial")), "{:?}", mp4s(&h.dir.join("output")));
    assert!(std::fs::read_dir(h.dir.join("output").join(".bili-temp")).unwrap().next().is_none());
}

/// Temp on another volume, e.g. BILI_TEST_OTHER_VOLUME=D:\ (skipped when not set).
#[test]
fn temp_on_another_volume() {
    let Ok(volume) = std::env::var("BILI_TEST_OTHER_VOLUME") else { return };
    let mut phone = Phone::one_device();
    let remote = phone.add("1/c_1", "跨磁碟", &["80"], true);
    let shared = std::sync::Arc::new(std::sync::Mutex::new(phone));
    let log = Logger::memory();
    let runner = runner(&shared, log.clone());
    let adb = Adb { runner: &runner, exe: "fake-adb.exe".into() };
    let items = adb.scan(SERIAL, &Cancel::default(), &mut |_| {}).unwrap();
    let temp = Path::new(&volume).join(format!("bili-test-temp-{}", uuid::Uuid::new_v4().simple()));
    let out = tempdir();
    let (out_text, temp_text) = (out.to_string_lossy().into_owned(), temp.to_string_lossy().into_owned());
    assert_ne!(bili_core::storage::disk_space(&out).unwrap().0, bili_core::storage::disk_space(Path::new(&volume)).unwrap().0);
    let storage = Storage::new(&tempdir(), log).unwrap();
    let request = Request {
        serial: SERIAL,
        item: &items[0],
        variant: &items[0].variants[0],
        ffmpeg: &ffmpeg(),
        ffprobe: &ffprobe(),
        output: &out_text,
        temp: &temp_text,
    };
    let result = export(&runner, &adb, &storage, &request, &Cancel::default(), &mut |_, _| {}).unwrap();
    assert!(result.cover_error.is_none() && Path::new(&result.path).exists() && remote.ends_with("c_1"));
    assert!(std::fs::read_dir(&temp).unwrap().next().is_none(), "temp on other volume cleaned");
    std::fs::remove_dir(&temp).unwrap();
}

#[test]
fn crash_leftovers_are_recovered_only_with_ownership_proof() {
    let h = harness(Phone::one_device());
    let out = h.dir.join("output");
    let temp = out.join(".bili-temp");
    let registry = h.dir.join(".bili-jobs");
    std::fs::create_dir_all(&registry).unwrap();
    let make = |id: &str, marker: &str| {
        let job = bili_core::storage::TemporaryJob { id: id.into(), temp_root: temp.to_string_lossy().into(), output_root: out.to_string_lossy().into() };
        std::fs::create_dir_all(job.directory()).unwrap();
        std::fs::write(job.directory().join(".bili-owned"), marker).unwrap();
        std::fs::write(job.directory().join("video.m4s"), b"leftover").unwrap();
        std::fs::write(job.partial_path(), b"partial").unwrap();
        std::fs::write(registry.join(format!("{id}.json")), serde_json::to_string(&job).unwrap()).unwrap();
        job
    };
    let owned = make("0123456789abcdef0123456789abcdef", "0123456789abcdef0123456789abcdef");
    let foreign = make("fedcba9876543210fedcba9876543210", "someone-else");
    std::fs::write(out.join("keep.mp4"), b"user video").unwrap();
    // A live job (lock held) must be skipped.
    let state = h.dir.clone();
    let storage = Storage::new(&state, Logger::memory()).unwrap();
    let live = storage.create_job(&out.to_string_lossy(), "", 1).unwrap();
    assert_eq!(h.app.snapshot().recoverable_jobs, 1, "only the crashed job with valid ownership is offered");
    let (cleaned, kept) = h.app.recover_jobs().unwrap();
    assert_eq!((cleaned, kept), (1, 0));
    assert!(!owned.directory().exists() && !owned.partial_path().exists());
    assert!(foreign.directory().exists() && foreign.partial_path().exists(), "unproven ownership kept");
    assert!(live.record.directory().exists(), "active job untouched");
    assert_eq!(std::fs::read(out.join("keep.mp4")).unwrap(), b"user video");
    drop(live);
}

#[test]
fn cancel_during_real_ffmpeg_stops_process() {
    let runner = bili_core::runner::Runner::new(Logger::memory());
    let cancel = Cancel::default();
    let c = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        c.cancel();
    });
    let started = std::time::Instant::now();
    let args =
        bili_core::runner::strings(&["-hide_banner", "-nostdin", "-v", "error", "-re", "-f", "lavfi", "-i", "testsrc2=size=32x32:rate=1", "-f", "null", "-"]);
    let e = runner.run(&ffmpeg().to_string_lossy(), &args, &cancel, &mut |_| {}, 30).unwrap_err();
    assert_eq!(e, bili_core::Error::Cancelled);
    assert!(started.elapsed().as_secs() < 5);
    let e = runner.run(&ffmpeg().to_string_lossy(), &args, &Cancel::default(), &mut |_| {}, 1).unwrap_err();
    assert!(matches!(e, bili_core::Error::Timeout(ref m) if !m.contains("手機")));
}
