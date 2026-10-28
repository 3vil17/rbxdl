use crate::convert;
use crate::error::{Error, Result};
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

fn clean(text: &str, max: usize) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    cleaned.trim().trim_matches('.').trim().chars().take(max).collect()
}

fn sanitize(name: &str) -> String {
    let out = clean(name, 80);
    if out.is_empty() { "asset".into() } else { out }
}

/// Fills {name} {id} {type} {version} in the user's template. An old version
/// always gets a _vN suffix when the template doesn't place {version} itself.
pub fn file_stem(template: &str, name: &str, id: i64, type_label: &str, version: Option<u32>) -> String {
    let t = if template.trim().is_empty() { store::DEFAULT_TEMPLATE } else { template };
    let mut out = t
        .replace("{name}", &sanitize(name))
        .replace("{id}", &id.to_string())
        .replace("{type}", type_label)
        .replace("{version}", &version.map(|v| v.to_string()).unwrap_or_default());
    if let Some(v) = version.filter(|_| !t.contains("{version}")) {
        out.push_str(&format!("_v{v}"));
    }
    let out = clean(&out, 150);
    if out.is_empty() { format!("asset_{id}") } else { out }
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

    if name == "[ Content Deleted ]" {
        return Err(Error::Denied("this asset was moderated by Roblox (content deleted)".into()));
    }
    let mut attempt = match resolver::resolve(rb, api_key, pool, id, version, creator, None).await {
        Ok(a) => a,
        Err(Error::Denied(msg)) if atype0 == 10 => {
            return Err(Error::Denied(format!(
                "{msg} | Note: this is a model. Packages and private models only download for accounts that have access to them"
            )))
        }
        Err(e) => return Err(e),
    };

    let mut atype = atype0;
    let follow = matches!(atype0, 0 | 1 | 13);
    if let Some(inner) = convert::inner_asset_id(&attempt.bytes).filter(|_| follow) {
        if inner != id {
            if let Ok(a2) = resolver::resolve(rb, api_key, pool, inner, None, None, None).await {
                attempt.bytes = a2.bytes;
                attempt.method = format!("{} (decal \u{2192} {})", attempt.method, inner);
                atype = 1;
            }
        }
    }

    let mut ext = convert::extension_for(&attempt.bytes, atype);
    if settings.images_png && matches!(ext, "jpg" | "webp") {
        if let Some(png) = convert::to_png(&attempt.bytes) {
            attempt.bytes = png;
            ext = "png";
        }
    }
    let type_label = convert::type_label(atype0).to_string();
    let filename = format!("{}.{}", file_stem(&settings.name_template, &name, id, &type_label, version), ext);

    let mut dir = std::path::PathBuf::from(&settings.save_dir);
    if settings.sort_by_type {
        dir.push(sanitize(&type_label));
    }
    std::fs::create_dir_all(&dir)?;
    let full = dir.join(&filename);
    std::fs::write(&full, &attempt.bytes)?;
    if ext == "mesh" && settings.mesh_obj {
        if let Some(obj) = crate::mesh::to_obj(&attempt.bytes) {
            let _ = std::fs::write(full.with_extension("obj"), obj);
        }
    }
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

pub async fn resolve_bytes(
    rb: &Roblox,
    api_key: Option<&str>,
    pool: &Pool,
    id: i64,
    place: Option<i64>,
) -> Result<Vec<u8>> {
    let creator = rb.asset_details(id).await.ok().map(|d| (d.creator_type, d.creator_id));
    Ok(resolver::resolve(rb, api_key, pool, id, None, creator, place).await?.bytes)
}
