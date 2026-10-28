use crate::error::{Error, Result};
use crate::pool::Pool;
use crate::roblox::Roblox;

pub struct Attempt {
    pub bytes: Vec<u8>,
    pub method: String,
    pub place_id: Option<i64>,
    pub account: Option<String>,
}

struct Denial {
    error: Error,
    cookie: String,
    label: String,
}

async fn via_cookie(
    rb: &Roblox,
    pool: &Pool,
    id: i64,
    place: Option<i64>,
    exclude: &[String],
) -> std::result::Result<(Vec<u8>, String), Option<Denial>> {
    for _ in 0..8 {
        let lease = pool.acquire(exclude).await.ok_or(None)?;
        match rb.fetch_cookie(&lease.cookie, id, place).await {
            Ok(bytes) => {
                pool.report_ok(&lease.cookie);
                return Ok((bytes, lease.label));
            }
            Err(Error::RateLimited(s)) => pool.report_limited(&lease.cookie, s),
            Err(e) => {
                if e.to_string().starts_with("401") {
                    pool.report_dead(&lease.cookie);
                } else {
                    pool.report_ok(&lease.cookie);
                }
                return Err(Some(Denial { error: e, cookie: lease.cookie, label: lease.label }));
            }
        }
    }
    Err(None)
}

pub async fn resolve(
    rb: &Roblox,
    api_key: Option<&str>,
    pool: &Pool,
    id: i64,
    creator: Option<(String, i64)>,
) -> Result<Attempt> {
    let has_cookies = pool.alive_count() > 0;
    if api_key.is_none() && !has_cookies {
        if pool.total_count() > 0 {
            return Err(Error::Denied(
                "every saved cookie is expired or invalid, replace them in Settings and Recheck in Accounts".into(),
            ));
        }
        return Err(Error::NoCredentials);
    }
    let mut notes: Vec<String> = Vec::new();
    let mut limited = false;

    if let Some(key) = api_key {
        match rb.fetch_opencloud(key, id).await {
            Ok(bytes) => {
                return Ok(Attempt { bytes, method: "Open Cloud API key".into(), place_id: None, account: None })
            }
            Err(Error::RateLimited(_)) => {
                limited = true;
                notes.push("Open Cloud: rate limited".into());
            }
            Err(e) => notes.push(format!("Open Cloud: {e}")),
        }
    }

    if !has_cookies {
        notes.push("Cookie: none saved or all expired".into());
        return Err(finish(notes, limited));
    }

    let mut tried: Vec<String> = Vec::new();
    loop {
        match via_cookie(rb, pool, id, None, &tried).await {
            Ok((bytes, label)) => {
                return Ok(Attempt {
                    bytes,
                    method: format!("Cookie (direct) \u{b7} {label}"),
                    place_id: None,
                    account: Some(label),
                })
            }
            Err(Some(d)) => {
                notes.push(format!("Cookie {}: {}", d.label, d.error));
                tried.push(d.cookie);
            }
            Err(None) => {
                if tried.is_empty() {
                    limited = true;
                    notes.push("Cookie: every account is rate limited".into());
                }
                break;
            }
        }
    }

    let Some((ctype, cid)) = creator.filter(|(_, cid)| *cid > 0) else {
        notes.push("Place-Id fallback: skipped, asset has no public creator info".into());
        return Err(finish(notes, limited));
    };
    let places = rb.list_creator_places(&ctype, cid).await;
    if places.is_empty() {
        notes.push(format!("Place-Id fallback: the creator ({ctype} {cid}) has no public places"));
        return Err(finish(notes, limited));
    }
    let mut last = String::new();
    let mut count = 0;
    for p in &places {
        match via_cookie(rb, pool, id, Some(p.place_id), &[]).await {
            Ok((bytes, label)) => {
                return Ok(Attempt {
                    bytes,
                    method: format!("Cookie \u{b7} {label} \u{b7} Place-Id {} ({})", p.place_id, p.name),
                    place_id: Some(p.place_id),
                    account: Some(label),
                })
            }
            Err(Some(d)) => {
                count += 1;
                last = d.error.to_string();
            }
            Err(None) => {
                limited = true;
                notes.push(format!("Place-Id fallback: rate limited after {count} places"));
                return Err(finish(notes, limited));
            }
        }
    }
    notes.push(format!(
        "Place-Id fallback: {count} public places from the creator, all denied (last: {last})"
    ));
    Err(finish(notes, limited))
}

fn finish(notes: Vec<String>, limited: bool) -> Error {
    if limited && notes.iter().all(|n| n.contains("rate limited")) {
        Error::RateLimited(0)
    } else {
        Error::Denied(notes.join(" | "))
    }
}
