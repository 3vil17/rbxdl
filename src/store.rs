use crate::error::Result;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const SERVICE: &str = "dev.wares.rbxdl";

fn entry(key: &str) -> Result<Entry> {
    Ok(Entry::new(SERVICE, key)?)
}

pub fn save_secret(key: &str, value: &str) -> Result<()> {
    entry(key)?.set_password(value)?;
    Ok(())
}

pub fn get_secret(key: &str) -> Option<String> {
    entry(key).ok()?.get_password().ok()
}

pub fn clear_secret(key: &str) -> Result<()> {
    let e = entry(key)?;
    match e.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

const MAX_COOKIES: usize = 50;

fn cookie_key(i: usize) -> String {
    if i == 0 { "cookie".into() } else { format!("cookie_{}", i + 1) }
}

/// Cookies live in separate credential entries since each one is near the
/// Credential Manager size cap. The first keeps the old `cookie` name.
pub fn load_cookies() -> Vec<String> {
    let mut out = Vec::new();
    for i in 0..MAX_COOKIES {
        match get_secret(&cookie_key(i)) {
            Some(c) => out.push(c),
            None => break,
        }
    }
    out
}

pub fn save_cookies(list: &[String]) -> Result<()> {
    for i in 0..MAX_COOKIES {
        clear_secret(&cookie_key(i))?;
    }
    for (i, c) in list.iter().take(MAX_COOKIES).enumerate() {
        save_secret(&cookie_key(i), c)?;
    }
    Ok(())
}

static HISTORY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn config_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("rbxdl");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn default_save_dir() -> PathBuf {
    let base = dirs::download_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("rbxyoinker")
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    pub save_dir: String,
    #[serde(default)]
    pub sort_by_type: bool,
    #[serde(default = "yes")]
    pub mesh_obj: bool,
    #[serde(default)]
    pub accepted_terms: bool,
    #[serde(default)]
    pub seen_version: String,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            save_dir: default_save_dir().to_string_lossy().to_string(),
            sort_by_type: false,
            mesh_obj: true,
            accepted_terms: false,
            seen_version: String::new(),
        }
    }
}

pub fn load_settings() -> Settings {
    let path = config_dir().join("settings.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(s: &Settings) -> Result<()> {
    let path = config_dir().join("settings.json");
    std::fs::write(&path, serde_json::to_vec_pretty(s)?)?;
    Ok(())
}

#[derive(Serialize, Deserialize, Clone)]
pub struct HistoryEntry {
    pub id: i64,
    pub name: String,
    pub type_label: String,
    pub path: String,
    pub method: String,
    pub when: String,
}

pub fn load_history() -> Vec<HistoryEntry> {
    let path = config_dir().join("history.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn push_history(entry: HistoryEntry) -> Result<()> {
    let _guard = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = load_history();
    all.insert(0, entry);
    all.truncate(200);
    let path = config_dir().join("history.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&all)?)?;
    Ok(())
}

pub fn clear_history() -> Result<()> {
    let path = config_dir().join("history.json");
    let _ = std::fs::remove_file(path);
    Ok(())
}

pub fn existing_download(id: i64) -> Option<HistoryEntry> {
    load_history()
        .into_iter()
        .find(|h| h.id == id && std::path::Path::new(&h.path).exists())
}
