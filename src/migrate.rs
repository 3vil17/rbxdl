use crate::convert;
use crate::download;
use crate::error::{Error, Result};
use crate::pool::Pool;
use crate::roblox::{Creator, Roblox};
use crate::store;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Moves a place's assets to a creator the account manages. Only assets made
/// by that account or one of the groups it manages are touched.
#[derive(Serialize, Clone, Default)]
pub struct Item {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub type_label: String,
    pub owner: String,
    pub owner_name: String,
    pub state: String,
    pub new_id: Option<i64>,
    pub decal_id: Option<i64>,
    pub error: Option<String>,
}

#[derive(Serialize, Default)]
pub struct Job {
    pub place: String,
    pub checked: bool,
    pub running: bool,
    pub cancel: bool,
    pub run_id: u64,
    pub mode: String,
    pub items: Vec<Item>,
}

pub type Shared = Arc<Mutex<Job>>;

pub fn lock(job: &Shared) -> std::sync::MutexGuard<'_, Job> {
    job.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn kind_of(asset_type_id: i64) -> &'static str {
    match asset_type_id {
        3 => "Audio",
        1 | 13 => "Image",
        4 | 40 => "Mesh",
        24 => "Animation",
        _ => "",
    }
}

/// Fills in name, type and owner for everything the plugin sent.
pub async fn check(rb: Roblox, pool: Pool, job: Shared) {
    let ids: Vec<i64> = lock(&job).items.iter().map(|i| i.id).collect();
    if let Some(lease) = pool.acquire(&[]).await {
        rb.prefetch_details(&lease.cookie, &ids).await;
    }
    let mut details = Vec::new();
    for id in &ids {
        let d = match rb.cached_details(*id) {
            Some(d) => Some(d),
            None => rb.asset_details(*id).await.ok(),
        };
        details.push(d);
    }
    let mut owners: Vec<String> = details.iter().flatten().map(|d| format!("{}:{}", d.creator_type, d.creator_id)).collect();
    owners.sort();
    owners.dedup();
    let names = rb.owner_names(&owners).await;
    let mut j = lock(&job);
    for (it, d) in j.items.iter_mut().zip(details) {
        if let Some(d) = d {
            it.name = d.name;
            it.kind = kind_of(d.asset_type_id).to_string();
            it.type_label = convert::type_label(d.asset_type_id).to_string();
            it.owner = format!("{}:{}", d.creator_type, d.creator_id);
            it.owner_name = names.get(&it.owner).cloned().unwrap_or(d.creator_name);
        }
    }
    j.checked = true;
}

fn upload_kind(kind: &str, bytes: &[u8]) -> Option<(&'static str, &'static str, &'static str)> {
    let ext = convert::extension_for(bytes, 0);
    Some(match (kind, ext) {
        ("Audio", "ogg") => ("Audio", "asset.ogg", "audio/ogg"),
        ("Audio", "mp3") => ("Audio", "asset.mp3", "audio/mpeg"),
        ("Audio", "wav") => ("Audio", "asset.wav", "audio/wav"),
        ("Image", "png") => ("Decal", "asset.png", "image/png"),
        ("Image", "jpg") => ("Decal", "asset.jpg", "image/jpeg"),
        ("Mesh", "mesh") => ("Mesh", "asset.mesh", "model/x-file-mesh-data"),
        ("Animation", "rbxm") => ("Animation", "asset.rbxm", "model/x-rbxm"),
        ("Animation", "rbxmx") => ("Animation", "asset.rbxmx", "model/x-rbxmx"),
        _ => return None,
    })
}

/// A decal upload hands back the decal's id, but properties like
/// ImageLabel.Image need the image inside it, which shows up once Roblox
/// has processed the upload.
async fn image_inside(rb: &Roblox, cookie: &str, decal_id: i64) -> Option<i64> {
    for _ in 0..10 {
        if let Ok(bytes) = rb.fetch_cookie(cookie, decal_id, None, None).await {
            if let Some(inner) = convert::inner_asset_id(&bytes) {
                return Some(inner);
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    None
}

fn still_processing(decal: i64) -> Error {
    Error::Other(format!("uploaded as decal {decal}, Roblox is still processing it. Retry in a minute"))
}

/// Uploads one asset, or for an image whose decal already went up, only
/// looks for the image again. The decal id is written to the item as soon
/// as it exists so a retry never uploads the same image twice.
async fn move_one(rb: &Roblox, pool: &Pool, cookie: &str, to: &Creator, job: &Shared, i: usize) -> Result<i64> {
    let item = lock(job).items[i].clone();
    if let Some(decal) = item.decal_id {
        return image_inside(rb, cookie, decal).await.ok_or_else(|| still_processing(decal));
    }
    let api_key = store::get_secret("api_key");
    let mut bytes = download::resolve_bytes(rb, api_key.as_deref(), pool, item.id).await?;
    if item.kind == "Image" {
        if let Some(inner) = convert::inner_asset_id(&bytes).filter(|i| *i != item.id) {
            bytes = download::resolve_bytes(rb, api_key.as_deref(), pool, inner).await?;
        }
        if convert::extension_for(&bytes, 1) == "webp" {
            bytes = convert::to_png(&bytes).ok_or_else(|| Error::Other("couldn't convert the webp image".into()))?;
        }
    }
    let (asset_type, file_name, mime) = upload_kind(&item.kind, &bytes)
        .ok_or_else(|| Error::Other(format!("Roblox doesn't take this file as {}", item.kind.to_lowercase())))?;
    let name: String = if item.name.trim().is_empty() { format!("asset {}", item.id) } else { item.name.chars().take(50).collect() };
    let mut tries = 0;
    let new_id = loop {
        match rb.upload_asset(cookie, bytes.clone(), asset_type, &name, file_name, mime, to).await {
            Err(Error::RateLimited(s)) if tries < 3 => {
                tries += 1;
                tokio::time::sleep(Duration::from_secs(s.clamp(5, 60))).await;
            }
            other => break other?,
        }
    };
    if asset_type != "Decal" {
        return Ok(new_id);
    }
    lock(job).items[i].decal_id = Some(new_id);
    image_inside(rb, cookie, new_id).await.ok_or_else(|| still_processing(new_id))
}

pub async fn run(rb: Roblox, pool: Pool, job: Shared, cookie: String, to: Creator, todo: Vec<usize>) {
    for i in todo {
        {
            let mut j = lock(&job);
            if j.cancel {
                j.items[i].state = "found".into();
                continue;
            }
            j.items[i].state = "working".into();
        }
        let res = move_one(&rb, &pool, &cookie, &to, &job, i).await;
        {
            let mut j = lock(&job);
            let it = &mut j.items[i];
            match res {
                Ok(new_id) => {
                    it.state = "ok".into();
                    it.new_id = Some(new_id);
                    it.error = None;
                }
                Err(e) => {
                    it.state = "error".into();
                    it.error = Some(e.to_string());
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
    }
    let mut j = lock(&job);
    j.running = false;
    j.cancel = false;
}
