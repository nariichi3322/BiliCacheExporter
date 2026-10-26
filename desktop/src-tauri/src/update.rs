use crate::tools::Fetch;
use crate::{fail, Cancel, Error, Res};
use serde::Serialize;
use std::path::Path;

const REPO: &str = "nariichi3322/BiliCacheExporter";
pub const PROJECT_URL: &str = "https://github.com/nariichi3322/BiliCacheExporter";
pub const RELEASES_URL: &str = "https://github.com/nariichi3322/BiliCacheExporter/releases/latest";
/// Test-only override of the GitHub hosts, used by the packaged end-to-end check.
pub const SOURCE_ENV: &str = "BILI_UPDATE_SOURCE";

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub newer: bool,
}

/// GitHub API origin; an override is honoured only for a local test server.
pub fn api_origin(source: Option<&str>) -> String {
    match source {
        Some(s) if s == "http://127.0.0.1" || s.starts_with("http://127.0.0.1:") => s.trim_end_matches('/').into(),
        _ => "https://api.github.com".into(),
    }
}

fn source() -> String {
    api_origin(std::env::var(SOURCE_ENV).ok().as_deref())
}

/// `v2.1.0` or `2.1.0+abc` → (2, 1, 0); anything else is not a release version.
pub fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.trim().trim_start_matches('v').split('+').next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

/// Reads the latest published release (GitHub already excludes drafts and pre-releases).
pub fn check(fetch: &Fetch, current: &str) -> Res<Release> {
    let url = format!("{}/repos/{REPO}/releases/latest", source());
    let dest = std::env::temp_dir().join(format!("bili-release-{}.json", uuid::Uuid::new_v4().simple()));
    let result = fetch(&url, &dest, 1 << 20, &Cancel::default(), &mut |_| {}).and_then(|()| read_release(&dest, current));
    let _ = std::fs::remove_file(&dest);
    result.map_err(|e| Error::Failed(format!("檢查更新失敗：{e}")))
}

fn read_release(path: &Path, current: &str) -> Res<Release> {
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?).map_err(|e| Error::Failed(format!("回應無法解析：{e}")))?;
    let tag = json["tag_name"].as_str().unwrap_or_default();
    let (Some(latest), Some(mine)) = (parse(tag), parse(current)) else { return fail(format!("無法辨識版本號：{tag}")) };
    Ok(Release {
        version: format!("{}.{}.{}", latest.0, latest.1, latest.2),
        notes: json["body"].as_str().unwrap_or_default().replace("\r\n", "\n"),
        newer: latest > mine,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically_and_ignore_build_info() {
        assert_eq!(parse("v2.10.0"), Some((2, 10, 0)));
        assert_eq!(parse("2.0.2+4d54bd7"), Some((2, 0, 2)));
        assert!(parse("v2.10.0") > parse("2.9.9+x"));
        assert_eq!(parse("v2.1"), None);
        assert_eq!(parse("v2.1.0-beta"), None);
    }

    #[test]
    fn override_accepts_only_the_local_test_server() {
        assert_eq!(api_origin(None), "https://api.github.com");
        assert_eq!(api_origin(Some("http://127.0.0.1:8123/")), "http://127.0.0.1:8123");
        for bad in ["https://evil.example", "http://127.0.0.1.evil.example", "http://localhost:80", "http://127.0.0.10:1"] {
            assert_eq!(api_origin(Some(bad)), "https://api.github.com", "{bad}");
        }
    }
}
