use crate::batch::{self, BatchProgress, ItemResult, Summary};
use crate::log::Logger;
use crate::runner::Runner;
use crate::scan::{Adb, CacheItem, Device};
use crate::settings::{self, Settings};
use crate::storage::{Storage, TemporaryJob};
use crate::tools::{self, check_tool, DownloadProgress, Fetch, Kind, Package, PACKAGES};
use crate::update::{self, Release};
use crate::{Cancel, Error};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    Log { line: String },
    Stage { text: String },
    Item { item: CacheItem },
    Progress { progress: BatchProgress },
    Result { result: ItemResult },
    Download { progress: DownloadProgress },
}

/// Error sent to the UI; `missing` names tool packages the user can download to fix it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CmdError {
    pub message: String,
    pub cancelled: bool,
    pub missing: Vec<String>,
}

impl From<Error> for CmdError {
    fn from(e: Error) -> Self {
        CmdError { cancelled: e == Error::Cancelled, message: e.to_string(), missing: Vec::new() }
    }
}

pub fn err(message: impl Into<String>) -> CmdError {
    CmdError { message: message.into(), cancelled: false, missing: Vec::new() }
}

pub type Out<T> = Result<T, CmdError>;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub version: String,
    pub settings: Settings,
    pub settings_error: Option<String>,
    pub storage_error: Option<String>,
    pub log_path: Option<String>,
    pub devices: Vec<Device>,
    pub serial: Option<String>,
    pub items: Vec<CacheItem>,
    pub generation: u64,
    pub recoverable_jobs: usize,
    pub packages: Vec<Package>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsInput {
    pub adb: String,
    pub ffmpeg: String,
    pub ffprobe: String,
    pub output: String,
    pub temp: String,
    pub theme: String,
    pub check_updates: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub kind: String,
    pub path: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConnectResult {
    pub devices: Vec<Device>,
    /// Set when a device was connected and scanned; `None` means the user must pick one.
    pub serial: Option<String>,
    pub items: Vec<CacheItem>,
    pub generation: u64,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub id: usize,
    pub variant: Option<usize>,
}

#[derive(Default)]
struct State {
    settings: Option<Settings>,
    settings_error: Option<String>,
    devices: Vec<Device>,
    serial: Option<String>,
    items: Vec<CacheItem>,
    generation: u64,
}

pub struct App {
    pub dir: PathBuf,
    pub log: Arc<Logger>,
    runner: Runner,
    emit: Arc<dyn Fn(Event) + Send + Sync>,
    state: Mutex<State>,
    busy: AtomicBool,
    cancel: Mutex<Cancel>,
    fetch: Box<Fetch>,
    storage: Option<Storage>,
    storage_error: Option<String>,
}

struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst)
    }
}

impl App {
    /// Startup only reads settings; it never queries devices (no polling, no background monitor).
    pub fn new(dir: PathBuf, runner: Runner, fetch: Box<Fetch>, emit: Arc<dyn Fn(Event) + Send + Sync>, storage_error: Option<String>) -> App {
        let log = runner.log.clone();
        let sink = emit.clone();
        log.set_sink(Box::new(move |line| sink(Event::Log { line: line.to_string() })));
        let mut state = State::default();
        match settings::load(&dir) {
            Ok(s) => state.settings = Some(s),
            Err(e) => {
                log.log(&e.to_string());
                state.settings_error = Some(e.to_string());
            }
        }
        let (storage, storage_error) = match storage_error {
            Some(e) => (None, Some(e)),
            None => match Storage::new(&dir, log.clone()) {
                Ok(s) => (Some(s), None),
                Err(e) => (None, Some(e.to_string())),
            },
        };
        log.log(&format!("BiliCacheExporter {}", crate::VERSION));
        App {
            dir,
            log,
            runner,
            emit,
            state: Mutex::new(state),
            busy: AtomicBool::new(false),
            cancel: Mutex::new(Cancel::default()),
            fetch,
            storage,
            storage_error,
        }
    }

    fn begin(&self) -> Out<(Busy<'_>, Cancel)> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err(err("目前有作業進行中，請等待完成或先取消。"));
        }
        let cancel = Cancel::default();
        *self.cancel.lock().unwrap() = cancel.clone();
        Ok((Busy(&self.busy), cancel))
    }

    pub fn cancel(&self) {
        self.cancel.lock().unwrap().cancel();
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    fn settings(&self) -> Out<Settings> {
        let state = self.state.lock().unwrap();
        match (&state.settings, &state.settings_error) {
            (Some(s), _) => Ok(s.clone()),
            (None, e) => Err(err(e.clone().unwrap_or_default())),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let state = self.state.lock().unwrap();
        Snapshot {
            version: crate::VERSION.into(),
            settings: state.settings.clone().unwrap_or_else(Settings::defaults),
            settings_error: state.settings_error.clone(),
            storage_error: self.storage_error.clone(),
            log_path: self.log.path.as_ref().map(|p| crate::log::redact(&p.to_string_lossy())),
            devices: state.devices.clone(),
            serial: state.serial.clone(),
            items: state.items.clone(),
            generation: state.generation,
            recoverable_jobs: self.storage.as_ref().map(|s| s.find_recoverable().len()).unwrap_or(0),
            packages: PACKAGES.to_vec(),
        }
    }

    fn writable(&self) -> Out<()> {
        match &self.storage_error {
            Some(e) => Err(err(format!("{e}。請將整個程式資料夾移到可寫的位置後再開啟。"))),
            None => Ok(()),
        }
    }

    pub fn save_settings(&self, input: SettingsInput) -> Out<Settings> {
        self.writable()?;
        if self.is_busy() {
            return Err(err("作業進行中，暫時不能變更設定。"));
        }
        let mut s = self.settings()?;
        s.adb = settings::portable(&self.dir, &input.adb);
        s.ffmpeg = settings::portable(&self.dir, &input.ffmpeg);
        s.ffprobe = settings::portable(&self.dir, &input.ffprobe);
        s.output = input.output.trim().into();
        s.temp = input.temp.trim().into();
        s.theme = if input.theme == "light" { "light".into() } else { "dark".into() };
        s.check_updates = input.check_updates;
        settings::save(&self.dir, &s)?;
        self.state.lock().unwrap().settings = Some(s.clone());
        Ok(s)
    }

    pub fn reset_settings(&self) -> Out<Settings> {
        self.writable()?;
        let s = settings::backup_and_reset(&self.dir)?;
        let mut state = self.state.lock().unwrap();
        state.settings = Some(s.clone());
        state.settings_error = None;
        self.log.log("損壞的設定檔已備份，改用預設設定。");
        Ok(s)
    }

    pub fn tool_path(&self, s: &Settings, kind: Kind) -> PathBuf {
        let value = match kind {
            Kind::Adb => &s.adb,
            Kind::Ffmpeg => &s.ffmpeg,
            Kind::Ffprobe => &s.ffprobe,
        };
        if value.trim().is_empty() {
            PathBuf::new()
        } else {
            settings::resolve(&self.dir, value)
        }
    }

    pub fn check_tools(&self) -> Out<Vec<ToolStatus>> {
        let s = self.settings()?;
        Ok([Kind::Adb, Kind::Ffmpeg, Kind::Ffprobe]
            .into_iter()
            .map(|kind| {
                let path = self.tool_path(&s, kind);
                let result = check_tool(&self.runner, kind, &path);
                ToolStatus {
                    kind: kind.label().into(),
                    path: path.to_string_lossy().into(),
                    ok: result.is_ok(),
                    message: result.unwrap_or_else(|e| e.to_string()),
                }
            })
            .collect())
    }

    /// Cheap presence check before an operation; running the tool happens in the operation itself.
    fn require(&self, s: &Settings, kinds: &[Kind]) -> Out<()> {
        let mut missing: Vec<String> = Vec::new();
        let mut labels = Vec::new();
        for &kind in kinds {
            if !self.tool_path(s, kind).is_file() {
                labels.push(kind.label());
                let package = if kind == Kind::Adb { "adb" } else { "ffmpeg" };
                if !missing.iter().any(|m| m == package) {
                    missing.push(package.into());
                }
            }
        }
        if missing.is_empty() {
            return Ok(());
        }
        Err(CmdError {
            message: format!("缺少必要工具：{}。可下載並設定，或在設定頁指定已有的工具。", labels.join("、")), cancelled: false, missing
        })
    }

    fn adb_error(e: Error) -> CmdError {
        match e {
            Error::Timeout(_) => err("ADB 回應逾時。請重新插拔 USB、確認手機已解鎖後再按連線。"),
            Error::Failed(m) if m.starts_with("無法啟動") => err(format!("{m}。請在設定確認 ADB 路徑。")),
            other => other.into(),
        }
    }

    fn invalidate(&self) -> u64 {
        let mut state = self.state.lock().unwrap();
        state.serial = None;
        state.items.clear();
        state.generation += 1;
        state.generation
    }

    /// Manual connect: one device listing; a single authorized device (or the chosen one) is scanned at once.
    pub fn connect(&self, serial: Option<String>) -> Out<ConnectResult> {
        self.writable()?;
        let s = self.settings()?;
        self.require(&s, &[Kind::Adb])?;
        let (_busy, cancel) = self.begin()?;
        self.invalidate();
        (self.emit)(Event::Stage { text: "正在檢查裝置…".into() });
        let adb = Adb { runner: &self.runner, exe: self.tool_path(&s, Kind::Adb).to_string_lossy().into() };
        let devices = adb.devices(&cancel).map_err(Self::adb_error)?;
        self.state.lock().unwrap().devices = devices.clone();
        let chosen = match serial {
            Some(serial) => devices.iter().find(|d| d.serial == serial).cloned().ok_or_else(|| err("找不到所選裝置，可能已拔除。請確認 USB 連接後按連線。"))?,
            None => match devices.len() {
                0 => return Err(err("找不到 Android 裝置。請確認 USB 已連接、手機已開啟 USB 偵錯，再按連線。")),
                1 => devices[0].clone(),
                _ => return Ok(ConnectResult { devices, serial: None, items: Vec::new(), generation: self.state.lock().unwrap().generation }),
            },
        };
        device_ready(&chosen)?;
        self.state.lock().unwrap().serial = Some(chosen.serial.clone());
        self.scan_locked(&adb, &chosen.serial, &cancel).map(|(items, generation)| ConnectResult { devices, serial: Some(chosen.serial), items, generation })
    }

    /// Manual rescan re-checks that the selected device is still present and authorized.
    pub fn scan(&self) -> Out<ConnectResult> {
        self.writable()?;
        let s = self.settings()?;
        self.require(&s, &[Kind::Adb])?;
        let (_busy, cancel) = self.begin()?;
        let serial = self.state.lock().unwrap().serial.clone().ok_or_else(|| err("尚未連線裝置，請先按連線。"))?;
        let adb = Adb { runner: &self.runner, exe: self.tool_path(&s, Kind::Adb).to_string_lossy().into() };
        self.invalidate();
        let devices = adb.devices(&cancel).map_err(Self::adb_error)?;
        self.state.lock().unwrap().devices = devices.clone();
        let device = devices.iter().find(|d| d.serial == serial).ok_or_else(|| err("裝置已離線或已拔除。請確認 USB 連接後重新連線。"))?;
        device_ready(device)?;
        self.state.lock().unwrap().serial = Some(serial.clone());
        self.scan_locked(&adb, &serial, &cancel).map(|(items, generation)| ConnectResult { devices, serial: Some(serial), items, generation })
    }

    fn scan_locked(&self, adb: &Adb, serial: &str, cancel: &Cancel) -> Out<(Vec<CacheItem>, u64)> {
        (self.emit)(Event::Stage { text: "正在讀取快取影片…".into() });
        let result = adb.scan(serial, cancel, &mut |item| (self.emit)(Event::Item { item: item.clone() }));
        match result {
            Ok(items) => {
                let mut state = self.state.lock().unwrap();
                state.items = items.clone();
                self.log.log(&format!("掃描完成：{} 個項目", items.len()));
                Ok((items, state.generation))
            }
            Err(e) => {
                // A failed or interrupted scan leaves no list that could be exported by mistake.
                self.invalidate();
                Err(Self::adb_error(e))
            }
        }
    }

    pub fn export(&self, generation: u64, selection: Vec<Selection>) -> Out<Summary> {
        self.writable()?;
        let s = self.settings()?;
        self.require(&s, &[Kind::Adb, Kind::Ffmpeg, Kind::Ffprobe])?;
        let (_busy, cancel) = self.begin()?;
        let (serial, tasks) = {
            let state = self.state.lock().unwrap();
            if state.generation != generation {
                return Err(err("清單已更新，請重新選取要匯出的影片。"));
            }
            let serial = state.serial.clone().ok_or_else(|| err("尚未連線裝置，請重新連線。"))?;
            if selection.is_empty() {
                return Err(err("沒有選取可匯出的影片。"));
            }
            let mut tasks = Vec::new();
            for pick in &selection {
                let item = state.items.get(pick.id).ok_or_else(|| err("選取的項目不存在，請重新掃描。"))?;
                let variant = item.exportable_variant(pick.variant).map_err(|e| err(format!("{}：{e}", item.title)))?;
                tasks.push((item.clone(), variant.clone()));
            }
            (serial, tasks)
        };
        if s.output.trim().is_empty() || !Path::new(s.output.trim()).is_absolute() {
            return Err(err("請在下方指定輸出資料夾的完整路徑。"));
        }
        let adb = Adb { runner: &self.runner, exe: self.tool_path(&s, Kind::Adb).to_string_lossy().into() };
        (self.emit)(Event::Stage { text: "正在確認裝置…".into() });
        let devices = adb.devices(&cancel).map_err(Self::adb_error)?;
        match devices.iter().find(|d| d.serial == serial) {
            Some(d) if d.state == "device" => {}
            Some(d) => {
                device_ready(d)?;
            }
            None => {
                // Reconnecting never resumes the export by itself; the user starts it again.
                self.invalidate();
                return Err(err("裝置已離線或已拔除，尚未開始匯出。請重新連線後再試。"));
            }
        }
        let storage = self.storage.as_ref().ok_or_else(|| err("程式資料夾無法寫入。"))?;
        let (ffmpeg, ffprobe) = (self.tool_path(&s, Kind::Ffmpeg), self.tool_path(&s, Kind::Ffprobe));
        let tools = batch::Tools { ffmpeg: &ffmpeg, ffprobe: &ffprobe, output: s.output.trim(), temp: s.temp.trim() };
        let summary = batch::run(
            &self.runner,
            &adb,
            storage,
            &serial,
            &tasks,
            &tools,
            &cancel,
            &mut |progress| (self.emit)(Event::Progress { progress }),
            &mut |result| (self.emit)(Event::Result { result: result.clone() }),
        );
        self.log.log(&format!(
            "匯出結束：{} 成功，{} 部分完成，{} 失敗，{} 取消，{} 未開始",
            summary.successful, summary.partial, summary.failed, summary.cancelled, summary.not_started
        ));
        Ok(summary)
    }

    pub fn recoverable_jobs(&self) -> Vec<TemporaryJob> {
        self.storage.as_ref().map(|s| s.find_recoverable()).unwrap_or_default()
    }

    /// Cleans only jobs with a free lock and a valid ownership record; returns (cleaned, kept).
    pub fn recover_jobs(&self) -> Out<(usize, usize)> {
        let (_busy, _) = self.begin()?;
        let Some(storage) = &self.storage else { return Ok((0, 0)) };
        let (mut cleaned, mut kept) = (0, 0);
        for job in storage.find_recoverable() {
            match storage.recover(&job) {
                Ok(()) => {
                    cleaned += 1;
                    self.log.log(&format!("已清理殘留暫存：{}", job.directory().display()));
                }
                Err(e) => {
                    kept += 1;
                    self.log.log(&format!("殘留暫存清理失敗：{e}；{}", job.directory().display()));
                }
            }
        }
        Ok((cleaned, kept))
    }

    /// Asks GitHub for the latest release; works while busy since it changes nothing.
    pub fn check_update(&self) -> Out<Release> {
        Ok(update::check(&*self.fetch, crate::VERSION)?)
    }

    /// Called when the app exits; see [`tools::stop_own_adb`].
    pub fn stop_own_adb(&self) {
        tools::stop_own_adb(&self.dir, &self.runner)
    }

    /// Downloads the named packages (only when the user asks) and points settings at them.
    pub fn download_tools(&self, names: Vec<String>) -> Out<Settings> {
        self.writable()?;
        let mut s = self.settings()?;
        let (_busy, cancel) = self.begin()?;
        for name in &names {
            let package = PACKAGES.iter().find(|p| p.name == name).ok_or_else(|| err(format!("未知的工具：{name}")))?;
            let folder = tools::install(&self.dir, &self.runner, package, &*self.fetch, &cancel, &mut |progress| (self.emit)(Event::Download { progress }))?;
            let rel = |exe: &str| settings::portable(&self.dir, &folder.join(exe).to_string_lossy());
            if package.name == "adb" {
                s.adb = rel("adb.exe");
            } else {
                s.ffmpeg = rel("ffmpeg.exe");
                s.ffprobe = rel("ffprobe.exe");
            }
            settings::save(&self.dir, &s)?;
            self.state.lock().unwrap().settings = Some(s.clone());
            self.log.log(&format!("{} {} 已下載、驗證並啟用。", package.title, package.version));
        }
        Ok(s)
    }
}

fn device_ready(device: &Device) -> Out<()> {
    match device.state.as_str() {
        "device" => Ok(()),
        "unauthorized" => Err(err("手機尚未授權 USB 偵錯。請解鎖手機，在提示中允許這部電腦，然後再按連線。")),
        "offline" => Err(err("裝置目前離線（offline）。請重新插拔 USB，或關閉再開啟 USB 偵錯後再按連線。")),
        other => Err(err(format!("裝置狀態為 {other}，無法讀取快取。請將手機開機到正常系統並解鎖後再按連線。"))),
    }
}
