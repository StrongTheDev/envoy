//! Files Envoy keeps under `%APPDATA%\Envoy`: settings and backups.

use crate::model::EnvSet;
use chrono::{DateTime, Datelike, Local};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Envoy")
}

// ───────────────────────────── settings ─────────────────────────────

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub dark: Option<bool>,
    /// Lower-cased name → shown as a list (true) or as text (false), overriding the guess.
    #[serde(default)]
    pub list: BTreeMap<String, bool>,
    /// Lower-cased name → masked (true) or not (false), overriding the guess.
    #[serde(default)]
    pub secret: BTreeMap<String, bool>,
}

impl Settings {
    fn path() -> PathBuf {
        data_dir().join("settings.json")
    }

    pub fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(data_dir());
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(Self::path(), json);
        }
    }
}

// ───────────────────────────── backups ─────────────────────────────

const KEEP_AUTO: usize = 40;

#[derive(Clone, Serialize, Deserialize)]
pub struct Backup {
    pub created: DateTime<Local>,
    pub note: String,
    /// "auto" or "manual"
    pub kind: String,
    pub env: EnvSet,
}

#[derive(Clone)]
pub struct BackupEntry {
    pub path: PathBuf,
    pub backup: Backup,
}

fn backup_dir() -> PathBuf {
    data_dir().join("backups")
}

pub fn save_backup(env: &EnvSet, note: &str, kind: &str) -> io::Result<PathBuf> {
    let dir = backup_dir();
    std::fs::create_dir_all(&dir)?;
    let now = Local::now();
    let backup = Backup { created: now, note: note.to_owned(), kind: kind.to_owned(), env: env.clone() };
    let path = dir.join(format!("{}-{kind}.json", now.format("%Y%m%d-%H%M%S%3f")));
    std::fs::write(&path, serde_json::to_vec_pretty(&backup)?)?;
    if kind == "auto" {
        prune();
    }
    Ok(path)
}

/// Newest first.
pub fn list_backups() -> Vec<BackupEntry> {
    let Ok(dir) = std::fs::read_dir(backup_dir()) else { return Vec::new() };
    let mut out: Vec<BackupEntry> = dir
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let backup = serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()?;
            Some(BackupEntry { path: e.path(), backup })
        })
        .collect();
    out.sort_by(|a, b| b.backup.created.cmp(&a.backup.created));
    out
}

pub fn delete_backup(entry: &BackupEntry) -> io::Result<()> {
    std::fs::remove_file(&entry.path)
}

fn prune() {
    let autos: Vec<BackupEntry> = list_backups().into_iter().filter(|b| b.backup.kind == "auto").collect();
    for old in autos.iter().skip(KEEP_AUTO) {
        let _ = delete_backup(old);
    }
}

/// "Today, 09:41" · "Yesterday, 17:12" · "Oct 14, 08:30"
pub fn friendly_time(t: &DateTime<Local>) -> String {
    let today = Local::now().date_naive();
    let day = t.date_naive();
    if day == today {
        format!("Today, {}", t.format("%H:%M"))
    } else if Some(day) == today.pred_opt() {
        format!("Yesterday, {}", t.format("%H:%M"))
    } else if day.year() == today.year() {
        t.format("%b %d, %H:%M").to_string()
    } else {
        t.format("%b %d %Y, %H:%M").to_string()
    }
}
