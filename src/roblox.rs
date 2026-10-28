use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const UA: &str = "Roblox/WinInet";
const CACHE_TTL: Duration = Duration::from_secs(600);

#[derive(Clone)]
pub struct Roblox {
    http: reqwest::Client,
    cache: Arc<Mutex<Cache>>,
}

#[derive(Default)]
struct Cache {
    places: HashMap<(String, i64, usize), (Instant, Vec<PlaceRef>)>,
    groups: HashMap<i64, (Instant, Vec<GroupRef>)>,
    winners: HashMap<String, Vec<PlaceRef>>,
    details: HashMap<i64, AssetDetails>,
    releases: Option<(Instant, Vec<Release>)>,
}

fn releases_file() -> std::path::PathBuf {
    crate::store::config_dir().join("releases.json")
}

#[derive(Serialize, Deserialize)]
struct SavedReleases {
    etag: String,
    body: String,
}

fn parse_releases(body: &str) -> Option<Vec<Release>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let list = v
        .as_array()?
        .iter()
        .filter(|r| r["draft"].as_bool() != Some(true) && r["prerelease"].as_bool() != Some(true))
        .filter_map(|r| {
            let exe_url = r["assets"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|a| a["name"].as_str().is_some_and(|n| n.to_ascii_lowercase().ends_with(".exe")))
                .and_then(|a| a["browser_download_url"].as_str())
                .map(String::from);
            Some(Release {
                tag: r["tag_name"].as_str()?.to_string(),
                url: r["html_url"].as_str().unwrap_or_default().to_string(),
                notes: r["body"].as_str().unwrap_or_default().to_string(),
                exe_url,
            })
        })
        .collect();
    Some(list)
}

fn places_file() -> std::path::PathBuf {
    crate::store::config_dir().join("places.json")
}

fn fresh<T: Clone>(entry: Option<&(Instant, T)>) -> Option<T> {
    entry.filter(|(t, _)| t.elapsed() < CACHE_TTL).map(|(_, v)| v.clone())
}

#[derive(Serialize, Clone)]
pub struct AssetDetails {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub asset_type_id: i64,
    pub creator_type: String,
    pub creator_id: i64,
    pub creator_name: String,
}

#[derive(Serialize, Clone)]
pub struct CookieInfo {
    pub alive: bool,
    pub user_id: Option<i64>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub robux: Option<i64>,
}

#[derive(Clone)]
pub struct Release {
    pub tag: String,
    pub url: String,
    pub notes: String,
    pub exe_url: Option<String>,
}

/// Who a moved asset gets created under.
pub enum Creator {
    User(i64),
    Group(i64),
}

#[derive(Clone)]
pub struct GroupRef {
    pub id: i64,
    pub name: String,
    pub rank: i64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PlaceRef {
    pub place_id: i64,
    pub name: String,
}

impl Roblox {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(UA)
            .gzip(true)
            .build()
            .expect("http client");
        let winners = std::fs::read_to_string(places_file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let cache = Cache { winners, ..Default::default() };
        Self { http, cache: Arc::new(Mutex::new(cache)) }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub async fn user_groups(&self, user_id: i64) -> Vec<GroupRef> {
        if let Some(v) = fresh(self.cache().groups.get(&user_id)) {
            return v;
        }
        let v = self.fetch_user_groups(user_id).await;
        self.cache().groups.insert(user_id, (Instant::now(), v.clone()));
        v
    }

    pub async fn list_creator_places(&self, creator_type: &str, creator_id: i64, max: usize) -> Vec<PlaceRef> {
        let key = (creator_type.to_ascii_lowercase(), creator_id, max);
        if let Some(v) = fresh(self.cache().places.get(&key)) {
            return v;
        }
        let v = self.fetch_creator_places(creator_type, creator_id, max).await;
        self.cache().places.insert(key, (Instant::now(), v.clone()));
        v
    }

    pub fn known_places(&self, creator_type: &str, creator_id: i64) -> Vec<PlaceRef> {
        let key = format!("{}:{creator_id}", creator_type.to_ascii_lowercase());
        self.cache().winners.get(&key).cloned().unwrap_or_default()
    }

    pub fn remember_place(&self, creator_type: &str, creator_id: i64, place: &PlaceRef) {
        let key = format!("{}:{creator_id}", creator_type.to_ascii_lowercase());
        let mut c = self.cache();
        let list = c.winners.entry(key).or_default();
        if list.first().is_some_and(|p| p.place_id == place.place_id) {
            return;
        }
        list.retain(|p| p.place_id != place.place_id);
        list.insert(0, place.clone());
        list.truncate(5);
        if let Ok(json) = serde_json::to_vec(&c.winners) {
            let _ = std::fs::write(places_file(), json);
        }
    }

    pub async fn prefetch_details(&self, cookie: &str, ids: &[i64]) -> usize {
        let mut found = 0;
        for chunk in ids.chunks(50) {
            let list: Vec<String> = chunk.iter().map(|i| i.to_string()).collect();
            let url = format!("https://develop.roblox.com/v1/assets?assetIds={}", list.join(","));
            let Ok(resp) = self.http.get(&url).header("Cookie", format!(".ROBLOSECURITY={cookie}")).send().await else {
                continue;
            };
            let Ok(v) = resp.json::<Value>().await else { continue };
            for it in v["data"].as_array().into_iter().flatten() {
                let Some(id) = it["id"].as_i64() else { continue };
                let creator = &it["creator"];
                let ctype = match creator["type"].as_str() {
                    Some(t) => t.to_string(),
                    None if creator["typeId"].as_i64() == Some(2) => "Group".into(),
                    None => "User".into(),
                };
                let d = AssetDetails {
                    id,
                    name: it["name"].as_str().unwrap_or("Unknown").to_string(),
                    description: it["description"].as_str().unwrap_or("").to_string(),
                    asset_type_id: it["typeId"].as_i64().unwrap_or(0),
                    creator_type: ctype,
                    creator_id: creator["targetId"].as_i64().unwrap_or(0),
                    creator_name: String::new(),
                };
                self.cache().details.insert(id, d);
                found += 1;
            }
        }
        found
    }

    pub async fn asset_details(&self, id: i64) -> Result<AssetDetails> {
        if let Some(d) = self.cache().details.get(&id) {
            return Ok(d.clone());
        }
        let url = format!("https://economy.roblox.com/v2/assets/{id}/details");
        let v: Value = self.http.get(&url).send().await?.json().await?;
        if v.get("errors").is_some() {
            return Err(Error::Other(format!("asset {id} has no public details")));
        }
        let creator = v.get("Creator").cloned().unwrap_or(Value::Null);
        Ok(AssetDetails {
            id,
            name: v["Name"].as_str().unwrap_or("Unknown").to_string(),
            description: v["Description"].as_str().unwrap_or("").to_string(),
            asset_type_id: v["AssetTypeId"].as_i64().unwrap_or(0),
            creator_type: creator["CreatorType"].as_str().unwrap_or("User").to_string(),
            creator_id: creator["CreatorTargetId"].as_i64().unwrap_or(0),
            creator_name: creator["Name"].as_str().unwrap_or("Unknown").to_string(),
        })
    }

    pub async fn thumbnail(&self, id: i64) -> (Option<String>, Option<String>) {
        let url = format!(
            "https://thumbnails.roblox.com/v1/assets?assetIds={id}&size=420x420&format=Png"
        );
        let v: Option<Value> = match self.http.get(&url).send().await {
            Ok(r) => r.json().await.ok(),
            Err(_) => None,
        };
        let Some(v) = v else { return (None, None) };
        let t = &v["data"][0];
        (t["imageUrl"].as_str().map(String::from), t["state"].as_str().map(String::from))
    }

    pub async fn public_assets(&self, ids: &[i64]) -> std::collections::HashSet<i64> {
        let mut out = std::collections::HashSet::new();
        for chunk in ids.chunks(50) {
            let list: Vec<String> = chunk.iter().map(|i| i.to_string()).collect();
            let url = format!(
                "https://apis.roblox.com/toolbox-service/v1/items/details?assetIds={}",
                list.join(",")
            );
            let Ok(resp) = self.http.get(&url).send().await else { continue };
            let Ok(v) = resp.json::<Value>().await else { continue };
            for it in v["data"].as_array().into_iter().flatten() {
                if it["fiatProduct"]["published"].as_bool() == Some(true) {
                    if let Some(id) = it["asset"]["id"].as_i64() {
                        out.insert(id);
                    }
                }
            }
        }
        out
    }

    pub fn cached_details(&self, id: i64) -> Option<AssetDetails> {
        self.cache().details.get(&id).cloned()
    }

    pub async fn thumbnails(&self, ids: &[i64]) -> HashMap<i64, String> {
        let mut out = HashMap::new();
        for chunk in ids.chunks(100) {
            let list: Vec<String> = chunk.iter().map(|i| i.to_string()).collect();
            let url = format!(
                "https://thumbnails.roblox.com/v1/assets?assetIds={}&size=150x150&format=Png",
                list.join(",")
            );
            let Ok(resp) = self.http.get(&url).send().await else { continue };
            let Ok(v) = resp.json::<Value>().await else { continue };
            for it in v["data"].as_array().into_iter().flatten() {
                if let (Some(id), Some(img)) = (it["targetId"].as_i64(), it["imageUrl"].as_str()) {
                    out.insert(id, img.to_string());
                }
            }
        }
        out
    }

    pub async fn user_headshots(&self, ids: &[i64]) -> std::collections::HashMap<i64, String> {
        let mut out = std::collections::HashMap::new();
        if ids.is_empty() {
            return out;
        }
        let list: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
        let url = format!(
            "https://thumbnails.roblox.com/v1/users/avatar-headshot?userIds={}&size=48x48&format=Png&isCircular=false",
            list.join(",")
        );
        let Ok(resp) = self.http.get(&url).send().await else { return out };
        let Ok(v) = resp.json::<Value>().await else { return out };
        for it in v["data"].as_array().into_iter().flatten() {
            if let (Some(id), Some(img)) = (it["targetId"].as_i64(), it["imageUrl"].as_str()) {
                out.insert(id, img.to_string());
            }
        }
        out
    }

    /// Bundles list their parts as items, outfits as assets. Both are public.
    /// Display names for "Group:1" / "User:2" style owners, looked up in batches.
    pub async fn owner_names(&self, owners: &[String]) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let ids_of = |kind: &str| -> Vec<String> {
            owners.iter().filter_map(|o| o.strip_prefix(kind)).map(String::from).collect()
        };
        for chunk in ids_of("Group:").chunks(100) {
            let url = format!("https://groups.roblox.com/v2/groups?groupIds={}", chunk.join(","));
            let Ok(resp) = self.http.get(&url).send().await else { continue };
            let Ok(v) = resp.json::<Value>().await else { continue };
            for g in v["data"].as_array().into_iter().flatten() {
                if let (Some(id), Some(name)) = (g["id"].as_i64(), g["name"].as_str()) {
                    out.insert(format!("Group:{id}"), name.to_string());
                }
            }
        }
        let users: Vec<i64> = ids_of("User:").iter().filter_map(|x| x.parse().ok()).collect();
        for chunk in users.chunks(100) {
            let Ok(resp) = self.http.post("https://users.roblox.com/v1/users").json(&json!({ "userIds": chunk })).send().await else {
                continue;
            };
            let Ok(v) = resp.json::<Value>().await else { continue };
            for u in v["data"].as_array().into_iter().flatten() {
                if let (Some(id), Some(name)) = (u["id"].as_i64(), u["name"].as_str()) {
                    out.insert(format!("User:{id}"), format!("@{name}"));
                }
            }
        }
        out
    }

    pub async fn bundle_assets(&self, id: i64) -> Option<(String, Vec<i64>)> {
        let url = format!("https://catalog.roblox.com/v1/bundles/{id}/details");
        let v: Value = self.http.get(&url).send().await.ok()?.json().await.ok()?;
        let ids = v["items"]
            .as_array()?
            .iter()
            .filter(|it| it["type"].as_str() == Some("Asset"))
            .filter_map(|it| it["id"].as_i64())
            .collect();
        Some((v["name"].as_str().unwrap_or("bundle").to_string(), ids))
    }

    pub async fn outfit_assets(&self, id: i64) -> Option<(String, Vec<i64>)> {
        let url = format!("https://avatar.roblox.com/v3/outfits/{id}/details");
        let v: Value = self.http.get(&url).send().await.ok()?.json().await.ok()?;
        let ids = v["assets"].as_array()?.iter().filter_map(|it| it["id"].as_i64()).collect();
        Some((v["name"].as_str().unwrap_or("outfit").to_string(), ids))
    }

    /// A CSRF token, taken from an endpoint that refuses requests without one.
    async fn csrf_token(&self, cookie: &str) -> Option<String> {
        let resp = self
            .http
            .post("https://auth.roblox.com/v2/logout")
            .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
            .send()
            .await
            .ok()?;
        resp.headers().get("x-csrf-token").and_then(|v| v.to_str().ok()).map(String::from)
    }

    /// Groups this cookie's user can manage, the only ones assets get moved between.
    pub async fn manageable_groups(&self, cookie: &str) -> Option<Vec<i64>> {
        let resp = self
            .http
            .get("https://develop.roblox.com/v1/user/groups/canmanage")
            .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let v: Value = resp.json().await.ok()?;
        Some(v["data"].as_array()?.iter().filter_map(|it| it["id"].as_i64()).collect())
    }

    pub async fn group_icons(&self, ids: &[i64]) -> HashMap<i64, String> {
        let mut out = HashMap::new();
        for chunk in ids.chunks(100) {
            let csv: Vec<String> = chunk.iter().map(|i| i.to_string()).collect();
            let url = format!(
                "https://thumbnails.roblox.com/v1/groups/icons?groupIds={}&size=150x150&format=Png&isCircular=false",
                csv.join(",")
            );
            let Ok(resp) = self.http.get(&url).send().await else { continue };
            let Ok(v) = resp.json::<Value>().await else { continue };
            for it in v["data"].as_array().into_iter().flatten() {
                if let (Some(id), Some(img)) = (it["targetId"].as_i64(), it["imageUrl"].as_str()) {
                    out.insert(id, img.to_string());
                }
            }
        }
        out
    }

    /// Creates a new asset through the Assets API with the cookie's login and
    /// waits for the upload operation to hand back the new id.
    pub async fn upload_asset(
        &self,
        cookie: &str,
        bytes: Vec<u8>,
        asset_type: &str,
        name: &str,
        file_name: &str,
        mime: &str,
        creator: &Creator,
    ) -> Result<i64> {
        let token = self
            .csrf_token(cookie)
            .await
            .ok_or_else(|| Error::Other("couldn't get a CSRF token from Roblox".into()))?;
        let creator_json = match creator {
            Creator::User(id) => json!({ "userId": id.to_string() }),
            Creator::Group(id) => json!({ "groupId": id.to_string() }),
        };
        let request = json!({
            "assetType": asset_type,
            "displayName": name,
            "description": "",
            "creationContext": { "creator": creator_json },
        })
        .to_string();
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(file_name.to_string())
            .mime_str(mime)
            .map_err(|e| Error::Other(e.to_string()))?;
        let form = reqwest::multipart::Form::new().text("request", request).part("fileContent", part);
        let resp = self
            .http
            .post("https://apis.roblox.com/assets/user-auth/v1/assets")
            .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
            .header("x-csrf-token", &token)
            .multipart(form)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(error_from(resp).await);
        }
        let v: Value = resp.json().await?;
        if let Some(id) = asset_id_from(&v) {
            return Ok(id);
        }
        let op = v["operationId"]
            .as_str()
            .map(String::from)
            .or_else(|| v["path"].as_str().map(|p| p.rsplit('/').next().unwrap_or(p).to_string()))
            .ok_or_else(|| Error::Other("upload started but Roblox returned no operation".into()))?;
        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let ov: Value = self
                .http
                .get(format!("https://apis.roblox.com/assets/user-auth/v1/operations/{op}"))
                .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
                .header("x-csrf-token", &token)
                .send()
                .await?
                .json()
                .await?;
            if ov["done"].as_bool() == Some(true) {
                return asset_id_from(&ov).ok_or_else(|| {
                    Error::Other(format!("upload finished without an asset id: {}", ov.to_string().chars().take(200).collect::<String>()))
                });
            }
        }
        Err(Error::Other("upload is still processing after 30s, check the group's creations".into()))
    }

    /// GitHub allows 60 unauthenticated API calls an hour per IP, so the list
    /// is kept in memory for a couple of minutes and on disk with its ETag.
    /// A 304 doesn't count toward the limit, and when GitHub refuses the last
    /// list it gave is used instead.
    pub async fn releases(&self) -> Option<Vec<Release>> {
        if let Some((at, list)) = &self.cache().releases {
            if at.elapsed() < Duration::from_secs(120) {
                return Some(list.clone());
            }
        }
        let saved: Option<SavedReleases> = std::fs::read(releases_file()).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let mut req = self
            .http
            .get("https://api.github.com/repos/3vil17/rbxyoinker/releases?per_page=30")
            .header("User-Agent", "rbxyoinker");
        if let Some(etag) = saved.as_ref().map(|s| s.etag.as_str()).filter(|e| !e.is_empty()) {
            req = req.header("If-None-Match", etag);
        }
        let mut body = None;
        if let Ok(resp) = req.send().await {
            if resp.status().is_success() {
                let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
                if let Ok(text) = resp.text().await {
                    let _ = std::fs::write(releases_file(), serde_json::to_vec(&SavedReleases { etag, body: text.clone() }).unwrap_or_default());
                    body = Some(text);
                }
            }
        }
        let body = body.or_else(|| saved.map(|s| s.body))?;
        let list = parse_releases(&body)?;
        self.cache().releases = Some((Instant::now(), list.clone()));
        Some(list)
    }

    pub async fn download_file(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.http.get(url).header("User-Agent", "rbxyoinker").send().await?;
        if !resp.status().is_success() {
            return Err(Error::Other(format!("download failed with {}", resp.status())));
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn cookie_info(&self, cookie: &str) -> CookieInfo {
        let auth = self
            .http
            .get("https://users.roblox.com/v1/users/authenticated")
            .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
            .send()
            .await;
        let v = match auth {
            Ok(r) if r.status().is_success() => r.json::<Value>().await.ok(),
            _ => None,
        };
        let Some(v) = v else {
            return CookieInfo { alive: false, user_id: None, username: None, display_name: None, robux: None };
        };
        let user_id = v["id"].as_i64();
        let robux = self
            .http
            .get("https://economy.roblox.com/v1/user/currency")
            .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
            .send()
            .await
            .ok();
        let robux = match robux {
            Some(r) if r.status().is_success() => {
                r.json::<Value>().await.ok().and_then(|j| j["robux"].as_i64())
            }
            _ => None,
        };
        CookieInfo {
            alive: true,
            user_id,
            username: v["name"].as_str().map(|s| s.to_string()),
            display_name: v["displayName"].as_str().map(|s| s.to_string()),
            robux,
        }
    }

    async fn fetch_user_groups(&self, user_id: i64) -> Vec<GroupRef> {
        let url = format!("https://groups.roblox.com/v2/users/{user_id}/groups/roles");
        let Ok(resp) = self.http.get(&url).send().await else { return Vec::new() };
        let Ok(v) = resp.json::<Value>().await else { return Vec::new() };
        v["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|it| {
                Some(GroupRef {
                    id: it["group"]["id"].as_i64()?,
                    name: it["group"]["name"].as_str().unwrap_or("group").to_string(),
                    rank: it["role"]["rank"].as_i64().unwrap_or(0),
                })
            })
            .collect()
    }

    async fn fetch_creator_places(&self, creator_type: &str, creator_id: i64, max: usize) -> Vec<PlaceRef> {
        let page = if max <= 10 { 10 } else { 50 };
        let base = if creator_type.eq_ignore_ascii_case("Group") {
            format!("https://games.roblox.com/v2/groups/{creator_id}/games")
        } else {
            format!("https://games.roblox.com/v2/users/{creator_id}/games")
        };
        let mut out = Vec::new();
        let mut cursor = String::new();
        for _ in 0..4 {
            let url = format!("{base}?accessFilter=Public&limit={page}&sortOrder=Asc&cursor={cursor}");
            let mut got = None;
            for attempt in 0..4u64 {
                let Ok(resp) = self.http.get(&url).send().await else { break };
                if resp.status().as_u16() == 429 {
                    tokio::time::sleep(std::time::Duration::from_secs(2 + attempt * 2)).await;
                    continue;
                }
                got = resp.json::<Value>().await.ok();
                break;
            }
            let Some(v) = got else { break };
            if let Some(items) = v["data"].as_array() {
                for it in items {
                    let place_id = it["rootPlaceId"]
                        .as_i64()
                        .or_else(|| it["rootPlace"]["id"].as_i64())
                        .or_else(|| it["id"].as_i64());
                    if let Some(place_id) = place_id {
                        out.push(PlaceRef {
                            place_id,
                            name: it["name"].as_str().unwrap_or("game").to_string(),
                        });
                    }
                }
            }
            match v["nextPageCursor"].as_str() {
                Some(c) if !c.is_empty() => cursor = c.to_string(),
                _ => break,
            }
            if out.len() >= max {
                break;
            }
        }
        out.truncate(max);
        out
    }

    pub async fn fetch_opencloud(&self, api_key: &str, id: i64, version: Option<u32>) -> Result<Vec<u8>> {
        let url = match version {
            Some(v) => format!("https://apis.roblox.com/asset-delivery-api/v1/assetId/{id}/version/{v}"),
            None => format!("https://apis.roblox.com/asset-delivery-api/v1/assetId/{id}"),
        };
        let resp = self
            .http
            .get(&url)
            .header("x-api-key", api_key)
            .header("Accept-Encoding", "gzip")
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(error_from(resp).await);
        }
        let v: Value = resp.json().await?;
        let location = v["location"]
            .as_str()
            .or_else(|| v["locations"][0]["location"].as_str())
            .ok_or_else(|| Error::Denied(message_of(&v).unwrap_or_else(|| "no download location returned".into())))?;
        let bytes = self.http.get(location).send().await?.bytes().await?;
        Ok(bytes.to_vec())
    }

    pub async fn fetch_cookie(
        &self,
        cookie: &str,
        id: i64,
        place_id: Option<i64>,
        version: Option<u32>,
    ) -> Result<Vec<u8>> {
        let mut url = format!("https://assetdelivery.roblox.com/v1/asset/?id={id}");
        if let Some(v) = version {
            url.push_str(&format!("&version={v}"));
        }
        let mut req = self
            .http
            .get(&url)
            .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
            .header("Accept-Encoding", "gzip");
        if let Some(pid) = place_id {
            req = req.header("Roblox-Place-Id", pid.to_string());
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(error_from(resp).await);
        }
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = resp.bytes().await?.to_vec();
        // A JSON error body can come back with a 200, so inspect it.
        if ct.contains("json") || bytes.starts_with(b"{\"errors\"") {
            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                if v.get("errors").is_some() {
                    return Err(Error::Denied(message_of(&v).unwrap_or_else(|| "unknown error".into())));
                }
            }
        }
        if bytes.is_empty() {
            return Err(Error::Denied("empty response".into()));
        }
        Ok(bytes)
    }
}

fn message_of(v: &Value) -> Option<String> {
    let e = &v["errors"][0];
    e["message"]
        .as_str()
        .or_else(|| e["detail"].as_str())
        .or_else(|| v["message"].as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

async fn error_from(resp: reqwest::Response) -> Error {
    let status = resp.status().as_u16();
    let retry = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    if status == 429 {
        return Error::RateLimited(retry);
    }
    let body = resp.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<Value>(&body).ok().and_then(|v| message_of(&v));
    match (status, msg) {
        (401, _) => Error::Denied("401 cookie is expired or invalid".into()),
        (s, Some(m)) => Error::Denied(format!("{s} {m}")),
        (403, None) => Error::Denied("403 forbidden".into()),
        (404, None) => Error::Denied("404 asset not found".into()),
        (s, None) => Error::Denied(format!("{s} no message from Roblox")),
    }
}

fn asset_id_from(v: &Value) -> Option<i64> {
    [&v["response"]["assetId"], &v["assetId"], &v["response"]["id"]]
        .iter()
        .find_map(|x| x.as_i64().or_else(|| x.as_str().and_then(|s| s.parse().ok())))
}
