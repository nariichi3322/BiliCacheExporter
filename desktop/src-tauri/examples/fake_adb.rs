//! Test-only fake `adb.exe` for driving the packaged app without a phone (never shipped).
//! The phone lives in `<exe dir>/fake-phone`: `devices.txt` is the `adb devices -l` output,
//! `sdcard/...` mirrors remote files, optional `pull-delay-ms`. Every call is appended to `calls.log`.
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    let phone = std::env::current_exe().unwrap().parent().unwrap().join("fake-phone");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut log = std::fs::OpenOptions::new().create(true).append(true).open(phone.join("calls.log")).unwrap();
    writeln!(log, "{}", serde_json::to_string(&args).unwrap()).unwrap();
    let code = run(&phone, &args);
    std::process::exit(code);
}

fn local(phone: &Path, remote: &str) -> PathBuf {
    assert!(remote.starts_with("/sdcard/") && !remote.contains(".."), "unsafe path {remote}");
    phone.join(remote.trim_start_matches('/'))
}

fn unquote(s: &str) -> String {
    s.trim().trim_start_matches('\'').trim_end_matches('\'').replace("'\"'\"'", "'")
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.path().is_dir() {
            files(&e.path(), out)
        } else {
            out.push(e.path())
        }
    }
}

fn remote_of(phone: &Path, path: &Path) -> String {
    format!("/{}", path.strip_prefix(phone).unwrap().to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
}

fn run(phone: &Path, args: &[String]) -> i32 {
    let devices = std::fs::read_to_string(phone.join("devices.txt")).unwrap_or_else(|_| "List of devices attached\n".into());
    match args.first().map(String::as_str) {
        Some("version") => {
            println!("Android Debug Bridge version 1.0.41 (fake phone for tests)");
            return 0;
        }
        Some("devices") => {
            print!("{devices}");
            return 0;
        }
        Some("-s") if devices.contains(&format!("{}\tdevice", args[1])) => {}
        _ => {
            eprintln!("adb: device not found");
            return 1;
        }
    }
    match args[2].as_str() {
        "pull" => {
            if let Ok(ms) = std::fs::read_to_string(phone.join("pull-delay-ms")) {
                std::thread::sleep(std::time::Duration::from_millis(ms.trim().parse().unwrap_or(0)));
            }
            match std::fs::copy(local(phone, &args[3]), &args[4]) {
                Ok(_) => {
                    println!("1 file pulled");
                    0
                }
                Err(_) => {
                    eprintln!("adb: error: remote object does not exist");
                    1
                }
            }
        }
        "shell" => {
            let c = &args[3];
            // An app counts as installed when its data folder exists on the fake phone.
            if let Some(package) = c.strip_prefix("pm list packages --user 0 ") {
                if phone.join("sdcard/Android/data").join(package).is_dir() {
                    println!("package:{package}");
                }
                return 0;
            }
            if let Some(rest) = c.strip_prefix("find ") {
                let (target, entries) = match rest.strip_suffix(" -type f -name entry.json") {
                    Some(r) => (r, true),
                    None => (rest.strip_suffix(" -type f").unwrap(), false),
                };
                let mut all = Vec::new();
                files(&local(phone, unquote(target).trim_end_matches('/')), &mut all);
                for f in all.iter().filter(|f| !entries || f.ends_with("entry.json")) {
                    println!("{}", remote_of(phone, f))
                }
                return 0;
            }
            if let Some(rest) = c.strip_prefix("stat -c %s ") {
                return match std::fs::metadata(local(phone, &unquote(rest))) {
                    Ok(m) => {
                        println!("{}", m.len());
                        0
                    }
                    Err(_) => 1,
                };
            }
            if let Some(rest) = c.strip_prefix("cat ") {
                return match std::fs::read(local(phone, &unquote(rest))) {
                    Ok(b) => {
                        std::io::stdout().write_all(&b).unwrap();
                        0
                    }
                    Err(_) => 1,
                };
            }
            eprintln!("fake phone refuses command: {c}");
            2
        }
        _ => 1,
    }
}
