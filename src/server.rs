use crate::download;
use crate::error::Error;
use crate::pool::Pool;
use crate::roblox::Roblox;
use crate::store;
use axum::{
    extract::{Query, State},
    http::header,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const BULK_MAX: usize = 500;

#[derive(Clone)]
struct AppState {
    rb: Roblox,
    pool: Pool,
    bulk: Arc<Mutex<Bulk>>,
}

#[derive(Default)]
struct Bulk {
    running: bool,
    cancel: bool,
    items: Vec<BulkItem>,
}

#[derive(Serialize, Clone, Default)]
struct BulkItem {
    id: i64,
    state: &'static str,
    name: Option<String>,
    method: Option<String>,
    account: Option<String>,
    path: Option<String>,
    error: Option<String>,
}

pub async fn run() -> std::io::Result<()> {
    let state = AppState { rb: Roblox::new(), pool: Pool::default(), bulk: Arc::default() };
    state.pool.reload(&state.rb).await;

    let app = Router::new()
        .route("/", get(index))
        .route("/logo.png", get(logo))
        .route("/api/lookup", get(lookup))
        .route("/api/download", post(download_asset))
        .route("/api/settings", get(get_settings).post(set_settings))
        .route("/api/credentials", post(set_credentials))
        .route("/api/accounts", get(accounts))
        .route("/api/accounts/refresh", post(refresh_accounts))
        .route("/api/bulk", post(bulk_start))
        .route("/api/bulk/status", get(bulk_status))
        .route("/api/bulk/cancel", post(bulk_cancel))
        .route("/api/history", get(get_history))
        .route("/api/history/clear", post(clear_history))
        .route("/api/reveal", post(reveal))
        .route("/api/pick-folder", post(pick_folder))
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

fn parse_asset_id(input: &str) -> Option<i64> {
    let digits: String = input.chars().filter(|c| c.is_ascii_digit()).collect();
    if let Ok(n) = input.trim().parse::<i64>() {
        if n > 0 {
            return Some(n);
        }
    }
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
    match download::fetch_and_save(
        &st.rb,
        api_key.as_deref(),
        &st.pool,
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
            "account": info.account,
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
        "cookie_count": store::load_cookies().len(),
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
    cookies: Option<String>,
}

fn parse_cookies(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in raw.split(|c: char| c.is_whitespace() || c == ';') {
        let c = part.trim().trim_start_matches(".ROBLOSECURITY=").trim();
        if !c.is_empty() && !out.iter().any(|x| x == c) {
            out.push(c.to_string());
        }
    }
    out
}

async fn set_credentials(State(st): State<AppState>, Json(body): Json<CredsBody>) -> Json<Value> {
    if let Some(v) = body.api_key {
        let v = v.trim();
        let res = if v.is_empty() { store::clear_secret("api_key") } else { store::save_secret("api_key", v) };
        if let Err(e) = res {
            return fail(e.to_string());
        }
    }
    if let Some(raw) = body.cookies {
        if let Err(e) = store::save_cookies(&parse_cookies(&raw)) {
            return fail(e.to_string());
        }
        st.pool.reload(&st.rb).await;
    }
    ok(json!({
        "has_key": store::get_secret("api_key").is_some(),
        "cookie_count": store::load_cookies().len(),
    }))
}

async fn accounts(State(st): State<AppState>) -> Json<Value> {
    ok(st.pool.snapshot())
}

async fn refresh_accounts(State(st): State<AppState>) -> Json<Value> {
    st.pool.reload(&st.rb).await;
    ok(st.pool.snapshot())
}

#[derive(Deserialize)]
struct BulkBody {
    ids: Vec<String>,
}

async fn bulk_start(State(st): State<AppState>, Json(body): Json<BulkBody>) -> Json<Value> {
    let mut ids: Vec<i64> = Vec::new();
    for raw in &body.ids {
        if let Some(id) = parse_asset_id(raw) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    if ids.is_empty() {
        return fail("no valid asset ids found");
    }
    if ids.len() > BULK_MAX {
        return fail(format!("{} ids, the limit is {BULK_MAX} per batch", ids.len()));
    }
    {
        let mut b = st.bulk.lock().unwrap_or_else(|e| e.into_inner());
        if b.running {
            return fail("a bulk download is already running");
        }
        *b = Bulk {
            running: true,
            cancel: false,
            items: ids.iter().map(|&id| BulkItem { id, state: "queued", ..Default::default() }).collect(),
        };
    }
    let workers = (st.pool.alive_count() * 2).clamp(2, 8);
    let total = ids.len();
    tokio::spawn(run_bulk(st, ids, workers));
    ok(json!({ "total": total, "workers": workers }))
}

fn bulk_set(st: &AppState, i: usize, f: impl FnOnce(&mut BulkItem)) -> bool {
    let mut b = st.bulk.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut b.items[i]);
    b.cancel
}

async fn run_bulk(st: AppState, ids: Vec<i64>, workers: usize) {
    let ids = Arc::new(ids);
    let next = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for _ in 0..workers {
        let (st, ids, next) = (st.clone(), ids.clone(), next.clone());
        handles.push(tokio::spawn(async move {
            let settings = store::load_settings();
            let api_key = store::get_secret("api_key");
            loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(&id) = ids.get(i) else { break };
                if bulk_set(&st, i, |it| it.state = "working") {
                    bulk_set(&st, i, |it| it.state = "skipped");
                    continue;
                }
                let mut tries = 0;
                let res = loop {
                    let r = download::fetch_and_save(&st.rb, api_key.as_deref(), &st.pool, id, &settings.save_dir).await;
                    match r {
                        Err(Error::RateLimited(s)) if tries < 4 => {
                            tries += 1;
                            bulk_set(&st, i, |it| it.state = "waiting");
                            tokio::time::sleep(std::time::Duration::from_secs(s.max(10))).await;
                            bulk_set(&st, i, |it| it.state = "working");
                        }
                        other => break other,
                    }
                };
                match res {
                    Ok(info) => bulk_set(&st, i, |it| {
                        it.state = "ok";
                        it.name = Some(info.name);
                        it.method = Some(info.method);
                        it.account = info.account;
                        it.path = Some(info.path);
                    }),
                    Err(e) => bulk_set(&st, i, |it| {
                        it.state = "error";
                        it.error = Some(e.to_string());
                    }),
                };
            }
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    st.bulk.lock().unwrap_or_else(|e| e.into_inner()).running = false;
}

async fn bulk_status(State(st): State<AppState>) -> Json<Value> {
    let b = st.bulk.lock().unwrap_or_else(|e| e.into_inner());
    ok(json!({ "running": b.running, "items": b.items }))
}

async fn bulk_cancel(State(st): State<AppState>) -> Json<Value> {
    st.bulk.lock().unwrap_or_else(|e| e.into_inner()).cancel = true;
    ok(Value::Null)
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

// Windows only lets the process that got the last input steal focus, and that's
// the browser. A no-op mouse move makes it us, then we pass the right on.
fn unlock_foreground() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: 0, dwFlags: MOUSEEVENTF_MOVE, time: 0, dwExtraInfo: 0 },
        },
    };
    unsafe {
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
        AllowSetForegroundWindow(ASFW_ANY);
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// Explorer often opens behind the browser anyway, so find its window by title
// and raise it ourselves.
async fn raise_explorer(dir: std::path::PathBuf) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GetWindowTextW, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };
    let full = dir.to_string_lossy().to_lowercase();
    let name = dir.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let class = wide("CabinetWClass");
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let mut found = None;
        unsafe {
            let mut h = FindWindowExW(std::ptr::null_mut(), std::ptr::null_mut(), class.as_ptr(), std::ptr::null());
            while !h.is_null() {
                let mut buf = [0u16; 512];
                let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
                let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]).to_lowercase();
                let base = title.split(" - ").next().unwrap_or("");
                if base == name || base == full {
                    found = Some(h as usize);
                    break;
                }
                h = FindWindowExW(std::ptr::null_mut(), h, class.as_ptr(), std::ptr::null());
            }
        }
        if let Some(h) = found {
            unlock_foreground();
            unsafe {
                let h = h as windows_sys::Win32::Foundation::HWND;
                if IsIconic(h) != 0 {
                    ShowWindow(h, SW_RESTORE);
                }
                SetForegroundWindow(h);
            }
            return;
        }
    }
}

async fn pick_folder() -> Json<Value> {
    let start = store::load_settings().save_dir;
    let picked = tokio::task::spawn_blocking(move || {
        unlock_foreground();
        rfd::FileDialog::new().set_title("Download folder").set_directory(&start).pick_folder()
    })
    .await
    .ok()
    .flatten();
    let Some(dir) = picked else {
        return ok(json!({ "save_dir": Value::Null }));
    };
    let s = store::Settings { save_dir: dir.to_string_lossy().to_string() };
    match store::save_settings(&s) {
        Ok(_) => ok(json!({ "save_dir": s.save_dir })),
        Err(e) => fail(e.to_string()),
    }
}

async fn reveal(Json(body): Json<RevealBody>) -> Json<Value> {
    let path = body.path.trim().replace('/', "\\");
    if path.is_empty() {
        return fail("no path");
    }
    let p = std::path::Path::new(&path);
    let Some(dir) = p.parent().filter(|d| d.exists()).map(|d| d.to_path_buf()) else {
        return fail("that folder no longer exists");
    };
    let arg = if p.exists() { format!("/select,{path}") } else { dir.to_string_lossy().to_string() };
    unlock_foreground();
    let res = std::process::Command::new("explorer").arg(arg).spawn();
    if res.is_ok() {
        tokio::spawn(raise_explorer(dir));
    }
    match res {
        Ok(_) => ok(Value::Null),
        Err(e) => fail(e.to_string()),
    }
}
