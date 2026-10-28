use crate::download;
use crate::error::Error;
use crate::pool::Pool;
use crate::roblox::Roblox;
use crate::store;
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Query, State},
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
    skip_existing: bool,
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

async fn already_running() -> Option<String> {
    let raw = std::fs::read_to_string(store::config_dir().join("instance.json")).ok()?;
    let url = serde_json::from_str::<Value>(&raw).ok()?["url"].as_str()?.to_string();
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(2)).build().ok()?;
    let resp = client.get(format!("{url}/api/settings")).send().await.ok()?;
    resp.status().is_success().then_some(url)
}

pub async fn run() -> std::io::Result<()> {
    if let Some(url) = already_running().await {
        println!("rbxyoinker is already running at {url}");
        let _ = open::that(&url);
        return Ok(());
    }
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
        .route("/api/bulk/retry", post(bulk_retry))
        .route("/api/bulk/zip", post(bulk_zip))
        .route("/api/scan-file", post(scan_file).layer(DefaultBodyLimit::max(512 * 1024 * 1024)))
        .route("/api/update", get(update_check))
        .route("/api/history", get(get_history))
        .route("/api/history/clear", post(clear_history))
        .route("/api/reveal", post(reveal))
        .route("/api/pick-folder", post(pick_folder))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let url = format!("http://{addr}");
    println!("rbxyoinker running at {url}");
    let _ = std::fs::write(
        store::config_dir().join("instance.json"),
        json!({ "url": url, "pid": std::process::id() }).to_string(),
    );
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
    version: Option<u32>,
}

async fn download_asset(State(st): State<AppState>, Json(body): Json<DownloadBody>) -> Json<Value> {
    let Some(id) = parse_asset_id(&body.id) else {
        return fail("enter a valid asset id or URL");
    };
    let settings = store::load_settings();
    let api_key = store::get_secret("api_key");
    let version = body.version.filter(|v| *v > 0);
    match download::fetch_and_save(&st.rb, api_key.as_deref(), &st.pool, id, version, &settings).await {
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
        "sort_by_type": s.sort_by_type,
        "has_key": store::get_secret("api_key").is_some(),
        "cookie_count": store::load_cookies().len(),
    }))
}

#[derive(Deserialize)]
struct SettingsBody {
    save_dir: Option<String>,
    sort_by_type: Option<bool>,
}

async fn set_settings(Json(body): Json<SettingsBody>) -> Json<Value> {
    let mut s = store::load_settings();
    if let Some(dir) = body.save_dir {
        let dir = dir.trim();
        if dir.is_empty() {
            return fail("save folder cannot be empty");
        }
        s.save_dir = dir.to_string();
    }
    if let Some(sort) = body.sort_by_type {
        s.sort_by_type = sort;
    }
    match store::save_settings(&s) {
        Ok(_) => ok(json!({ "save_dir": s.save_dir, "sort_by_type": s.sort_by_type })),
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

fn version_tuple(v: &str) -> Vec<u32> {
    v.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

async fn update_check(State(st): State<AppState>) -> Json<Value> {
    let current = env!("CARGO_PKG_VERSION");
    match st.rb.latest_release().await {
        Some((tag, url)) if version_tuple(&tag) > version_tuple(current) => {
            ok(json!({ "available": true, "latest": tag, "url": url, "current": current }))
        }
        _ => ok(json!({ "available": false, "current": current })),
    }
}

async fn scan_file(body: Bytes) -> Json<Value> {
    let ids = tokio::task::spawn_blocking(move || crate::scan::asset_ids(&body)).await.unwrap_or_default();
    ok(json!({ "ids": ids.iter().map(|i| i.to_string()).collect::<Vec<_>>() }))
}

#[derive(Deserialize)]
struct BulkBody {
    ids: Vec<String>,
    #[serde(default)]
    skip_existing: bool,
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
            skip_existing: body.skip_existing,
            items: ids.iter().map(|&id| BulkItem { id, state: "queued", ..Default::default() }).collect(),
        };
    }
    let total = ids.len();
    let workers = spawn_bulk(&st, (0..total).collect());
    ok(json!({ "total": total, "workers": workers }))
}

async fn bulk_retry(State(st): State<AppState>) -> Json<Value> {
    let todo: Vec<usize> = {
        let mut b = st.bulk.lock().unwrap_or_else(|e| e.into_inner());
        if b.running {
            return fail("a bulk download is already running");
        }
        let todo: Vec<usize> =
            b.items.iter().enumerate().filter(|(_, it)| it.state == "error" || it.state == "skipped").map(|(i, _)| i).collect();
        for &i in &todo {
            b.items[i] = BulkItem { id: b.items[i].id, state: "queued", ..Default::default() };
        }
        if !todo.is_empty() {
            b.running = true;
            b.cancel = false;
        }
        todo
    };
    if todo.is_empty() {
        return fail("nothing failed, nothing to retry");
    }
    let total = todo.len();
    let workers = spawn_bulk(&st, todo);
    ok(json!({ "total": total, "workers": workers }))
}

fn spawn_bulk(st: &AppState, todo: Vec<usize>) -> usize {
    let workers = (st.pool.alive_count() * 2).clamp(2, 8);
    tokio::spawn(run_bulk(st.clone(), todo, workers));
    workers
}

fn bulk_set(st: &AppState, i: usize, f: impl FnOnce(&mut BulkItem)) -> bool {
    let mut b = st.bulk.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut b.items[i]);
    b.cancel
}

async fn run_bulk(st: AppState, todo: Vec<usize>, workers: usize) {
    let todo = Arc::new(todo);
    let next = Arc::new(AtomicUsize::new(0));
    let skip_existing = st.bulk.lock().unwrap_or_else(|e| e.into_inner()).skip_existing;
    let mut handles = Vec::new();
    for _ in 0..workers {
        let (st, todo, next) = (st.clone(), todo.clone(), next.clone());
        handles.push(tokio::spawn(async move {
            let settings = store::load_settings();
            let api_key = store::get_secret("api_key");
            loop {
                let Some(&i) = todo.get(next.fetch_add(1, Ordering::SeqCst)) else { break };
                let id = st.bulk.lock().unwrap_or_else(|e| e.into_inner()).items[i].id;
                if bulk_set(&st, i, |it| it.state = "working") {
                    bulk_set(&st, i, |it| it.state = "skipped");
                    continue;
                }
                if skip_existing {
                    if let Some(h) = store::existing_download(id) {
                        bulk_set(&st, i, |it| {
                            it.state = "have";
                            it.name = Some(h.name);
                            it.method = Some("already downloaded".into());
                            it.path = Some(h.path);
                        });
                        continue;
                    }
                }
                let mut tries = 0;
                let res = loop {
                    let r = download::fetch_and_save(&st.rb, api_key.as_deref(), &st.pool, id, None, &settings).await;
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

fn write_zip(save_dir: &str, paths: &[String]) -> std::result::Result<String, String> {
    use std::io::Write;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let base = std::path::Path::new(save_dir);
    std::fs::create_dir_all(base).map_err(|e| e.to_string())?;
    let out = base.join(format!("rbxyoinker_{stamp}.zip"));
    let file = std::fs::File::create(&out).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut used: Vec<String> = Vec::new();
    for p in paths {
        let path = std::path::Path::new(p);
        let Ok(bytes) = std::fs::read(path) else { continue };
        let rel = path.strip_prefix(base).unwrap_or(path);
        let mut name = rel.to_string_lossy().replace('\\', "/");
        if path.strip_prefix(base).is_err() {
            name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(name);
        }
        if used.contains(&name) {
            continue;
        }
        zip.start_file(name.as_str(), opts).map_err(|e| e.to_string())?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
        used.push(name);
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(out.to_string_lossy().to_string())
}

async fn bulk_zip(State(st): State<AppState>) -> Json<Value> {
    let paths: Vec<String> = {
        let b = st.bulk.lock().unwrap_or_else(|e| e.into_inner());
        b.items.iter().filter(|it| it.state == "ok" || it.state == "have").filter_map(|it| it.path.clone()).collect()
    };
    if paths.is_empty() {
        return fail("nothing downloaded to zip yet");
    }
    let dir = store::load_settings().save_dir;
    let count = paths.len();
    match tokio::task::spawn_blocking(move || write_zip(&dir, &paths)).await {
        Ok(Ok(path)) => ok(json!({ "path": path, "count": count })),
        Ok(Err(e)) => fail(e),
        Err(e) => fail(e.to_string()),
    }
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
    let mut s = store::load_settings();
    s.save_dir = dir.to_string_lossy().to_string();
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
