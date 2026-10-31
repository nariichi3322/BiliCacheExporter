use crate::export::{export, Request};
use crate::runner::Runner;
use crate::scan::{Adb, CacheItem, Variant};
use crate::storage::Storage;
use crate::{Cancel, Error};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Success,
    Partial,
    Failed,
    Cancelled,
    NotStarted,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ItemResult {
    pub id: usize,
    pub outcome: Outcome,
    pub message: String,
    /// Short reason for the row (cover or failure cause); the full text stays in `message` for the log.
    pub detail: String,
    pub path: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BatchProgress {
    pub id: usize,
    pub index: usize,
    pub total: usize,
    pub completed: usize,
    pub fraction: Option<f64>,
    pub stage: String,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub total: usize,
    pub successful: usize,
    pub partial: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub not_started: usize,
}

pub struct Tools<'a> {
    pub ffmpeg: &'a Path,
    pub ffprobe: &'a Path,
    pub output: &'a str,
    pub temp: &'a str,
    pub full_decode: bool,
}

/// Exports one item at a time; a failure moves on to the next item, cancellation stops the queue.
#[allow(clippy::too_many_arguments)]
pub fn run(
    runner: &Runner,
    adb: &Adb,
    storage: &Storage,
    serial: &str,
    tasks: &[(CacheItem, Variant)],
    tools: &Tools,
    cancel: &Cancel,
    on_progress: &mut dyn FnMut(BatchProgress),
    on_result: &mut dyn FnMut(&ItemResult),
) -> Summary {
    let mut summary = Summary { total: tasks.len(), ..Default::default() };
    for (index, (item, variant)) in tasks.iter().enumerate() {
        if cancel.is_cancelled() {
            for (rest, _) in &tasks[index..] {
                summary.not_started += 1;
                on_result(&ItemResult {
                    id: rest.id, outcome: Outcome::NotStarted, message: "未開始（已取消）".into(), detail: String::new(), path: None
                });
            }
            break;
        }
        let completed = summary.successful + summary.partial + summary.failed;
        let request = Request {
            serial,
            item,
            variant,
            ffmpeg: tools.ffmpeg,
            ffprobe: tools.ffprobe,
            output: tools.output,
            temp: tools.temp,
            full_decode: tools.full_decode,
        };
        let result = export(runner, adb, storage, &request, cancel, &mut |fraction, stage| {
            if !cancel.is_cancelled() {
                on_progress(BatchProgress {
                    id: item.id,
                    index: index + 1,
                    total: tasks.len(),
                    completed,
                    fraction: fraction.filter(|f| f.is_finite()),
                    stage: stage.into(),
                });
            }
        });
        let outcome = match result {
            Ok(r) if r.cover_error.is_some() => {
                summary.partial += 1;
                let reason = r.cover_error.unwrap().trim_end_matches('。').to_string();
                ItemResult {
                    id: item.id,
                    outcome: Outcome::Partial,
                    message: format!("部分完成：{reason}；影片：{}", r.path),
                    detail: reason,
                    path: Some(r.path),
                }
            }
            Ok(r) => {
                summary.successful += 1;
                ItemResult { id: item.id, outcome: Outcome::Success, message: format!("完成：{}", r.path), detail: String::new(), path: Some(r.path) }
            }
            Err(Error::Cancelled) => {
                summary.cancelled += 1;
                ItemResult { id: item.id, outcome: Outcome::Cancelled, message: "已取消".into(), detail: String::new(), path: None }
            }
            Err(e) => {
                summary.failed += 1;
                ItemResult { id: item.id, outcome: Outcome::Failed, message: format!("失敗：{e}"), detail: e.to_string(), path: None }
            }
        };
        runner.log(&format!("{}：{}", item.title, outcome.message));
        on_result(&outcome);
    }
    summary
}
