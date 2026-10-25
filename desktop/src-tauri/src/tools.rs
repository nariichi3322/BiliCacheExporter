use crate::runner::{strings, Runner};
use crate::storage::ensure_writable;
use crate::{fail, Cancel, Error, Res};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub name: &'static str,
    pub title: &'static str,
    pub version: &'static str,
    pub publisher: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    pub license: &'static str,
    /// (path inside the archive, file name under tools/<name>/)
    #[serde(skip)]
    pub files: &'static [(&'static str, &'static str)],
}

/// Pinned packages; hashes were checked against the publishers' own checksums on 2026-10-26.
pub const PACKAGES: &[Package] = &[
    Package {
        name: "adb",
        title: "Android SDK Platform-Tools（ADB）",
        version: "37.0.1",
        publisher: "Google（dl.google.com）",
        url: "https://dl.google.com/android/repository/platform-tools_r37.0.1-win.zip",
        sha256: "45f4d63113e895ebde0c90f194099a4676b6ac653bd28d54314a9e022bbc1a99",
        size: 8_044_989,
        license: "Android SDK License；由使用者自 Google 下載，本程式不再散布。",
        files: &[
            ("platform-tools/adb.exe", "adb.exe"),
            ("platform-tools/AdbWinApi.dll", "AdbWinApi.dll"),
            ("platform-tools/AdbWinUsbApi.dll", "AdbWinUsbApi.dll"),
            ("platform-tools/libwinpthread-1.dll", "libwinpthread-1.dll"),
            ("platform-tools/NOTICE.txt", "NOTICE.txt"),
            ("platform-tools/source.properties", "source.properties"),
        ],
    },
    Package {
        name: "ffmpeg",
        title: "FFmpeg 與 FFprobe（gyan.dev essentials）",
        version: "9.0.1",
        publisher: "gyan.dev（GitHub GyanD/codexffmpeg 發行版）",
        url: "https://github.com/GyanD/codexffmpeg/releases/download/9.0.1/ffmpeg-9.0.1-essentials_build.zip",
        sha256: "fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9",
        size: 111_253_802,
        license: "GPLv3（隨附 LICENSE）；由使用者自 GitHub 下載，本程式不再散布。",
        files: &[
            ("ffmpeg-9.0.1-essentials_build/bin/ffmpeg.exe", "ffmpeg.exe"),
            ("ffmpeg-9.0.1-essentials_build/bin/ffprobe.exe", "ffprobe.exe"),
            ("ffmpeg-9.0.1-essentials_build/LICENSE", "LICENSE"),
        ],
    },
];

/// Download seam: tests replace the network with local files or simulated failures.
pub type Fetch = dyn Fn(&str, &Path, u64, &Cancel, &mut dyn FnMut(u64)) -> Res<()> + Send + Sync;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Adb,
    Ffmpeg,
    Ffprobe,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Adb => "ADB",
            Kind::Ffmpeg => "FFmpeg",
            Kind::Ffprobe => "FFprobe",
        }
    }
}

/// Shared by manual paths and downloads: the file must exist and actually run as the expected tool.
pub fn check_tool(runner: &Runner, kind: Kind, path: &Path) -> Res<String> {
    let label = kind.label();
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return fail(format!("尚未設定 {label}。請在設定指定 {label} 的完整路徑，或下載工具。"));
    }
    if !path.is_file() {
        return fail(format!("找不到 {label}：{}。請重新指定或下載工具。", path.display()));
    }
    let (args, marker) = match kind {
        Kind::Adb => (strings(&["version"]), "Android Debug Bridge"),
        Kind::Ffmpeg => (strings(&["-hide_banner", "-version"]), "ffmpeg version"),
        Kind::Ffprobe => (strings(&["-hide_banner", "-version"]), "ffprobe version"),
    };
    let output =
        runner.run(&path.to_string_lossy(), &args, &Cancel::default(), &mut |_| {}, 20).map_err(|e| Error::Failed(format!("{label} 無法執行：{e}")))?;
    let first = output.lines().find(|l| l.contains(marker));
    match first {
        Some(line) => Ok(line.trim().to_string()),
        None => fail(format!("{label} 路徑指向的程式不是 {label}：{}", path.display())),
    }
}

pub fn https_fetch(url: &str, dest: &Path, max: u64, cancel: &Cancel, progress: &mut dyn FnMut(u64)) -> Res<()> {
    if !url.starts_with("https://") {
        return fail("只接受 HTTPS 下載來源。");
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_connect(Some(std::time::Duration::from_secs(30)))
        .timeout_recv_response(Some(std::time::Duration::from_secs(60)))
        .build()
        .into();
    let response = agent.get(url).call().map_err(|e| Error::Failed(format!("下載失敗：{e}。請檢查網路後重試，或改為手動指定工具。")))?;
    let mut reader = response.into_body().into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| Error::Failed(format!("無法寫入下載暫存：{e}")))?;
    let mut buffer = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        cancel.check()?;
        let n = reader.read(&mut buffer).map_err(|e| Error::Failed(format!("下載中斷：{e}。請重試。")))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > max {
            return fail("下載內容大於預期，已停止。");
        }
        file.write_all(&buffer[..n]).map_err(|e| Error::Failed(format!("下載寫入失敗（磁碟空間或權限）：{e}")))?;
        progress(total);
    }
    file.sync_all().map_err(|e| Error::Failed(format!("下載寫入失敗：{e}")))?;
    Ok(())
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub name: String,
    pub stage: String,
    pub bytes: u64,
    pub total: u64,
}

/// Downloads into a private staging folder, verifies hash, archive paths, files and executability,
/// and only then swaps the folder into tools/<name>; a failure leaves any existing tool untouched.
pub fn install(dir: &Path, runner: &Runner, package: &Package, fetch: &Fetch, cancel: &Cancel, progress: &mut dyn FnMut(DownloadProgress)) -> Res<PathBuf> {
    let tools = dir.join("tools");
    ensure_writable(&tools)?;
    let staging = tools.join(format!(".download-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&staging)?;
    let result = install_staged(&tools, &staging, runner, package, fetch, cancel, progress);
    if let Err(e) = std::fs::remove_dir_all(&staging) {
        runner.log(&format!("下載暫存清理失敗：{e}"));
    }
    result
}

fn install_staged(
    tools: &Path,
    staging: &Path,
    runner: &Runner,
    package: &Package,
    fetch: &Fetch,
    cancel: &Cancel,
    progress: &mut dyn FnMut(DownloadProgress),
) -> Res<PathBuf> {
    let mut report = |stage: &str, bytes: u64| progress(DownloadProgress { name: package.name.into(), stage: stage.into(), bytes, total: package.size });
    let archive = staging.join("package.zip");
    runner.log(&format!("下載 {} {}：{}", package.title, package.version, package.url));
    report("下載", 0);
    fetch(package.url, &archive, package.size, cancel, &mut |bytes| report("下載", bytes))?;
    cancel.check()?;
    report("檢查完整性", package.size);
    let bytes = std::fs::read(&archive).map_err(|e| Error::Failed(format!("無法讀取下載檔：{e}")))?;
    if bytes.len() as u64 != package.size {
        return fail(format!("下載大小不符（{} / {} bytes），可能未下載完整。請重試。", bytes.len(), package.size));
    }
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if hash != package.sha256 {
        return fail("下載檔案的 SHA-256 與發行時記錄的不符，已拒絕使用。請重試或改為手動指定工具。");
    }
    report("解壓縮", package.size);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| Error::Failed(format!("壓縮檔無法讀取：{e}")))?;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| Error::Failed(format!("壓縮檔無法讀取：{e}")))?;
        if entry.enclosed_name().is_none() {
            return fail("壓縮檔包含越界路徑，已拒絕解壓縮。");
        }
    }
    let out = staging.join(package.name);
    std::fs::create_dir_all(&out)?;
    for (inside, name) in package.files {
        cancel.check()?;
        let mut entry = zip.by_name(inside).map_err(|_| Error::Failed(format!("壓縮檔缺少必要檔案：{inside}")))?;
        let mut file = std::fs::File::create(out.join(name)).map_err(|e| Error::Failed(format!("解壓縮寫入失敗：{e}")))?;
        std::io::copy(&mut entry, &mut file).map_err(|e| Error::Failed(format!("解壓縮寫入失敗（磁碟空間或權限）：{e}")))?;
    }
    report("驗證工具可執行", package.size);
    for (kind, exe) in executables(package.name) {
        check_tool(runner, *kind, &out.join(exe))?;
    }
    cancel.check()?;
    let target = tools.join(package.name);
    let old = tools.join(format!(".old-{}-{}", package.name, uuid::Uuid::new_v4().simple()));
    if target.exists() {
        rename_retry(&target, &old).map_err(|e| Error::Failed(format!("無法替換現有工具（可能仍在使用中）：{e}")))?;
    }
    if let Err(e) = rename_retry(&out, &target) {
        if old.exists() {
            let _ = std::fs::rename(&old, &target);
        }
        return fail(format!("無法啟用下載的工具：{e}"));
    }
    if old.exists() {
        if let Err(e) = std::fs::remove_dir_all(&old) {
            runner.log(&format!("舊工具資料夾清理失敗：{e}"));
        }
    }
    report("完成", package.size);
    Ok(target)
}

/// Antivirus scans and just-exited tool processes briefly hold fresh executables open on Windows.
fn rename_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && attempt < 50 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            other => return other,
        }
    }
}

pub fn executables(name: &str) -> &'static [(Kind, &'static str)] {
    match name {
        "adb" => &[(Kind::Adb, "adb.exe")],
        _ => &[(Kind::Ffmpeg, "ffmpeg.exe"), (Kind::Ffprobe, "ffprobe.exe")],
    }
}
