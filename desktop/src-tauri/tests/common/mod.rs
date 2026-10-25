//! Fake phone (ADB double) plus real FFmpeg fixtures shared by the flow tests.
#![allow(dead_code)]

use bili_core::app::{App, Event};
use bili_core::log::Logger;
use bili_core::runner::{strings, Runner};
use bili_core::scan::ROOT;
use bili_core::tools::Fetch;
use bili_core::{Cancel, Error, Res};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub fn ffmpeg() -> PathBuf {
    let path = std::env::var("BILI_TEST_FFMPEG").expect("set BILI_TEST_FFMPEG to a real ffmpeg.exe (ffprobe.exe beside it)");
    PathBuf::from(path)
}

pub fn ffprobe() -> PathBuf {
    ffmpeg().with_file_name("ffprobe.exe")
}

pub fn real(args: &[&str]) -> String {
    Runner::new(Logger::memory()).run(&ffmpeg().to_string_lossy(), &strings(args), &Cancel::default(), &mut |_| {}, 120).expect("ffmpeg fixture")
}

pub fn probe(path: &Path) -> serde_json::Value {
    let out = Runner::new(Logger::memory())
        .run(
            &ffprobe().to_string_lossy(),
            &strings(&["-v", "error", "-show_streams", "-of", "json", &path.to_string_lossy()]),
            &Cancel::default(),
            &mut |_| {},
            60,
        )
        .unwrap();
    serde_json::from_str(&out).unwrap()
}

pub struct Media {
    pub video: Vec<u8>,
    pub audio: Vec<u8>,
    pub cover: Vec<u8>,
    pub cover_only: Vec<u8>,
    pub damaged: Vec<u8>,
}

/// Real H.264/AAC fragmented sources and a JPEG cover, generated once with the real ffmpeg.
pub fn media() -> &'static Media {
    static M: OnceLock<Media> = OnceLock::new();
    M.get_or_init(|| {
        let dir = tempdir();
        let p = |n: &str| dir.join(n).to_string_lossy().into_owned();
        real(&[
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=160x90:rate=10",
            "-t",
            "1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+frag_keyframe+empty_moov",
            "-f",
            "mp4",
            &p("video.m4s"),
        ]);
        real(&[
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "1",
            "-c:a",
            "aac",
            "-movflags",
            "+frag_keyframe+empty_moov",
            "-f",
            "mp4",
            &p("audio.m4s"),
        ]);
        real(&["-hide_banner", "-nostdin", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=320x180", "-frames:v", "1", &p("cover.jpg")]);
        real(&[
            "-hide_banner",
            "-nostdin",
            "-v",
            "error",
            "-i",
            &p("cover.jpg"),
            "-i",
            &p("audio.m4s"),
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c",
            "copy",
            "-disposition:v:0",
            "attached_pic",
            "-f",
            "mp4",
            &p("cover-only.m4s"),
        ]);
        let video = std::fs::read(p("video.m4s")).unwrap();
        // Zero the media payload but keep headers: still probeable as H.264, not decodable.
        let mut damaged = video.clone();
        let mdat = damaged.windows(4).position(|w| w == b"mdat").expect("mdat box");
        let size = u32::from_be_bytes(damaged[mdat - 4..mdat].try_into().unwrap()) as usize;
        damaged[mdat + 4..mdat - 4 + size].fill(0);
        Media {
            video,
            audio: std::fs::read(p("audio.m4s")).unwrap(),
            cover: std::fs::read(p("cover.jpg")).unwrap(),
            cover_only: std::fs::read(p("cover-only.m4s")).unwrap(),
            damaged,
        }
    })
}

pub fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bili-test-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[derive(Default)]
pub struct Phone {
    pub devices: String,
    pub package: bool,
    pub files: BTreeMap<String, Vec<u8>>,
    pub extra_entries: Vec<String>,
    pub disconnected: bool,
    pub devices_timeout: bool,
    pub fail_pull: Option<String>,
    pub cancel_pull: Option<String>,
    pub disconnect_after_pull: Option<String>,
    pub fail_tool: bool,
    pub calls: Vec<Vec<String>>,
    pub pulls: Vec<String>,
}

pub const SERIAL: &str = "PHONE-SERIAL-1";

impl Phone {
    pub fn one_device() -> Phone {
        Phone { devices: format!("List of devices attached\n{SERIAL}\tdevice product:x model:SM_S9260 device:y\n"), package: true, ..Default::default() }
    }

    /// Adds one part with the given quality directories; each holds video/audio (and index.json when `meta`).
    pub fn add(&mut self, id: &str, title: &str, qualities: &[&str], cover: bool) -> String {
        let remote = format!("{ROOT}{id}");
        let m = media();
        self.files.insert(format!("{remote}/entry.json"), format!(r#"{{"title":"{title}","is_completed":true,"page_data":{{"part":"第一集"}}}}"#).into_bytes());
        for q in qualities {
            self.files.insert(format!("{remote}/{q}/video.m4s"), m.video.clone());
            self.files.insert(format!("{remote}/{q}/audio.m4s"), m.audio.clone());
            self.files.insert(format!("{remote}/{q}/index.json"), br#"{"video":[{"width":160,"height":90,"codecs":"avc1"}]}"#.to_vec());
        }
        if cover {
            self.files.insert(format!("{remote}/cover.jpg"), m.cover.clone());
        }
        remote
    }

    pub fn device_queries(&self) -> usize {
        self.calls.iter().filter(|c| c.first().map(String::as_str) == Some("devices")).count()
    }

    fn shell(&mut self, command: &str) -> Res<String> {
        let unquote = |s: &str| s.trim().strip_prefix('\'').and_then(|x| x.strip_suffix('\'')).expect("quoted argument").replace("'\"'\"'", "'");
        if let Some(rest) = command.strip_prefix("pm list packages --user 0 ") {
            assert_eq!(rest, "com.bilibili.app.in");
            return Ok(if self.package { "package:com.bilibili.app.in\n".into() } else { String::new() });
        }
        if let Some(rest) = command.strip_prefix("find ") {
            if let Some(root) = rest.strip_suffix(" -type f -name entry.json") {
                assert_eq!(unquote(root), ROOT);
                let mut lines: Vec<String> = self.files.keys().filter(|k| k.ends_with("/entry.json")).cloned().collect();
                lines.extend(self.extra_entries.clone());
                return Ok(lines.join("\n"));
            }
            let dir = unquote(rest.strip_suffix(" -type f").expect("find form"));
            return Ok(self.files.keys().filter(|k| k.starts_with(&format!("{dir}/"))).cloned().collect::<Vec<_>>().join("\n"));
        }
        if let Some(rest) = command.strip_prefix("stat -c %s ") {
            return self.files.get(&unquote(rest)).map(|f| f.len().to_string()).ok_or_else(|| Error::Failed("stat: No such file".into()));
        }
        if let Some(rest) = command.strip_prefix("cat ") {
            return self.files.get(&unquote(rest)).map(|f| String::from_utf8_lossy(f).into_owned()).ok_or_else(|| Error::Failed("cat: No such file".into()));
        }
        panic!("unexpected (possibly writing) phone command: {command}");
    }

    pub fn handle(&mut self, args: &[String], cancel: &Cancel) -> Res<String> {
        self.calls.push(args.to_vec());
        match args[0].as_str() {
            "version" => return if self.fail_tool { Err(Error::Failed("not runnable".into())) } else { Ok("Android Debug Bridge version 1.0.41\n".into()) },
            "devices" => {
                assert_eq!(args, ["devices", "-l"]);
                return if self.devices_timeout {
                    Err(Error::Timeout("adb.exe 超時（15 秒），程序已停止。".into()))
                } else if self.disconnected {
                    Ok("List of devices attached
"
                    .into())
                } else {
                    Ok(self.devices.clone())
                };
            }
            "-s" => {}
            other => panic!("unexpected adb command {other}"),
        }
        if self.disconnected || !self.devices.contains(&format!("{}\tdevice", args[1])) {
            return Err(Error::Failed("adb: device not found".into()));
        }
        match args[2].as_str() {
            "shell" => {
                assert_eq!(args.len(), 4);
                self.shell(&args[3])
            }
            "pull" => {
                let (remote, local) = (&args[3], &args[4]);
                if self.fail_pull.as_ref().is_some_and(|f| remote.contains(f.as_str())) {
                    return Err(Error::Failed("adb: error: device offline".into()));
                }
                let data = self.files.get(remote).cloned().ok_or_else(|| Error::Failed("adb: error: remote object does not exist".into()))?;
                std::fs::write(local, data).unwrap();
                self.pulls.push(remote.clone());
                if self.disconnect_after_pull.as_ref().is_some_and(|f| remote.contains(f.as_str())) {
                    self.disconnected = true;
                }
                if self.cancel_pull.as_ref().is_some_and(|f| remote.contains(f.as_str())) {
                    cancel.cancel();
                    return Err(Error::Cancelled);
                }
                Ok("1 file pulled".into())
            }
            other => panic!("unexpected adb subcommand {other}"),
        }
    }
}

pub type Shared = Arc<Mutex<Phone>>;

pub fn runner(phone: &Shared, log: Arc<Logger>) -> Runner {
    let phone = phone.clone();
    Runner::with_fake(
        log,
        Arc::new(move |exe: &str, args: &[String], cancel: &Cancel, _: &mut dyn FnMut(&str)| {
            exe.ends_with("fake-adb.exe").then(|| phone.lock().unwrap().handle(args, cancel))
        }),
    )
}

pub struct Harness {
    pub dir: PathBuf,
    pub app: App,
    pub phone: Shared,
    pub log: Arc<Logger>,
    pub events: Arc<Mutex<Vec<Event>>>,
}

pub fn no_network() -> Box<Fetch> {
    Box::new(|_: &str, _: &Path, _: u64, _: &Cancel, _: &mut dyn FnMut(u64)| panic!("tests must not use the network"))
}

pub fn write_settings(dir: &Path) {
    std::fs::write(dir.join("fake-adb.exe"), b"fake").unwrap();
    let settings = serde_json::json!({
        "Adb": "fake-adb.exe",
        "Ffmpeg": ffmpeg(),
        "Ffprobe": ffprobe(),
        "Output": dir.join("output"),
        "Temp": "",
        "Theme": "dark",
        "SettingsVersion": 2
    });
    std::fs::write(dir.join("settings.json"), settings.to_string()).unwrap();
}

pub fn harness_in(dir: PathBuf, phone: Phone, fetch: Box<Fetch>) -> Harness {
    let phone = Arc::new(Mutex::new(phone));
    let log = Logger::memory();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let app = App::new(dir.clone(), runner(&phone, log.clone()), fetch, Arc::new(move |e| sink.lock().unwrap().push(e)), None);
    Harness { dir, app, phone, log, events }
}

pub fn harness(phone: Phone) -> Harness {
    let dir = tempdir();
    write_settings(&dir);
    harness_in(dir, phone, no_network())
}

pub fn mp4s(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(dir).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    names.sort();
    names
}
