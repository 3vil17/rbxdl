use crate::error::{Error, Result};
use serde::Serialize;
use serde_json::Value;
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
    winners: HashMap<(String, i64), Vec<PlaceRef>>,
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
pub struct GroupRef {
    pub id: i64,
    pub name: String,
    pub rank: i64,
}

#[derive(Serialize, Clone)]
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
        Self { http, cache: Arc::default() }
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
        let key = (creator_type.to_ascii_lowercase(), creator_id);
        self.cache().winners.get(&key).cloned().unwrap_or_default()
    }

    pub fn remember_place(&self, creator_type: &str, creator_id: i64, place: &PlaceRef) {
        let key = (creator_type.to_ascii_lowercase(), creator_id);
        let mut c = self.cache();
        let list = c.winners.entry(key).or_default();
        list.retain(|p| p.place_id != place.place_id);
        list.insert(0, place.clone());
        list.truncate(5);
    }

    pub async fn asset_details(&self, id: i64) -> Result<AssetDetails> {
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

    pub async fn thumbnail_url(&self, id: i64) -> Option<String> {
        let url = format!(
            "https://thumbnails.roblox.com/v1/assets?assetIds={id}&size=420x420&format=Png"
        );
        let v: Value = self.http.get(&url).send().await.ok()?.json().await.ok()?;
        v["data"][0]["imageUrl"].as_str().map(|s| s.to_string())
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

    pub async fn latest_release(&self) -> Option<(String, String)> {
        let v: Value = self
            .http
            .get("https://api.github.com/repos/3vil17/rbxyoinker/releases/latest")
            .header("User-Agent", "rbxyoinker")
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        Some((v["tag_name"].as_str()?.to_string(), v["html_url"].as_str()?.to_string()))
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
