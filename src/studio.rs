use crate::store;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Studio pairing. The app shows a short code, the plugin trades it once for
/// a token, and every plugin request after that carries the token.
const CODE_LIFETIME: Duration = Duration::from_secs(300);
const MAX_WRONG: u32 = 5;

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    tokens: Vec<String>,
}

#[derive(Default)]
pub struct Pairing {
    code: Option<(String, Instant)>,
    wrong: u32,
    tokens: Vec<String>,
    pub last_seen: Option<Instant>,
    pub place: String,
}

pub type Shared = Arc<Mutex<Pairing>>;

pub fn lock(p: &Shared) -> std::sync::MutexGuard<'_, Pairing> {
    p.lock().unwrap_or_else(|e| e.into_inner())
}

fn saved_path() -> std::path::PathBuf {
    store::config_dir().join("studio.json")
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    let _ = getrandom::getrandom(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn load() -> Shared {
    let saved: Saved = std::fs::read_to_string(saved_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Arc::new(Mutex::new(Pairing { tokens: saved.tokens, ..Default::default() }))
}

impl Pairing {
    fn save(&self) {
        let saved = Saved { tokens: self.tokens.clone() };
        if let Ok(text) = serde_json::to_string(&saved) {
            let _ = std::fs::write(saved_path(), text);
        }
    }

    pub fn new_code(&mut self) -> String {
        let mut buf = [0u8; 4];
        let _ = getrandom::getrandom(&mut buf);
        let code = format!("{:06}", u32::from_le_bytes(buf) % 1_000_000);
        self.code = Some((code.clone(), Instant::now()));
        self.wrong = 0;
        code
    }

    pub fn pending_code(&self) -> Option<(String, u64)> {
        let (code, at) = self.code.as_ref()?;
        let left = CODE_LIFETIME.checked_sub(at.elapsed())?;
        Some((code.clone(), left.as_secs()))
    }

    /// Trades a code for a token. Too many wrong guesses burn the code.
    pub fn redeem(&mut self, code: &str) -> Option<String> {
        let (expected, _) = self.pending_code()?;
        if code.trim() != expected {
            self.wrong += 1;
            if self.wrong >= MAX_WRONG {
                self.code = None;
            }
            return None;
        }
        self.code = None;
        let token = random_hex(24);
        self.tokens.push(token.clone());
        self.last_seen = Some(Instant::now());
        self.save();
        Some(token)
    }

    pub fn check(&mut self, token: Option<&str>) -> bool {
        let ok = token.is_some_and(|t| !t.is_empty() && self.tokens.iter().any(|x| x == t));
        if ok {
            self.last_seen = Some(Instant::now());
        }
        ok
    }

    pub fn connected(&self) -> bool {
        !self.tokens.is_empty()
    }

    /// Forgets one Studio, the others stay paired.
    pub fn revoke(&mut self, token: &str) {
        self.tokens.retain(|t| t != token);
        if self.tokens.is_empty() {
            self.last_seen = None;
            self.place.clear();
        }
        self.save();
    }

    pub fn disconnect(&mut self) {
        self.tokens.clear();
        self.code = None;
        self.last_seen = None;
        self.place.clear();
        self.save();
    }
}
