#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bili_core::app::{App, CmdError, ConnectResult, Event, Out, Selection, SettingsInput, Snapshot, ToolStatus};
use bili_core::batch::Summary;
use bili_core::log::Logger;
use bili_core::runner::Runner;
use bili_core::settings::Settings;
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
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| CmdError { message: e.to_string(), cancelled: false, missing: vec![] })?
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

/// Opens only folders the app owns or the user configured; the UI cannot pass arbitrary paths.
#[tauri::command]
fn open_folder(s: State<Shared>, kind: String) -> Out<()> {
    let snap = s.app.snapshot();
    let path = match kind.as_str() {
        "log" => s.app.dir.join("log"),
        "output" => PathBuf::from(snap.settings.output.trim()),
        _ => return Err(CmdError { message: "不支援的資料夾".into(), cancelled: false, missing: vec![] }),
    };
    if !path.is_absolute() || !path.is_dir() {
        return Err(CmdError { message: format!("資料夾不存在：{}", path.display()), cancelled: false, missing: vec![] });
    }
    std::process::Command::new("explorer.exe").arg(&path).spawn().map(|_| ()).map_err(|e| CmdError {
        message: e.to_string(),
        cancelled: false,
        missing: vec![],
    })
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Native guidance shown before any web UI exists, since the page itself needs WebView2.
fn webview2_missing(detail: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONWARNING, MB_YESNO};
    let text = format!(
        "此程式需要 Microsoft Edge WebView2 Runtime 才能顯示介面，但這台電腦目前找不到可用的版本。\n\n({detail})\n\n按「是」開啟 Microsoft 官方下載頁，安裝「Evergreen Bootstrapper」後再重新開啟本程式。\n按「否」結束程式，不會變更任何檔案。"
    );
    let answer = unsafe { MessageBoxW(std::ptr::null_mut(), wide(&text).as_ptr(), wide("需要安裝 WebView2").as_ptr(), MB_YESNO | MB_ICONWARNING) };
    if answer == IDYES {
        let url = wide("https://developer.microsoft.com/microsoft-edge/webview2/");
        let open = wide("open");
        unsafe { ShellExecuteW(std::ptr::null_mut(), open.as_ptr(), url.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
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
        webview2_missing(&e.to_string());
        std::process::exit(2);
    }
    let measure = args.iter().position(|a| a == "--measure-startup").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())).expect("program folder");
    let (log, storage_error) = match Logger::open(&dir) {
        Ok(log) => (log, None),
        Err(e) => (Logger::memory(), Some(format!("程式資料夾無法寫入（{}）：{e}", dir.display()))),
    };
    tauri::Builder::default()
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
            open_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
