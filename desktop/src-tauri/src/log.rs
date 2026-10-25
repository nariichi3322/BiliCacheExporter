use regex::Regex;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

type Sink = Box<dyn Fn(&str) + Send + Sync>;

/// Session log in the portable `log` folder. Raw tool output is parsed elsewhere; only what is written here is redacted.
pub struct Logger {
    file: Mutex<Option<File>>,
    pub path: Option<PathBuf>,
    sink: Mutex<Option<Sink>>,
    memory: Option<Mutex<Vec<String>>>,
}

impl Logger {
    pub fn open(dir: &Path) -> std::io::Result<Arc<Logger>> {
        let folder = dir.join("log");
        std::fs::create_dir_all(&folder)?;
        let stamp = chrono_stamp();
        let path = folder.join(format!("session-{}-{}.log", stamp, &uuid::Uuid::new_v4().simple().to_string()[..8]));
        let file = OpenOptions::new().create_new(true).append(true).open(&path)?;
        Ok(Arc::new(Logger { file: Mutex::new(Some(file)), path: Some(path), sink: Mutex::new(None), memory: None }))
    }

    /// In-memory logger for tests and for running when the portable folder is not writable.
    pub fn memory() -> Arc<Logger> {
        Arc::new(Logger { file: Mutex::new(None), path: None, sink: Mutex::new(None), memory: Some(Mutex::new(Vec::new())) })
    }

    pub fn set_sink(&self, sink: Sink) {
        *self.sink.lock().unwrap() = Some(sink);
    }

    pub fn lines(&self) -> Vec<String> {
        self.memory.as_ref().map(|m| m.lock().unwrap().clone()).unwrap_or_default()
    }

    pub fn log(&self, message: &str) {
        let line = format!("[{}] {}", time_of_day(), redact(message));
        if let Some(file) = self.file.lock().unwrap().as_mut() {
            // A failed log write must not abort the user's operation; the UI still receives the line.
            let _ = writeln!(file, "{line}");
        }
        if let Some(memory) = &self.memory {
            memory.lock().unwrap().push(line.clone());
        }
        if let Some(sink) = self.sink.lock().unwrap().as_ref() {
            sink(&line);
        }
    }
}

fn now_local() -> (u64, i64) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    // shortcut: fixed Asia/Taipei offset for log names; use the OS time zone if users outside UTC+8 need local stamps.
    (secs, 8 * 3600)
}

fn time_of_day() -> String {
    let (ms, offset) = now_local();
    let s = (ms / 1000) as i64 + offset;
    format!("{:02}:{:02}:{:02}", (s / 3600) % 24, (s / 60) % 60, s % 60)
}

fn chrono_stamp() -> String {
    let (ms, offset) = now_local();
    let secs = (ms / 1000) as i64 + offset;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil-from-days (Howard Hinnant) to avoid a date crate for one file name.
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}-{:03}", rem / 3600, rem / 60 % 60, rem % 60, ms % 1000)
}

struct Patterns {
    url: Regex,
    serial_arg: Regex,
    device_line: Regex,
    profile: Option<Regex>,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| {
        let profile = std::env::var("USERPROFILE").ok().filter(|p| !p.is_empty()).map(|p| {
            let variants = [p.clone(), p.replace('\\', "\\\\"), p.replace('\\', "/")];
            Regex::new(&format!("(?i){}", variants.iter().map(|v| regex::escape(v)).collect::<Vec<_>>().join("|"))).unwrap()
        });
        Patterns {
            url: Regex::new(r#"(?i)(https?:[^\s"<>?]+)\?[^\s"<>]*"#).unwrap(),
            serial_arg: Regex::new(r#"("-s"\s+)"[^"]*""#).unwrap(),
            device_line: Regex::new(r"(?m)^\S+(\s+(?:device|offline|unauthorized)\b)").unwrap(),
            profile,
        }
    })
}

/// Same rules as the C# DiagnosticLog: drop metadata JSON, URL queries, device serials and the user profile path.
pub fn redact(text: &str) -> String {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return "[metadata omitted]".into();
    }
    let p = patterns();
    let text = p.url.replace_all(text, "$1?[redacted]");
    let text = p.serial_arg.replace_all(&text, "$1\"[device]\"");
    let text = p.device_line.replace_all(&text, "[device]$1");
    match &p.profile {
        Some(profile) => profile.replace_all(&text, "[user]").into_owned(),
        None => text.into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn redacts_sensitive_values() {
        assert_eq!(redact("{\"url\":\"x\"}"), "[metadata omitted]");
        assert_eq!(redact("get https://a.b/c.m4s?sign=1&t=2 ok"), "get https://a.b/c.m4s?[redacted] ok");
        assert_eq!(redact(r#"> adb "-s" "R58N123" "shell""#), r#"> adb "-s" "[device]" "shell""#);
        assert_eq!(redact("R58N123\tdevice product:x"), "[device]\tdevice product:x");
        let profile = std::env::var("USERPROFILE").unwrap();
        assert_eq!(redact(&format!("{}\\Videos\\a.mp4", profile.to_uppercase())), "[user]\\Videos\\a.mp4");
        assert_eq!(redact(&format!("\"{}\"", profile.replace('\\', "\\\\"))), "\"[user]\"");
    }
}
