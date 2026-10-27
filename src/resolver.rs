use crate::error::{Error, Result};
use crate::roblox::Roblox;

pub struct Attempt {
    pub bytes: Vec<u8>,
    pub method: String,
    pub place_id: Option<i64>,
}

/// Walk the authorization ladder: API key, then a direct cookie request, then
/// the creator's own places used as a `Roblox-Place-Id` context.
pub async fn resolve(
    rb: &Roblox,
    api_key: Option<&str>,
    cookie: Option<&str>,
    id: i64,
    creator: Option<(String, i64)>,
) -> Result<Attempt> {
    if api_key.is_none() && cookie.is_none() {
        return Err(Error::NoCredentials);
    }

    if let Some(key) = api_key {
        if let Ok(bytes) = rb.fetch_opencloud(key, id).await {
            return Ok(Attempt { bytes, method: "Open Cloud API key".into(), place_id: None });
        }
    }

    if let Some(c) = cookie {
        if let Ok(bytes) = rb.fetch_cookie(c, id, None).await {
            return Ok(Attempt { bytes, method: "Cookie (direct)".into(), place_id: None });
        }

        if let Some((ctype, cid)) = creator {
            if cid > 0 {
                let places = rb.list_creator_places(&ctype, cid).await;
                for p in places {
                    if let Ok(bytes) = rb.fetch_cookie(c, id, Some(p.place_id)).await {
                        return Ok(Attempt {
                            bytes,
                            method: format!("Cookie · Place-Id {} ({})", p.place_id, p.name),
                            place_id: Some(p.place_id),
                        });
                    }
                }
            }
        }
    }

    Err(Error::NotAuthorized)
}
