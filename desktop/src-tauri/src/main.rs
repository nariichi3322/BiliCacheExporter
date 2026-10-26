#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bili_core::app::{err, App, ConnectResult, Event, Out, Selection, SettingsInput, Snapshot, ToolStatus};
use bili_core::batch::Summary;
use bili_core::log::Logger;
use bili_core::runner::Runner;
use bili_core::settings::Settings;
use bili_core::update::{self, Release};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tauri::{Emitter, Manager, State};

struct Shared {
    app: Arc<App>,
    started: Instant,
    measure: Option<PathBuf>,
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Out<T> + Send + 'static) -> Out<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| err(e.to_string()))?
}

#[tauri::command]
fn snapshot(s: State<Shared>) -> Snapshot {
    s.app.snapshot()
}

/// Called by the UI once mounted: the window becomes visible and operable from here.
#[tauri::command]
fn ready(s: State<Shared>, window: tauri::WebviewWindow) {
    let _ = window.show();
    let _ = window.set_focus();
    if let Some(path) = &s.measure {
        let _ = std::fs::write(path, format!("{}", s.started.elapsed().as_secs_f64() * 1000.0));
        window.app_handle().exit(0);
    }
}

#[tauri::command]
async fn save_settings(s: State<'_, Shared>, input: SettingsInput) -> Out<Settings> {
    let app = s.app.clone();
    blocking(move || app.save_settings(input)).await
}

#[tauri::command]
async fn reset_settings(s: State<'_, Shared>) -> Out<Settings> {
    let app = s.app.clone();
    blocking(move || app.reset_settings()).await
}

#[tauri::command]
async fn check_tools(s: State<'_, Shared>) -> Out<Vec<ToolStatus>> {
    let app = s.app.clone();
    blocking(move || app.check_tools()).await
}

#[tauri::command]
async fn connect(s: State<'_, Shared>, serial: Option<String>) -> Out<ConnectResult> {
    let app = s.app.clone();
    blocking(move || app.connect(serial)).await
}

#[tauri::command]
async fn scan(s: State<'_, Shared>) -> Out<ConnectResult> {
    let app = s.app.clone();
    blocking(move || app.scan()).await
}

#[tauri::command]
async fn export(s: State<'_, Shared>, generation: u64, selection: Vec<Selection>) -> Out<Summary> {
    let app = s.app.clone();
    blocking(move || app.export(generation, selection)).await
}

#[tauri::command]
fn cancel(s: State<Shared>) {
    s.app.cancel()
}

#[tauri::command]
async fn recover_jobs(s: State<'_, Shared>) -> Out<(usize, usize)> {
    let app = s.app.clone();
    blocking(move || app.recover_jobs()).await
}

#[tauri::command]
async fn download_tools(s: State<'_, Shared>, names: Vec<String>) -> Out<Settings> {
    let app = s.app.clone();
    blocking(move || app.download_tools(names)).await
}

#[tauri::command]
async fn check_update(s: State<'_, Shared>) -> Out<Release> {
    let app = s.app.clone();
    blocking(move || app.check_update()).await
}

/// Installs the update, then starts the new executable and exits this one.
#[tauri::command]
async fn install_update(s: State<'_, Shared>, handle: tauri::AppHandle, version: String) -> Out<()> {
    let app = s.app.clone();
    let exe = std::env::current_exe().map_err(|e| err(e.to_string()))?;
    let target = exe.clone();
    blocking(move || app.install_update(&target, &version)).await?;
    std::process::Command::new(&exe).current_dir(&s.app.dir).spawn().map_err(|e| err(format!("新版本已安裝，但無法自動重新啟動：{e}。請手動開啟程式。")))?;
    handle.exit(0);
    Ok(())
}

#[tauri::command]
async fn skip_version(s: State<'_, Shared>, version: String) -> Out<Settings> {
    let app = s.app.clone();
    blocking(move || app.skip_version(version)).await
}

/// Opens only the project's own GitHub pages; the UI cannot pass arbitrary URLs.
#[tauri::command]
fn open_url(kind: String) -> Out<()> {
    let url = match kind.as_str() {
        "project" => update::PROJECT_URL,
        "releases" => update::RELEASES_URL,
        _ => return Err(err("不支援的網址")),
    };
    shell_open(url).then_some(()).ok_or_else(|| err(format!("無法開啟瀏覽器，請手動前往 {url}")))
}

/// Opens only folders the app owns or the user configured; the UI cannot pass arbitrary paths.
#[tauri::command]
fn open_folder(s: State<Shared>, kind: String) -> Out<()> {
    let snap = s.app.snapshot();
    let path = match kind.as_str() {
        "log" => s.app.dir.join("log"),
        "output" => PathBuf::from(snap.settings.output.trim()),
        _ => return Err(err("不支援的資料夾")),
    };
    if !path.is_absolute() || !path.is_dir() {
        return Err(err(format!("資料夾不存在：{}", path.display())));
    }
    std::process::Command::new("explorer.exe").arg(&path).spawn().map(|_| ()).map_err(|e| err(e.to_string()))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// ShellExecuteW reports success with a value above 32.
fn shell_open(target: &str) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let opened = unsafe { ShellExecuteW(std::ptr::null_mut(), wide("open").as_ptr(), wide(target).as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
    opened as usize > 32
}

const WEBVIEW2_URL: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";

/// Native guidance shown before (or instead of) the web UI, since the page itself needs WebView2.
fn webview2_guidance(title: &str, problem: &str, detail: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_YESNO};
    let text = format!(
        "{problem}\n\n({detail})\n\n按「是」開啟 Microsoft 官方下載頁，安裝「Evergreen Bootstrapper」後再重新開啟本程式。\n按「否」結束程式，不會變更任何檔案。"
    );
    let answer = unsafe { MessageBoxW(std::ptr::null_mut(), wide(&text).as_ptr(), wide(title).as_ptr(), MB_YESNO | MB_ICONWARNING) };
    if answer == IDYES && !shell_open(WEBVIEW2_URL) {
        let text = format!("無法自動開啟瀏覽器。請手動前往下列網址下載 WebView2 Runtime：\n\n{WEBVIEW2_URL}");
        unsafe { MessageBoxW(std::ptr::null_mut(), wide(&text).as_ptr(), wide(title).as_ptr(), MB_OK | MB_ICONERROR) };
    }
}

fn main() {
    let started = Instant::now();
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--version") {
        println!("{}", bili_core::VERSION);
        return;
    }
    if let Err(e) = tauri::webview_version() {
        webview2_guidance("需要安裝 WebView2", "此程式需要 Microsoft Edge WebView2 Runtime 才能顯示介面，但這台電腦目前找不到可用的版本。", &e.to_string());
        std::process::exit(2);
    }
    let measure = args.iter().position(|a| a == "--measure-startup").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())).expect("program folder");
    let (log, storage_error) = match Logger::open(&dir) {
        Ok(log) => (log, None),
        Err(e) => (Logger::memory(), Some(format!("程式資料夾無法寫入（{}）：{e}", dir.display()))),
    };
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |tauri_app| {
            let handle = tauri_app.handle().clone();
            let emit: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(move |event| {
                let _ = handle.emit("app-event", event);
            });
            let app = App::new(dir.clone(), Runner::new(log.clone()), Box::new(bili_core::tools::https_fetch), emit, storage_error.clone());
            tauri_app.manage(Shared { app: Arc::new(app), started, measure: measure.clone() });
            if let Some(window) = tauri_app.get_webview_window("main") {
                let _ = window.set_title(&format!("Bili 快取匯出 {}", bili_core::VERSION));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            ready,
            save_settings,
            reset_settings,
            check_tools,
            connect,
            scan,
            export,
            cancel,
            recover_jobs,
            download_tools,
            open_folder,
            check_update,
            install_update,
            skip_version,
            open_url
        ])
        .build(tauri::generate_context!());
    // A runtime that exists but cannot start (damaged install, policy) gets the same guidance instead of a silent crash.
    match result {
        Ok(app) => app.run(|handle, event| {
            // The adb server outlives its client and would keep the portable folder in use.
            if let tauri::RunEvent::Exit = event {
                handle.state::<Shared>().app.stop_own_adb();
            }
        }),
        Err(e) => {
            webview2_guidance("介面無法啟動", "介面無法啟動，WebView2 Runtime 可能損壞或被系統原則封鎖。請重新安裝 WebView2 Runtime 後再試。", &e.to_string());
            std::process::exit(1);
        }
    }
}
