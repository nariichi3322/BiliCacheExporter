use crate::log::Logger;
use crate::{fail, Error, Res};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const RESERVE: u64 = 64 * 1024 * 1024;
const REPARSE_POINT: u32 = 0x400;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct TemporaryJob {
    pub id: String,
    pub temp_root: String,
    pub output_root: String,
}

impl TemporaryJob {
    pub fn directory(&self) -> PathBuf {
        Path::new(&self.temp_root).join(format!("bili-export-{}", self.id))
    }
    pub fn partial_path(&self) -> PathBuf {
        Path::new(&self.output_root).join(format!(".bili-{}.partial.mp4", self.id))
    }
    pub fn cover_path(&self) -> PathBuf {
        Path::new(&self.output_root).join(format!(".bili-{}.partial.mp4.cover.mp4", self.id))
    }
}

pub type Space = dyn Fn(&Path) -> Res<(String, u64)> + Send + Sync;

pub struct Storage {
    registry: PathBuf,
    log: Arc<Logger>,
    space: Box<Space>,
}

/// An active job: holds the exclusive lock file so recovery never touches it; cleans up on drop.
pub struct Job<'a> {
    pub record: TemporaryJob,
    lock: Option<File>,
    storage: &'a Storage,
}

impl Drop for Job<'_> {
    fn drop(&mut self) {
        let cleaned = match self.storage.clean(&self.record) {
            Ok(()) => true,
            Err(e) => {
                self.storage.log.log(&format!("暫存清理失敗：{e}；目錄：{}", self.record.directory().display()));
                false
            }
        };
        drop(self.lock.take());
        if cleaned {
            if let Err(e) = std::fs::remove_file(self.storage.lock_path(&self.record)) {
                self.storage.log.log(&format!("暫存鎖清理失敗：{e}"));
            }
        }
    }
}

impl Storage {
    pub fn new(state_dir: &Path, log: Arc<Logger>) -> Res<Self> {
        Ok(Storage { registry: full_directory(state_dir)?.join(".bili-jobs"), log, space: Box::new(disk_space) })
    }

    pub fn with_space(state_dir: &Path, log: Arc<Logger>, space: Box<Space>) -> Res<Self> {
        Ok(Storage { registry: full_directory(state_dir)?.join(".bili-jobs"), log, space })
    }

    /// Checks writability and space for source, merged and covered outputs, then registers ownership.
    pub fn create_job(&self, output_root: &str, temp_root: &str, media_bytes: u64) -> Res<Job<'_>> {
        let output = full_directory(Path::new(output_root))?;
        let temp = if temp_root.trim().is_empty() { output.join(".bili-temp") } else { full_directory(Path::new(temp_root))? };
        ensure_writable(&output)?;
        ensure_writable(&temp)?;
        ensure_writable(&self.registry)?;
        if media_bytes == 0 || media_bytes > (u64::MAX - RESERVE) / 3 {
            return fail("影音大小無效，無法檢查暫存空間。");
        }
        let (temp_volume, temp_free) = (self.space)(&temp)?;
        let (output_volume, output_free) = (self.space)(&output)?;
        // shortcut: cover and container overhead reserve 64 MiB; size from the real cover if huge covers appear.
        if temp_volume.eq_ignore_ascii_case(&output_volume) {
            if temp_free.min(output_free) < 3 * media_bytes + RESERVE {
                return fail("輸出與暫存共用磁碟的可用空間不足（需容納來源、合併與封面輸出）。");
            }
        } else if temp_free < media_bytes + RESERVE || output_free < 2 * media_bytes + RESERVE {
            return fail("輸出或暫存磁碟的可用空間不足。");
        }
        let record = TemporaryJob {
            id: uuid::Uuid::new_v4().simple().to_string(),
            temp_root: temp.to_string_lossy().into_owned(),
            output_root: output.to_string_lossy().into_owned(),
        };
        let lock = OpenOptions::new().read(true).write(true).create_new(true).share_mode(0).open(self.lock_path(&record))?;
        // On an init error the returned-early Job is dropped, which cleans whatever was created.
        let job = Job { record: record.clone(), lock: Some(lock), storage: self };
        std::fs::create_dir_all(record.directory())?;
        std::fs::write(record.directory().join(".bili-owned"), &record.id)?;
        std::fs::write(self.record_path(&record), serde_json::to_string(&record).unwrap())?;
        Ok(job)
    }

    /// Jobs whose lock is free, i.e. left by a crashed or killed process.
    pub fn find_recoverable(&self) -> Vec<TemporaryJob> {
        let mut jobs = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.registry) else { return jobs };
        if guard(&self.registry).is_err() {
            return jobs;
        }
        for entry in entries.flatten() {
            let file = entry.path();
            if file.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let result: Res<Option<TemporaryJob>> = (|| {
                guard(&file)?;
                let record: TemporaryJob = serde_json::from_str(&std::fs::read_to_string(&file)?).map_err(|e| Error::Failed(e.to_string()))?;
                self.validate(&record)?;
                if file != self.record_path(&record) {
                    return fail("暫存紀錄名稱不符");
                }
                Ok(self.try_lock(&record)?.map(|_| record))
            })();
            match result {
                Ok(Some(record)) => jobs.push(record),
                Ok(None) => {}
                Err(e) => self.log.log(&format!("保留無法確認的暫存紀錄：{}；{e}", file.display())),
            }
        }
        jobs
    }

    pub fn recover(&self, record: &TemporaryJob) -> Res<()> {
        self.validate(record)?;
        let Some(lock) = self.try_lock(record)? else { return fail("暫存仍由另一個作業使用，不能清理。") };
        guard(&self.record_path(record))?;
        let stored: Option<TemporaryJob> = std::fs::read_to_string(self.record_path(record)).ok().and_then(|t| serde_json::from_str(&t).ok());
        if stored.as_ref() != Some(record) {
            return fail("暫存歸屬紀錄已改變或損壞，保留檔案。");
        }
        self.clean(record)?;
        drop(lock);
        std::fs::remove_file(self.lock_path(record))?;
        Ok(())
    }

    fn try_lock(&self, record: &TemporaryJob) -> Res<Option<File>> {
        guard(&self.lock_path(record))?;
        match OpenOptions::new().read(true).write(true).create(true).truncate(false).share_mode(0).open(self.lock_path(record)) {
            Ok(f) => Ok(Some(f)),
            Err(_) => Ok(None),
        }
    }

    fn record_path(&self, record: &TemporaryJob) -> PathBuf {
        self.registry.join(format!("{}.json", record.id))
    }

    fn lock_path(&self, record: &TemporaryJob) -> PathBuf {
        self.registry.join(format!("{}.lock", record.id))
    }

    fn validate(&self, record: &TemporaryJob) -> Res<()> {
        let id_ok = record.id.len() == 32 && record.id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
        if !id_ok
            || full_directory(Path::new(&record.temp_root))?.to_string_lossy() != record.temp_root
            || full_directory(Path::new(&record.output_root))?.to_string_lossy() != record.output_root
        {
            return fail("不合法的暫存歸屬紀錄。");
        }
        for path in [self.registry.clone(), record.directory(), record.partial_path(), record.cover_path()] {
            guard(&path)?;
        }
        let marker = record.directory().join(".bili-owned");
        guard(&marker)?;
        if record.directory().exists() && std::fs::read_to_string(&marker).ok().as_deref() != Some(record.id.as_str()) {
            return fail("暫存目錄缺少有效歸屬標記。");
        }
        Ok(())
    }

    fn clean(&self, record: &TemporaryJob) -> Res<()> {
        self.validate(record)?;
        if record.directory().exists() {
            guard_tree(&record.directory())?;
        }
        for partial in [record.partial_path(), record.cover_path()] {
            if partial.is_dir() {
                return fail(format!("未完成輸出被替換成目錄，保留檔案：{}", partial.display()));
            }
            if partial.exists() {
                std::fs::remove_file(&partial)?;
            }
        }
        if record.directory().exists() {
            std::fs::remove_dir_all(record.directory())?;
        }
        if self.record_path(record).exists() {
            std::fs::remove_file(self.record_path(record))?;
        }
        Ok(())
    }
}

fn full_directory(path: &Path) -> Res<PathBuf> {
    if !path.is_absolute() {
        return fail("請指定完整的輸出或暫存目錄路徑。");
    }
    let full = std::path::absolute(path)?;
    let text = full.to_string_lossy();
    let trimmed = text.trim_end_matches(['\\', '/']);
    // Keep a drive root like `D:\` intact.
    Ok(PathBuf::from(if trimmed.ends_with(':') { text.to_string() } else { trimmed.to_string() }))
}

pub fn ensure_writable(path: &Path) -> Res<()> {
    guard(path)?;
    std::fs::create_dir_all(path).map_err(|e| Error::Failed(format!("目錄無法寫入：{}（{e}）", path.display())))?;
    let probe = path.join(format!(".bili-write-{}", uuid::Uuid::new_v4().simple()));
    let result = OpenOptions::new().write(true).create_new(true).open(&probe);
    let _ = std::fs::remove_file(&probe);
    result.map(|_| ()).map_err(|e| Error::Failed(format!("目錄無法寫入：{}（{e}）", path.display())))
}

/// Refuses links/reparse points anywhere on the path so cleanup can never follow them elsewhere.
pub fn guard(path: &Path) -> Res<()> {
    let mut current = Some(path);
    while let Some(p) = current {
        if let Ok(meta) = std::fs::symlink_metadata(p) {
            if meta.file_attributes() & REPARSE_POINT != 0 {
                return fail(format!("為避免誤刪，不使用連結或重新解析點：{}", p.display()));
            }
        }
        current = p.parent();
    }
    Ok(())
}

fn guard_tree(dir: &Path) -> Res<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        guard(&path)?;
        if path.is_dir() {
            guard_tree(&path)?;
        }
    }
    Ok(())
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

/// Volume identity and free bytes for the volume holding `path`.
pub fn disk_space(path: &Path) -> Res<(String, u64)> {
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetVolumePathNameW};
    let mut volume = vec![0u16; 1024];
    let mut free = 0u64;
    let p = wide(path);
    unsafe {
        if GetVolumePathNameW(p.as_ptr(), volume.as_mut_ptr(), volume.len() as u32) == 0
            || GetDiskFreeSpaceExW(volume.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) == 0
        {
            return fail(format!("無法取得磁碟可用空間：{}", path.display()));
        }
    }
    let end = volume.iter().position(|&c| c == 0).unwrap_or(volume.len());
    Ok((String::from_utf16_lossy(&volume[..end]), free))
}
