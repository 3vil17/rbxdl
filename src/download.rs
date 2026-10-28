use crate::convert;
use crate::error::Result;
use crate::resolver;
use crate::pool::Pool;
use crate::roblox::Roblox;
use crate::store;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct DownloadInfo {
    pub id: i64,
    pub name: String,
    pub type_label: String,
    pub method: String,
    pub place_id: Option<i64>,
    pub account: Option<String>,
    pub path: String,
}

fn now_millis() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_default()
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() { "asset".into() } else { trimmed.chars().take(80).collect() }
}

pub async fn fetch_and_save(
    rb: &Roblox,
    api_key: Option<&str>,
    pool: &Pool,
    id: i64,
    version: Option<u32>,
    settings: &store::Settings,
) -> Result<DownloadInfo> {
    let details = rb.asset_details(id).await.ok();
    let (name, atype0, creator) = match &details {
        Some(d) => (d.name.clone(), d.asset_type_id, Some((d.creator_type.clone(), d.creator_id))),
        None => (format!("asset_{id}"), 0, None),
    };

    let mut attempt = resolver::resolve(rb, api_key, pool, id, version, creator).await?;

    let mut atype = atype0;
    if let Some(inner) = convert::inner_asset_id(&attempt.bytes) {
        if inner != id {
            if let Ok(a2) = resolver::resolve(rb, api_key, pool, inner, None, None).await {
                attempt.bytes = a2.bytes;
                attempt.method = format!("{} (decal \u{2192} {})", attempt.method, inner);
                atype = 1;
            }
        }
    }

    let ext = convert::extension_for(&attempt.bytes, atype);
    let type_label = convert::type_label(atype0).to_string();
    let filename = match version {
        Some(v) => format!("{}_{}_v{}.{}", sanitize(&name), id, v, ext),
        None => format!("{}_{}.{}", sanitize(&name), id, ext),
    };

    let mut dir = std::path::PathBuf::from(&settings.save_dir);
    if settings.sort_by_type {
        dir.push(sanitize(&type_label));
    }
    std::fs::create_dir_all(&dir)?;
    let full = dir.join(&filename);
    std::fs::write(&full, &attempt.bytes)?;
    let path = full.to_string_lossy().to_string();

    let _ = store::push_history(store::HistoryEntry {
        id,
        name: name.clone(),
        type_label: type_label.clone(),
        path: path.clone(),
        method: attempt.method.clone(),
        when: now_millis(),
    });

    Ok(DownloadInfo {
        id,
        name,
        type_label,
        method: attempt.method,
        place_id: attempt.place_id,
        account: attempt.account,
        path,
    })
}
