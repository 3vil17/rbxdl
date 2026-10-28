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
    version: Option<u32>,
    exclude: &[String],
) -> std::result::Result<(Vec<u8>, String), Option<Denial>> {
    for _ in 0..8 {
        let lease = pool.acquire(exclude).await.ok_or(None)?;
        match rb.fetch_cookie(&lease.cookie, id, place, version).await {
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
    version: Option<u32>,
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
        match rb.fetch_opencloud(key, id, version).await {
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
        match via_cookie(rb, pool, id, None, version, &tried).await {
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
    let mut tried_places: Vec<i64> = Vec::new();
    let places = rb.list_creator_places(&ctype, cid, 60).await;
    if places.is_empty() {
        notes.push(format!("Place-Id fallback: the creator ({ctype} {cid}) has no public places"));
    } else {
        match try_places(rb, pool, id, version, &places, None, &mut tried_places).await {
            Places::Hit(a) => return Ok(a),
            Places::Limited(n) => {
                notes.push(format!("Place-Id fallback: rate limited after {n} places"));
                return Err(finish(notes, true));
            }
            Places::Denied(n, last) => notes.push(format!(
                "Place-Id fallback: {n} public places from the creator, all denied (last: {last})"
            )),
        }
    }

    if !ctype.eq_ignore_ascii_case("User") {
        return Err(finish(notes, limited));
    }
    let mut groups = rb.user_groups(cid).await;
    groups.sort_by(|a, b| b.rank.cmp(&a.rank));
    groups.truncate(GROUP_LIMIT);
    let mut count = 0;
    let mut last = String::new();
    for g in &groups {
        if count >= GROUP_PLACE_LIMIT {
            break;
        }
        let places = rb.list_creator_places("Group", g.id, 10).await;
        match try_places(rb, pool, id, version, &places, Some(&g.name), &mut tried_places).await {
            Places::Hit(a) => return Ok(a),
            Places::Limited(n) => {
                notes.push(format!("Group games: rate limited after {} places", count + n));
                return Err(finish(notes, true));
            }
            Places::Denied(n, l) => {
                count += n;
                if !l.is_empty() {
                    last = l;
                }
            }
        }
    }
    if groups.is_empty() {
        notes.push("Group games: the creator is in no public groups".into());
    } else {
        notes.push(format!(
            "Group games: {count} places across {} of the creator's groups, all denied{}",
            groups.len(),
            if last.is_empty() { String::new() } else { format!(" (last: {last})") }
        ));
    }
    Err(finish(notes, limited))
}

const GROUP_LIMIT: usize = 15;
const GROUP_PLACE_LIMIT: usize = 60;

enum Places {
    Hit(Attempt),
    Denied(usize, String),
    Limited(usize),
}

async fn try_places(
    rb: &Roblox,
    pool: &Pool,
    id: i64,
    version: Option<u32>,
    places: &[crate::roblox::PlaceRef],
    group: Option<&str>,
    tried: &mut Vec<i64>,
) -> Places {
    let mut count = 0;
    let mut last = String::new();
    for p in places {
        if tried.contains(&p.place_id) {
            continue;
        }
        tried.push(p.place_id);
        match via_cookie(rb, pool, id, Some(p.place_id), version, &[]).await {
            Ok((bytes, label)) => {
                let via = match group {
                    Some(g) => format!(" via group {g}"),
                    None => String::new(),
                };
                return Places::Hit(Attempt {
                    bytes,
                    method: format!("Cookie \u{b7} {label} \u{b7} Place-Id {} ({}){via}", p.place_id, p.name),
                    place_id: Some(p.place_id),
                    account: Some(label),
                });
            }
            Err(Some(d)) => {
                count += 1;
                last = d.error.to_string();
            }
            Err(None) => return Places::Limited(count),
        }
    }
    Places::Denied(count, last)
}

fn finish(notes: Vec<String>, limited: bool) -> Error {
    if limited && notes.iter().all(|n| n.contains("rate limited")) {
        Error::RateLimited(0)
    } else {
        Error::Denied(notes.join(" | "))
    }
}
