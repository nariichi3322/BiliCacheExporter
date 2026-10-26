use crate::runner::{strings, Runner};
use crate::scan::{file_name, is_child_path, Adb, CacheItem, Variant, ROOT};
use crate::storage::Storage;
use crate::{fail, Cancel, Error, Res};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct Request<'a> {
    pub serial: &'a str,
    pub item: &'a CacheItem,
    pub variant: &'a Variant,
    pub ffmpeg: &'a Path,
    pub ffprobe: &'a Path,
    pub output: &'a str,
    pub temp: &'a str,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    /// Set when the video is valid but the cover could not be embedded (partial completion).
    pub cover_error: Option<String>,
}

pub type Progress<'a> = &'a mut dyn FnMut(Option<f64>, &str);

pub fn export(runner: &Runner, adb: &Adb, storage: &Storage, req: &Request, cancel: &Cancel, progress: Progress) -> Res<ExportResult> {
    cancel.check()?;
    let v = req.variant;
    if !v.issue.is_empty() {
        return fail(v.issue.clone());
    }
    if v.video_size == 0 || v.audio_size == 0 || v.video_size.checked_add(v.audio_size).is_none() {
        return fail("影音大小無效，請重新掃描並確認下載已完成。");
    }
    let remote = &req.item.remote;
    if !is_child_path(remote, ROOT)
        || !is_child_path(&v.video, remote)
        || !v.video.ends_with("/video.m4s")
        || v.audio != format!("{}audio.m4s", &v.video[..v.video.len() - "video.m4s".len()])
    {
        return fail("影音來源必須屬於同一個分 P 與畫質目錄，請重新掃描。");
    }
    for (tool, name) in [(req.ffmpeg, "ffmpeg.exe"), (req.ffprobe, "ffprobe.exe")] {
        if !tool.is_absolute() || !tool.is_file() {
            return fail(format!("請在設定指定有效的 {name}。"));
        }
    }
    if !Path::new(req.output).is_absolute() {
        return fail("請指定輸出目錄的完整路徑。");
    }
    let ffmpeg = req.ffmpeg.to_string_lossy().into_owned();
    let ffprobe = req.ffprobe.to_string_lossy().into_owned();
    let job = storage.create_job(req.output, req.temp, v.video_size + v.audio_size)?;
    let temp = job.record.directory();
    let partial = job.record.partial_path();
    let covered = job.record.cover_path();
    let s = |p: &Path| p.to_string_lossy().into_owned();

    let percent = Regex::new(r"(\d+)%").unwrap();
    let pull = |remote: &str, local: &Path, size: u64, progress: &mut dyn FnMut(Option<f64>, &str)| -> Res<()> {
        let stage = format!("傳輸 {}", local.file_name().unwrap().to_string_lossy());
        progress(None, &stage);
        // shortcut: ADB text progress differs by version, so without a percentage only the stage is shown; switch to a byte-counting transfer if users need exact progress.
        adb.pull(req.serial, remote, &s(local), cancel, &mut |line| {
            if let Some(m) = percent.captures(line) {
                progress(Some((m[1].parse::<f64>().unwrap_or(0.0) / 100.0).clamp(0.0, 1.0)), &stage);
            }
        })?;
        let actual = std::fs::metadata(local).map(|m| m.len()).unwrap_or(u64::MAX);
        if actual != size {
            return fail("傳輸大小與掃描時不同，請重新掃描，確認下載已完成。");
        }
        progress(Some(1.0), &format!("{stage} 完成"));
        Ok(())
    };
    let video = temp.join("video.m4s");
    let audio = temp.join("audio.m4s");
    pull(&v.video, &video, v.video_size, progress)?;
    pull(&v.audio, &audio, v.audio_size, progress)?;

    let probe = |path: &Path, kind: &str| -> Res<f64> {
        let json = runner.run(&ffprobe, &strings(&["-v", "error", "-show_streams", "-show_format", "-of", "json", &s(path)]), cancel, &mut |_| {}, 180)?;
        let doc: Value = serde_json::from_str(&json).map_err(|e| Error::Failed(format!("FFprobe 輸出無法解析：{e}")))?;
        let Some(streams) = doc.get("streams").and_then(Value::as_array) else { return fail("FFprobe 未找到影音串流。") };
        if streams.iter().any(|s| matches!(s.get("codec_tag_string").and_then(Value::as_str), Some("encv" | "enca"))) {
            return fail("串流標記為加密格式，不支援 DRM 解密。");
        }
        let main = streams.iter().any(|s| {
            s.get("codec_type").and_then(Value::as_str) == Some(kind)
                && s.get("codec_name").and_then(Value::as_str).is_some_and(|n| !n.trim().is_empty() && n != "unknown")
                && (kind != "video" || s.pointer("/disposition/attached_pic").and_then(Value::as_i64).unwrap_or(0) == 0)
        });
        if !main {
            return fail(format!("FFprobe 無法辨識正片 {kind} 串流；可能是不完整檔案、不支援格式或加密，不會嘗試修補或解密。"));
        }
        Ok(doc
            .pointer("/format/duration")
            .and_then(|d| d.as_str().map(str::to_string).or_else(|| Some(d.to_string())))
            .and_then(|d| d.parse().ok())
            .unwrap_or(0.0))
    };
    progress(None, "檢查串流");
    let duration = probe(&video, "video")?;
    probe(&audio, "audio")?;
    progress(None, "合併 MP4");
    runner.run(
        &ffmpeg,
        &strings(&[
            "-hide_banner",
            "-nostdin",
            "-n",
            "-xerror",
            "-i",
            &s(&video),
            "-i",
            &s(&audio),
            "-map",
            "0:V:0",
            "-map",
            "1:a:0",
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            "-progress",
            "pipe:1",
            "-nostats",
            &s(&partial),
        ]),
        cancel,
        &mut |line| {
            if let (Some(us), true) = (line.strip_prefix("out_time_us=").and_then(|v| v.parse::<f64>().ok()), duration > 0.0) {
                progress(Some((us / 1_000_000.0 / duration).clamp(0.0, 1.0)), "合併 MP4");
            }
        },
        7200,
    )?;
    let validate = |path: &Path, progress: &mut dyn FnMut(Option<f64>, &str)| -> Res<()> {
        probe(path, "video")?;
        probe(path, "audio")?;
        progress(None, "驗證影音完整性");
        runner.run(
            &ffmpeg,
            &strings(&[
                "-hide_banner",
                "-nostdin",
                "-v",
                "error",
                "-xerror",
                "-abort_on",
                "empty_output_stream",
                "-i",
                &s(path),
                "-map",
                "0:V:0",
                "-map",
                "0:a:0",
                "-f",
                "null",
                "-",
            ]),
            cancel,
            &mut |_| {},
            7200,
        )?;
        Ok(())
    };

    let cover_result: Res<()> = (|| {
        progress(None, "嵌入原始封面");
        if !req.item.has_cover {
            return fail("手機快取中沒有封面圖片（cover.jpg）。");
        }
        let cover = temp.join("cover.jpg");
        adb.pull(req.serial, &format!("{remote}/cover.jpg"), &s(&cover), cancel, &mut |_| {})?;
        let original: Value =
            serde_json::from_str(&runner.run(&ffprobe, &strings(&["-v", "error", "-show_streams", "-of", "json", &s(&cover)]), cancel, &mut |_| {}, 180)?)
                .map_err(|_| Error::Failed("FFprobe 無法辨識封面圖片。".into()))?;
        let image = original.pointer("/streams/0").cloned().unwrap_or(Value::Null);
        let codec = image.get("codec_name").and_then(Value::as_str).unwrap_or("").to_string();
        let width = image.get("width").and_then(Value::as_i64).unwrap_or(0);
        let height = image.get("height").and_then(Value::as_i64).unwrap_or(0);
        if !(codec == "mjpeg" || codec == "png") || width <= 0 || height <= 0 {
            return fail("封面不是可直接嵌入的 JPEG／PNG 圖片。");
        }
        runner.run(
            &ffmpeg,
            &strings(&[
                "-hide_banner",
                "-nostdin",
                "-v",
                "error",
                "-xerror",
                "-abort_on",
                "empty_output_stream",
                "-i",
                &s(&cover),
                "-map",
                "0:v:0",
                "-frames:v",
                "1",
                "-f",
                "null",
                "-",
            ]),
            cancel,
            &mut |_| {},
            180,
        )?;
        runner.run(
            &ffmpeg,
            &strings(&[
                "-hide_banner",
                "-nostdin",
                "-n",
                "-v",
                "error",
                "-xerror",
                "-i",
                &s(&partial),
                "-i",
                &s(&cover),
                "-map",
                "0:V:0",
                "-map",
                "0:a:0",
                "-map",
                "1:v:0",
                "-c",
                "copy",
                "-disposition:v:1",
                "attached_pic",
                "-movflags",
                "+faststart",
                &s(&covered),
            ]),
            cancel,
            &mut |_| {},
            7200,
        )?;
        let result: Value =
            serde_json::from_str(&runner.run(&ffprobe, &strings(&["-v", "error", "-show_streams", "-of", "json", &s(&covered)]), cancel, &mut |_| {}, 180)?)
                .map_err(|_| Error::Failed("未能確認原始封面已嵌入 MP4。".into()))?;
        let embedded = result.get("streams").and_then(Value::as_array).is_some_and(|streams| {
            streams.iter().any(|s| {
                s.pointer("/disposition/attached_pic").and_then(Value::as_i64) == Some(1)
                    && s.get("codec_name").and_then(Value::as_str) == Some(codec.as_str())
                    && s.get("width").and_then(Value::as_i64) == Some(width)
                    && s.get("height").and_then(Value::as_i64) == Some(height)
            })
        });
        if !embedded {
            return fail("未能確認原始封面已嵌入 MP4。");
        }
        validate(&covered, progress)
    })();
    let (chosen, cover_error) = match cover_result {
        Ok(()) => (covered.clone(), None),
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        Err(e) => {
            cancel.check()?;
            runner.log(&format!("封面匯出失敗，驗證並保留影片：{e}"));
            validate(&partial, progress)?;
            (partial.clone(), Some(e.to_string()))
        }
    };
    cancel.check()?;
    let name = file_name(&req.item.title);
    for i in 0..10_000 {
        let target: PathBuf = Path::new(req.output).join(format!("{name}{}.mp4", if i == 0 { String::new() } else { format!(" ({i})") }));
        match move_no_replace(&chosen, &target) {
            Ok(()) => {
                progress(Some(1.0), if cover_error.is_none() { "完成" } else { "部分完成：缺封面" });
                drop(job);
                return Ok(ExportResult { path: s(&target), cover_error });
            }
            Err(_) if target.exists() => continue,
            Err(e) => return fail(format!("無法寫入輸出檔：{e}")),
        }
    }
    fail("同名檔案過多，請整理輸出資料夾。")
}

/// Final placement never overwrites (std::fs::rename would replace an existing file on Windows).
fn move_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    // Raw Win32 calls are limited to MAX_PATH unless given the `\\?\` form (std adds it itself; we must too).
    let w = |p: &Path| -> std::io::Result<Vec<u16>> {
        let full = std::path::absolute(p)?.to_string_lossy().into_owned();
        let long = match full.strip_prefix(r"\\") {
            _ if full.starts_with(r"\\?\") => full,
            Some(unc) => format!(r"\\?\UNC\{unc}"),
            None => format!(r"\\?\{full}"),
        };
        Ok(std::ffi::OsStr::new(&long).encode_wide().chain(std::iter::once(0)).collect())
    };
    let (from, to) = (w(from)?, w(to)?);
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
