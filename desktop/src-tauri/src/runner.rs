use crate::log::Logger;
use crate::{Cancel, Error, Res};
use std::io::{BufReader, Read};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Test seam: return `Some` to replace a call (e.g. the fake phone), `None` to run the real executable.
pub type Fake = dyn Fn(&str, &[String], &Cancel, &mut dyn FnMut(&str)) -> Option<Res<String>> + Send + Sync;

#[derive(Clone)]
pub struct Runner {
    pub log: Arc<Logger>,
    fake: Option<Arc<Fake>>,
}

impl Runner {
    pub fn new(log: Arc<Logger>) -> Self {
        Runner { log, fake: None }
    }

    pub fn with_fake(log: Arc<Logger>, fake: Arc<Fake>) -> Self {
        Runner { log, fake: Some(fake) }
    }

    pub fn log(&self, message: &str) {
        self.log.log(message)
    }

    /// Runs one executable with structured arguments (never through a shell) and returns stdout.
    pub fn run(&self, exe: &str, args: &[String], cancel: &Cancel, progress: &mut dyn FnMut(&str), timeout_secs: u64) -> Res<String> {
        cancel.check()?;
        let quoted: Vec<String> = args.iter().map(|a| serde_json::to_string(a).unwrap()).collect();
        self.log(&format!("> {} {}", exe, quoted.join(" ")));
        if let Some(fake) = &self.fake {
            if let Some(result) = fake(exe, args, cancel, progress) {
                return result;
            }
        }
        let name = Path::new(exe).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| exe.to_string());
        let mut child = Command::new(exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| Error::Failed(format!("無法啟動 {name}：{e}")))?;
        let (tx, rx) = mpsc::channel::<(bool, String)>();
        spawn_reader(child.stdout.take().unwrap(), true, tx.clone());
        spawn_reader(child.stderr.take().unwrap(), false, tx);
        // Metadata reads (`cat`) stay out of the log; redaction would drop them anyway.
        let quiet_stdout = args.iter().any(|a| a.starts_with("cat "));
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let mut output = String::new();
        let mut exited_at: Option<Instant> = None;
        let mut outcome: Option<Error> = None;
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok((stdout, line)) => {
                    if !(stdout && quiet_stdout) {
                        self.log(&line);
                    }
                    if stdout {
                        output.push_str(&line);
                        output.push('\n');
                    }
                    progress(&line);
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if exited_at.is_none() && child.try_wait()?.is_some() {
                exited_at = Some(Instant::now());
            }
            // A detached grandchild (e.g. the adb server) may keep a pipe open; do not wait on it forever.
            if exited_at.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
                break;
            }
            if exited_at.is_none() && (cancel.is_cancelled() || Instant::now() > deadline) {
                outcome = Some(if cancel.is_cancelled() {
                    Error::Cancelled
                } else {
                    Error::Timeout(format!("{name} 超時（{timeout_secs} 秒），程序已停止。請查看日誌確認失敗階段。"))
                });
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
        if let Some(error) = outcome {
            return Err(error);
        }
        let status = child.wait()?;
        cancel.check()?;
        if !status.success() {
            let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "?".into());
            return Err(Error::Failed(format!("{name} 執行失敗（代碼 {code}）。請查看完整日誌；可能是連線、格式、缺檔或加密問題。不會嘗試繞過 DRM。")));
        }
        Ok(output)
    }
}

fn spawn_reader(stream: impl Read + Send + 'static, stdout: bool, tx: mpsc::Sender<(bool, String)>) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut buffer = Vec::new();
        // FFmpeg progress uses `\n`; ADB pull progress may use `\r`, so split on both.
        loop {
            buffer.clear();
            let mut byte = [0u8; 1];
            let mut got = false;
            loop {
                match reader.read(&mut byte) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        got = true;
                        if byte[0] == b'\n' || byte[0] == b'\r' {
                            break;
                        }
                        buffer.push(byte[0]);
                    }
                }
            }
            if !got {
                return;
            }
            if buffer.is_empty() {
                continue;
            }
            if tx.send((stdout, String::from_utf8_lossy(&buffer).into_owned())).is_err() {
                return;
            }
        }
    });
}

pub fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}
