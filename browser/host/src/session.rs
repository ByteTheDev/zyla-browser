//! Saves open tabs, Spaces and sidebar state so the browser comes back the way it was left.

use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};

fn one() -> f64 {
    1.0
}

fn first_space() -> u32 {
    1
}

fn sidebar_visible() -> bool {
    true
}

fn sidebar_width() -> f64 {
    260.0
}

fn sidebar_side() -> String { "left".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTab {
    pub url: String,
    #[serde(default = "one")]
    pub zoom: f64,
    #[serde(default = "first_space")]
    pub space: u32,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSpace {
    pub id: u32,
    pub name: String,
    pub tint: String,
    /// Index into the saved tabs of the tab that was active in this Space.
    #[serde(default)]
    pub last_active: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub tabs: Vec<SavedTab>,
    #[serde(default)]
    pub active: usize,
    #[serde(default)]
    pub spaces: Vec<SavedSpace>,
    #[serde(default = "first_space")]
    pub active_space: u32,
    #[serde(default = "sidebar_visible")]
    pub sidebar_visible: bool,
    #[serde(default = "sidebar_width")]
    pub sidebar_width: f64,
    #[serde(default = "sidebar_side")]
    pub sidebar_side: String,
    /// Tabs in a strip above the toolbar (Chrome and Firefox style) instead of in the sidebar.
    #[serde(default)]
    pub tabs_on_top: bool,
}

/// Per-user folder for the session file and the WebView2 profile (cookies, bookmarks, history).
pub fn data_dir() -> PathBuf {
    let root = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    root.join("TSGPUI Browser")
}

pub fn load(path: &Path) -> Option<Session> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Writes through a temporary file so a crash mid-write cannot leave a truncated session.
pub fn save(path: &Path, session: &Session) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(session).map_err(std::io::Error::other)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, text)?;
    fs::rename(&temp, path)
}
