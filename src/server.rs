use crate::download;
use crate::roblox::Roblox;
use crate::store;
use axum::{
    extract::{Query, State},
    http::header,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Clone)]
struct AppState {
    rb: Roblox,
}

pub async fn run() -> std::io::Result<()> {
    let state = AppState { rb: Roblox::new() };

    let app = Router::new()
        .route("/", get(index))
        .route("/logo.png", get(logo))
        .route("/api/lookup", get(lookup))
        .route("/api/download", post(download_asset))
        .route("/api/settings", get(get_settings).post(set_settings))
        .route("/api/credentials", post(set_credentials))
        .route("/api/cookie-info", get(cookie_info))
        .route("/api/history", get(get_history))
        .route("/api/history/clear", post(clear_history))
        .route("/api/reveal", post(reveal))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let url = format!("http://{addr}");
    println!("rbxdl running at {url}");
    let _ = open::that(&url);

    axum::serve(listener, app).await
}

async fn index() -> Html<&'static str> {
    Html(include_str!("web/index.html"))
}

async fn logo() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/png")], &include_bytes!("web/logo.png")[..])
}

fn ok(data: Value) -> Json<Value> {
    Json(json!({ "ok": true, "data": data }))
}

fn fail(msg: impl Into<String>) -> Json<Value> {
    Json(json!({ "ok": false, "error": msg.into() }))
}

/// Pull an asset id out of a plain number or any Roblox URL the user pastes.
fn parse_asset_id(input: &str) -> Option<i64> {
    let digits: String = input.chars().filter(|c| c.is_ascii_digit()).collect();
    if let Ok(n) = input.trim().parse::<i64>() {
        if n > 0 {
            return Some(n);
        }
    }
    // Longest run of digits, handles catalog/library/create URLs.
    let mut best = String::new();
    let mut cur = String::new();
    for ch in input.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else {
            if cur.len() > best.len() {
                best = cur.clone();
            }
            cur.clear();
        }
    }
    if cur.len() > best.len() {
        best = cur;
    }
    let pick = if best.is_empty() { digits } else { best };
    pick.parse::<i64>().ok().filter(|n| *n > 0)
}

#[derive(Deserialize)]
struct LookupQ {
    id: String,
}

async fn lookup(State(st): State<AppState>, Query(q): Query<LookupQ>) -> Json<Value> {
    let Some(id) = parse_asset_id(&q.id) else {
        return fail("enter a valid asset id or URL");
    };
    match st.rb.asset_details(id).await {
        Ok(d) => {
            let thumb = st.rb.thumbnail_url(id).await;
            ok(json!({
                "id": d.id,
                "name": d.name,
                "description": d.description,
                "type_label": crate::convert::type_label(d.asset_type_id),
                "creator_type": d.creator_type,
                "creator_name": d.creator_name,
                "thumbnail": thumb,
            }))
        }
        Err(_) => ok(json!({
            "id": id,
            "name": format!("asset_{id}"),
            "description": "",
            "type_label": "Asset",
            "creator_type": "",
            "creator_name": "",
            "thumbnail": Value::Null,
            "note": "no public details, download will still try the auth ladder",
        })),
    }
}

#[derive(Deserialize)]
struct DownloadBody {
    id: String,
}

async fn download_asset(State(st): State<AppState>, Json(body): Json<DownloadBody>) -> Json<Value> {
    let Some(id) = parse_asset_id(&body.id) else {
        return fail("enter a valid asset id or URL");
    };
    let settings = store::load_settings();
    let api_key = store::get_secret("api_key");
    let cookie = store::get_secret("cookie");
    match download::fetch_and_save(
        &st.rb,
        api_key.as_deref(),
        cookie.as_deref(),
        id,
        &settings.save_dir,
    )
    .await
    {
        Ok(info) => ok(json!({
            "id": info.id,
            "name": info.name,
            "type_label": info.type_label,
            "method": info.method,
            "place_id": info.place_id,
            "path": info.path,
        })),
        Err(e) => fail(e.to_string()),
    }
}

async fn get_settings() -> Json<Value> {
    let s = store::load_settings();
    ok(json!({
        "save_dir": s.save_dir,
        "has_key": store::get_secret("api_key").is_some(),
        "has_cookie": store::get_secret("cookie").is_some(),
    }))
}

#[derive(Deserialize)]
struct SettingsBody {
    save_dir: String,
}

async fn set_settings(Json(body): Json<SettingsBody>) -> Json<Value> {
    let dir = body.save_dir.trim();
    if dir.is_empty() {
        return fail("save folder cannot be empty");
    }
    let s = store::Settings {
        save_dir: dir.to_string(),
    };
    match store::save_settings(&s) {
        Ok(_) => ok(json!({ "save_dir": s.save_dir })),
        Err(e) => fail(e.to_string()),
    }
}

#[derive(Deserialize)]
struct CredsBody {
    api_key: Option<String>,
    cookie: Option<String>,
}

async fn set_credentials(Json(body): Json<CredsBody>) -> Json<Value> {
    for (key, val) in [("api_key", body.api_key), ("cookie", body.cookie)] {
        if let Some(v) = val {
            let v = v.trim();
            let res = if v.is_empty() {
                store::clear_secret(key)
            } else {
                store::save_secret(key, v)
            };
            if let Err(e) = res {
                return fail(e.to_string());
            }
        }
    }
    ok(json!({
        "has_key": store::get_secret("api_key").is_some(),
        "has_cookie": store::get_secret("cookie").is_some(),
    }))
}

async fn cookie_info(State(st): State<AppState>) -> Json<Value> {
    let Some(cookie) = store::get_secret("cookie") else {
        return ok(json!({ "alive": false, "reason": "no cookie saved" }));
    };
    let info = st.rb.cookie_info(&cookie).await;
    ok(json!({
        "alive": info.alive,
        "user_id": info.user_id,
        "username": info.username,
        "display_name": info.display_name,
        "robux": info.robux,
    }))
}

async fn get_history() -> Json<Value> {
    ok(json!(store::load_history()))
}

async fn clear_history() -> Json<Value> {
    match store::clear_history() {
        Ok(_) => ok(Value::Null),
        Err(e) => fail(e.to_string()),
    }
}

#[derive(Deserialize)]
struct RevealBody {
    path: String,
}

/// Open the system file browser with the saved file selected (Windows).
async fn reveal(Json(body): Json<RevealBody>) -> Json<Value> {
    let path = body.path.trim();
    if path.is_empty() {
        return fail("no path");
    }
    let res = std::process::Command::new("explorer")
        .arg(format!("/select,{path}"))
        .spawn();
    match res {
        Ok(_) => ok(Value::Null),
        Err(e) => fail(e.to_string()),
    }
}
