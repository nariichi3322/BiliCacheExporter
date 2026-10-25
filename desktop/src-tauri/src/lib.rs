//! Rust core for BiliCacheExporter: device access, cache parsing, external tools, storage and export.
//! The Tauri shell (main.rs) only forwards commands to [`app::App`], so tests drive the same entry points.

pub mod app;
pub mod batch;
pub mod export;
pub mod log;
pub mod runner;
pub mod scan;
pub mod settings;
pub mod storage;
pub mod tools;

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "+", env!("BILI_SOURCE"));

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Cancelled,
    Timeout(String),
    Failed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Cancelled => f.write_str("已取消"),
            Error::Timeout(m) | Error::Failed(m) => f.write_str(m),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Failed(e.to_string())
    }
}

pub type Res<T> = Result<T, Error>;

pub fn fail<T>(message: impl Into<String>) -> Res<T> {
    Err(Error::Failed(message.into()))
}

/// Shared cancellation flag checked by long operations and the process runner.
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst)
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn check(&self) -> Res<()> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}
