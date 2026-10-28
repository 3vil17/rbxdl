use crate::roblox::Roblox;
use crate::store;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Roblox publishes no per-cookie number for assetdelivery, so each account is
/// paced a little and backs off on 429 (Retry-After when sent, else doubling).
const MIN_GAP: Duration = Duration::from_millis(350);
const MAX_WAIT: Duration = Duration::from_secs(180);

struct Account {
    cookie: String,
    user_id: Option<i64>,
    username: Option<String>,
    alive: bool,
    uses: u64,
    limited: u64,
    backoff: u64,
    cooldown_until: Option<Instant>,
    last_use: Option<Instant>,
    last_error: Option<String>,
}

impl Account {
    fn label(&self) -> String {
        match &self.username {
            Some(u) => format!("@{u}"),
            None => "unknown account".into(),
        }
    }

    fn ready_at(&self, now: Instant) -> Instant {
        let gap = self.last_use.map_or(now, |t| t + MIN_GAP);
        let cool = self.cooldown_until.unwrap_or(now);
        gap.max(cool).max(now)
    }
}

#[derive(Default)]
struct State {
    accounts: Vec<Account>,
    cursor: usize,
}

pub struct Lease {
    pub cookie: String,
    pub label: String,
}

#[derive(Clone, Default)]
pub struct Pool {
    inner: Arc<Mutex<State>>,
}

impl Pool {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Re-read saved cookies and check who each one belongs to. Usage stats
    /// carry over for cookies that were already loaded.
    pub async fn reload(&self, rb: &Roblox) {
        let cookies = store::load_cookies();
        let mut fresh = Vec::with_capacity(cookies.len());
        for cookie in cookies {
            let info = rb.cookie_info(&cookie).await;
            let (uses, limited) = {
                let st = self.lock();
                st.accounts
                    .iter()
                    .find(|a| a.cookie == cookie)
                    .map_or((0, 0), |a| (a.uses, a.limited))
            };
            fresh.push(Account {
                cookie,
                user_id: info.user_id,
                username: info.username,
                alive: info.alive,
                uses,
                limited,
                backoff: 0,
                cooldown_until: None,
                last_use: None,
                last_error: if info.alive { None } else { Some("cookie is expired or invalid".into()) },
            });
        }
        let mut st = self.lock();
        st.accounts = fresh;
        st.cursor = 0;
    }

    pub fn total_count(&self) -> usize {
        self.lock().accounts.len()
    }

    pub fn alive_count(&self) -> usize {
        self.lock().accounts.iter().filter(|a| a.alive).count()
    }

    /// Next ready account in round-robin order, skipping `exclude`. Waits while
    /// every candidate is cooling down. None when there is nothing left to try.
    pub async fn acquire(&self, exclude: &[String]) -> Option<Lease> {
        let start = Instant::now();
        loop {
            let wait = {
                let mut st = self.lock();
                let now = Instant::now();
                let n = st.accounts.len();
                let mut soonest: Option<Instant> = None;
                let mut pick = None;
                for k in 0..n {
                    let i = (st.cursor + k) % n;
                    let a = &st.accounts[i];
                    if !a.alive || exclude.contains(&a.cookie) {
                        continue;
                    }
                    let ready = a.ready_at(now);
                    if ready <= now {
                        pick = Some(i);
                        break;
                    }
                    soonest = Some(soonest.map_or(ready, |s| s.min(ready)));
                }
                if let Some(i) = pick {
                    st.cursor = i + 1;
                    let a = &mut st.accounts[i];
                    a.last_use = Some(now);
                    a.uses += 1;
                    return Some(Lease { cookie: a.cookie.clone(), label: a.label() });
                }
                soonest? - now
            };
            if start.elapsed() + wait > MAX_WAIT {
                return None;
            }
            tokio::time::sleep(wait).await;
        }
    }

    fn with(&self, cookie: &str, f: impl FnOnce(&mut Account)) {
        let mut st = self.lock();
        if let Some(a) = st.accounts.iter_mut().find(|a| a.cookie == cookie) {
            f(a);
        }
    }

    pub fn report_ok(&self, cookie: &str) {
        self.with(cookie, |a| a.backoff = 0);
    }

    pub fn report_limited(&self, cookie: &str, retry_after: u64) {
        self.with(cookie, |a| {
            a.limited += 1;
            a.backoff = if retry_after > 0 { retry_after } else { (a.backoff * 2).clamp(5, 120) };
            a.cooldown_until = Some(Instant::now() + Duration::from_secs(a.backoff));
            a.last_error = Some(format!("429 rate limited, cooling {}s", a.backoff));
        });
    }

    pub fn report_dead(&self, cookie: &str) {
        self.with(cookie, |a| {
            a.alive = false;
            a.last_error = Some("cookie is expired or invalid".into());
        });
    }

    pub fn snapshot(&self) -> Value {
        let st = self.lock();
        let now = Instant::now();
        let list: Vec<Value> = st
            .accounts
            .iter()
            .map(|a| {
                let cooling = a
                    .cooldown_until
                    .filter(|t| *t > now)
                    .map_or(0, |t| (t - now).as_secs() + 1);
                json!({
                    "username": a.username,
                    "user_id": a.user_id,
                    "alive": a.alive,
                    "uses": a.uses,
                    "limited": a.limited,
                    "cooling": cooling,
                    "last_error": a.last_error,
                })
            })
            .collect();
        json!(list)
    }
}
