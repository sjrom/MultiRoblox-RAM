use crate::encryption;
use crate::settings::{load_settings, save_settings};
use crate::state::AppState;
use crate::storage;
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub fn show_main_window(app: tauri::AppHandle) {
    app.state::<AppState>()
        .ui_ready
        .store(true, std::sync::atomic::Ordering::SeqCst);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
    }
}

/// Real exit, from the in-app Quit affordance. Sets the flag first so the
/// hide-to-tray close handler and prevent_exit both stand aside.
#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    app.state::<AppState>()
        .quitting
        .store(true, std::sync::atomic::Ordering::SeqCst);
    app.exit(0);
}

/// Creates or removes the tray icon to match the toggle. The setting itself is
/// persisted by settings_save; this only reconciles the icon.
#[tauri::command]
pub fn set_hide_to_tray(app: tauri::AppHandle, enabled: bool) {
    crate::tray::apply_setting(&app, enabled);
}

// ---- settings ----
#[tauri::command]
pub fn settings_load() -> Value {
    let mut s = load_settings();
    for k in ["customKeyEnc", "customKey", "keyVerifier", "_deviceKey"] {
        s.remove(k);
    }
    let key_set = encryption::passphrase_mode();
    s.insert("keySet".into(), Value::Bool(key_set));
    Value::Object(s)
}

#[tauri::command]
pub fn settings_save(state: State<AppState>, data: Map<String, Value>) -> bool {
    let mut data = data;
    for k in ["customKey", "customKeyEnc", "keyVerifier"] {
        data.remove(k);
    }
    let mut s = load_settings();
    s.remove("webhook");
    s.remove("screenshotsEnabled");
    let multi_instance = data.get("multiInstance").cloned();
    let antiafk = data.get("antiAfk").cloned();
    let antiafk_interval_changed = data.contains_key("antiAfkInterval");
    for (k, v) in data {
        s.insert(k, v);
    }
    save_settings(&s);
    if let Some(Value::Bool(on)) = multi_instance {
        // Releases/re-acquires the mutex inside the one helper process;
        // toggling this no longer starts or kills anything.
        let app = state.app_handle.clone();
        tauri::async_runtime::spawn(async move {
            let st = app.state::<AppState>();
            crate::native::set_multi_instance(&app, &st, on).await;
        });
    }
    if let Some(Value::Bool(on)) = antiafk {
        let app = state.app_handle.clone();
        if on {
            tauri::async_runtime::spawn(async move {
                let st = app.state::<AppState>();
                crate::native::start_antiafk(&app, &st).await;
            });
        } else {
            crate::native::stop_antiafk(&state);
        }
    } else if antiafk_interval_changed
        && state.antiafk_on.load(std::sync::atomic::Ordering::SeqCst)
    {
        // Re-arm at the new interval. start_antiafk replaces the helper's
        // existing anti-AFK thread, so no explicit stop is needed first.
        let app = state.app_handle.clone();
        tauri::async_runtime::spawn(async move {
            let st = app.state::<AppState>();
            crate::native::start_antiafk(&app, &st).await;
        });
    }
    true
}

// ---- encryption ----
#[tauri::command]
pub fn enc_status(state: State<AppState>) -> Value {
    if !encryption::passphrase_mode() {
        return serde_json::json!({ "mode": "setup" });
    }
    let unlocked = state.session_pass.lock().unwrap().is_some();
    serde_json::json!({ "mode": if unlocked { "unlocked" } else { "locked" } })
}

#[tauri::command]
pub fn enc_unlock(state: State<AppState>, pass: String) -> Value {
    if pass.is_empty() || !encryption::verify_pass(&pass) {
        return serde_json::json!({ "ok": false });
    }
    *state.session_pass.lock().unwrap() = Some(pass);
    encryption::invalidate_key_cache(&state);
    serde_json::json!({ "ok": true })
}

#[tauri::command]
pub fn enc_set_key(state: State<AppState>, pass: Option<String>) -> Value {
    let np = pass.unwrap_or_default().trim().to_string();
    let raw = storage::load_accounts_raw();
    let accts_dec: Vec<Value> = raw
        .iter()
        .cloned()
        .map(|a| storage::decrypt_account(&state, a))
        .collect();
    for (orig, dec) in raw.iter().zip(accts_dec.iter()) {
        let had_cookie = orig
            .get("cookie")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        let now_empty = dec
            .get("cookie")
            .and_then(|v| v.as_str())
            .map(|s| s.is_empty())
            .unwrap_or(true);
        if had_cookie && now_empty {
            return serde_json::json!({ "ok": false, "error": "decrypt failed" });
        }
    }

    if !np.is_empty() {
        encryption::rotate_salt();
        *state.session_pass.lock().unwrap() = Some(np.clone());
        encryption::invalidate_key_cache(&state);
        let mut rest = load_settings();
        rest.remove("customKey");
        rest.remove("customKeyEnc");
        let verifier = encryption::make_verifier(&np);
        rest.insert("keyVerifier".into(), Value::String(verifier));
        rest.insert("encSetupDone".into(), Value::Bool(true));
        save_settings(&rest);
    } else {
        *state.session_pass.lock().unwrap() = None;
        encryption::invalidate_key_cache(&state);
        let mut rest = load_settings();
        rest.remove("customKey");
        rest.remove("customKeyEnc");
        rest.remove("keyVerifier");
        rest.insert("encSetupDone".into(), Value::Bool(true));
        save_settings(&rest);
    }
    encryption::invalidate_key_cache(&state);
    match storage::save_accounts(&state, accts_dec) {
        Ok(_) => serde_json::json!({ "ok": true }),
        Err(e) => serde_json::json!({ "ok": false, "error": e }),
    }
}

#[tauri::command]
pub async fn multiinstance_status(state: State<'_, AppState>) -> Result<Value, ()> {
    // Defaults on, matching the startup path in lib.rs; only an explicit
    // false counts as off. These two used to disagree (false here, true
    // there), so on a fresh install the toggle read "off" while the helper was
    // already holding the mutex.
    let enabled = load_settings()
        .get("multiInstance")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    // "active" now means the one helper is up AND actually holding the mutex,
    // which is a stronger (and more honest) signal than "a child exists".
    let active = crate::helper::is_holding_mutex(&state).await;
    Ok(serde_json::json!({ "enabled": enabled, "active": active }))
}

#[tauri::command]
pub fn antiafk_status(state: State<AppState>) -> Value {
    let enabled = load_settings()
        .get("antiAfk")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let active = state.antiafk_on.load(std::sync::atomic::Ordering::SeqCst);
    serde_json::json!({ "enabled": enabled, "active": active })
}

// ---- accounts ----
#[tauri::command]
pub fn accounts_load(state: State<AppState>) -> Vec<Value> {
    storage::load_accounts(&state)
}

#[tauri::command]
pub fn accounts_add(state: State<AppState>, account: Map<String, Value>) -> Result<Value, String> {
    let mut accounts = storage::load_accounts(&state);
    let mut a = Value::Object(account);
    if a.get("fpsCap").is_none() {
        a["fpsCap"] = Value::Number(30.into());
    }
    a["id"] = Value::String(uuid::Uuid::new_v4().to_string());
    a["createdAt"] = Value::String(chrono::Utc::now().to_rfc3339());
    a["lastUsed"] = Value::Null;
    accounts.push(a.clone());
    storage::save_accounts(&state, accounts)?;
    Ok(a)
}

#[tauri::command]
pub fn accounts_remove(state: State<AppState>, id: String) -> Result<bool, String> {
    let accounts = storage::load_accounts(&state);
    let filtered: Vec<Value> = accounts
        .into_iter()
        .filter(|a| a.get("id").and_then(|v| v.as_str()) != Some(id.as_str()))
        .collect();
    storage::save_accounts(&state, filtered)?;
    // Signal any launch already in flight (or still queued behind
    // launch_lock, which a mass-launch can hold for many seconds) to abandon
    // itself, so deleting an account stops a launch that is mid-flight rather
    // than letting it finish and spawn the removed account.
    crate::native::cancel_launch(&state, &id);
    // Drop bookkeeping so watch_tick stops polling a removed account.
    state.account_pids.lock().unwrap().remove(&id);
    state.watched_accounts.lock().unwrap().remove(&id);
    state.miss_counts.lock().unwrap().remove(&id);
    state.auto_relaunch_history.lock().unwrap().remove(&id);
    Ok(true)
}

#[tauri::command]
pub fn accounts_update(
    state: State<AppState>,
    id: String,
    data: Map<String, Value>,
) -> Result<Option<Value>, String> {
    let mut accounts = storage::load_accounts(&state);
    let idx = accounts
        .iter()
        .position(|a| a.get("id").and_then(|v| v.as_str()) == Some(id.as_str()));
    match idx {
        Some(i) => {
            if let Value::Object(existing) = &mut accounts[i] {
                for (k, v) in data {
                    existing.insert(k, v);
                }
            }
            let updated = accounts[i].clone();
            storage::save_accounts(&state, accounts)?;
            Ok(Some(updated))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub fn accounts_reorder(state: State<AppState>, ids: Vec<String>) -> Result<bool, String> {
    let accounts = storage::load_accounts(&state);
    let mut reordered: Vec<Value> = Vec::new();
    for id in &ids {
        if let Some(a) = accounts
            .iter()
            .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
        {
            reordered.push(a.clone());
        }
    }
    let rest: Vec<Value> = accounts
        .into_iter()
        .filter(|a| {
            let aid = a.get("id").and_then(|v| v.as_str()).unwrap_or("");
            !ids.iter().any(|id| id == aid)
        })
        .collect();
    reordered.extend(rest);
    storage::save_accounts(&state, reordered)?;
    Ok(true)
}

// ---- packages ----
#[tauri::command]
pub fn packages_load() -> Vec<Value> {
    storage::load_packages()
}
#[tauri::command]
pub fn packages_save(packages: Vec<Value>) -> bool {
    storage::save_packages(&packages).is_ok()
}

// ---- generated-account history ----
#[tauri::command]
pub fn genhistory_read(state: State<AppState>) -> Vec<Value> {
    storage::read_genhistory(&state)
}
#[tauri::command]
pub fn genhistory_write(state: State<AppState>, list: Vec<Value>) -> bool {
    storage::write_genhistory(&state, list).is_ok()
}
#[tauri::command]
pub fn genhistory_clear() -> bool {
    storage::write_json_array(&crate::paths::genhistory_path(), &[]).is_ok()
}

// ---- fflags / fps ----
#[tauri::command]
pub fn fflag_read() -> Value {
    match crate::native::get_fflag_path() {
        Some(p) if p.exists() => std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| Value::Object(Map::new())),
        _ => Value::Object(Map::new()),
    }
}

#[tauri::command]
pub fn fflag_write(flags: Value) -> bool {
    let Some(p) = crate::native::get_fflag_path() else {
        return false;
    };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(
        &p,
        serde_json::to_string_pretty(&flags).unwrap_or_else(|_| "{}".into()),
    )
    .is_ok()
}

// Compiled once instead of on every read/write. The patterns are constants,
// so Regex::new can't fail on them; doing it per call just re-paid the
// compile and kept an unwrap in the path for no reason.
static FPS_CAP_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
    regex::Regex::new(r#"(?i)<int\s+name="FramerateCap"\s*>(-?\d+)</int>"#).unwrap()
});
static CLOSING_PROPS_RE: once_cell::sync::Lazy<regex::Regex> =
    once_cell::sync::Lazy::new(|| regex::Regex::new(r"</Properties>").unwrap());

fn global_settings_path() -> std::path::PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    std::path::PathBuf::from(home)
        .join("AppData")
        .join("Local")
        .join("Roblox")
        .join("GlobalBasicSettings_13.xml")
}

pub fn set_global_settings_readonly(readonly: bool) {
    let p = global_settings_path();
    if let Ok(meta) = std::fs::metadata(&p) {
        let mut perms = meta.permissions();
        if perms.readonly() != readonly {
            perms.set_readonly(readonly);
            let _ = std::fs::set_permissions(&p, perms);
        }
    }
}

#[tauri::command]
pub fn fps_read() -> i64 {
    if let Some(saved) = load_settings().get("fpsCap").and_then(|v| v.as_i64()) {
        return saved;
    }
    let p = global_settings_path();
    let Ok(xml) = std::fs::read_to_string(&p) else {
        return 30;
    };
    match FPS_CAP_RE.captures(&xml).and_then(|c| c[1].parse::<i64>().ok()) {
        Some(v) if v >= 9999 => 0,
        Some(v) if v > 0 => v,
        _ => 30,
    }
}

#[tauri::command]
pub fn fps_write(cap: f64) -> Value {
    let p = global_settings_path();
    let Ok(xml) = std::fs::read_to_string(&p) else {
        return serde_json::json!({ "ok": false, "error": "GlobalBasicSettings_13.xml not found - launch Roblox once to create it." });
    };
    let raw = cap.round().max(0.0) as i64;
    let mut s = load_settings();
    s.insert("fpsCap".into(), Value::Number(raw.into()));
    save_settings(&s);
    let value = if raw == 0 { 9999 } else { raw };
    let new_xml = if FPS_CAP_RE.is_match(&xml) {
        FPS_CAP_RE
            .replace(&xml, format!(r#"<int name="FramerateCap">{}</int>"#, value))
            .to_string()
    } else {
        CLOSING_PROPS_RE.replace(
            &xml,
            format!("\t\t\t<int name=\"FramerateCap\">{}</int>\n\t\t</Properties>", value),
        )
        .to_string()
    };
    set_global_settings_readonly(false);
    if std::fs::write(&p, new_xml).is_err() {
        return serde_json::json!({ "ok": false, "error": "Failed to write GlobalBasicSettings_13.xml (file may be locked by another app)" });
    }
    set_global_settings_readonly(true);
    serde_json::json!({ "ok": true })
}

// ---- rdd (custom Roblox versions) ----
#[tauri::command]
pub async fn rdd_install(
    app: AppHandle,
    state: State<'_, AppState>,
    hash: String,
) -> Result<Value, String> {
    crate::rdd::install_version(&app, &state, &hash).await
}

#[tauri::command]
pub fn rdd_list_versions() -> Vec<Value> {
    crate::rdd::list_versions()
}

#[tauri::command]
pub fn rdd_delete_version(hash: String) -> Result<bool, String> {
    crate::rdd::delete_version(&hash).map(|_| true)
}

// ---- roblox process / launch ----
#[tauri::command]
pub async fn roblox_get_version(
    app: AppHandle,
    state: State<'_, AppState>,
    channel: Option<String>,
) -> Result<Option<String>, ()> {
    // One retry: covers a transient blip (CDN hiccup, brief DNS failure)
    // instead of leaving the badge stuck on "Not detected" until next launch.
    match crate::roblox_api::get_roblox_version(&state, channel.as_deref()).await {
        Ok(v) => Ok(Some(v)),
        Err(_) => {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            match crate::roblox_api::get_roblox_version(&state, channel.as_deref()).await {
                Ok(v) => Ok(Some(v)),
                Err(e) => {
                    crate::native::emit_log(&app, "warn", "system", &format!("Could not fetch latest Roblox version: {e}"), None);
                    Ok(None)
                }
            }
        }
    }
}

#[tauri::command]
pub async fn roblox_validate_cookie(
    state: State<'_, AppState>,
    cookie: String,
) -> Result<Value, ()> {
    let info = crate::roblox_api::fetch_user_info(&state, &cookie).await;
    Ok(
        serde_json::json!({ "ok": info.ok, "reachable": info.reachable, "username": info.username, "userId": info.user_id, "reason": info.reason }),
    )
}

#[tauri::command]
pub async fn roblox_set_volume(
    app: AppHandle,
    state: State<'_, AppState>,
    percent: f64,
) -> Result<Value, ()> {
    Ok(crate::native::set_roblox_volume(&app, &state, percent).await)
}

#[tauri::command]
pub async fn roblox_align_windows(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Value, ()> {
    match crate::helper::call(&app, &state, "align", crate::helper::DEFAULT_TIMEOUT).await {
        Ok(out) => {
            let count = out.trim().parse::<usize>().unwrap_or(0);
            Ok(serde_json::json!({ "ok": true, "aligned": count }))
        }
        Err(e) => Ok(serde_json::json!({ "ok": false, "error": e })),
    }
}

#[tauri::command]
pub async fn roblox_kill_all(app: AppHandle, state: State<'_, AppState>) -> Result<Value, ()> {
    let accounts = storage::load_accounts(&state);
    let running_names: Vec<String> = state
        .watched_accounts
        .lock()
        .unwrap()
        .keys()
        .map(|id| {
            accounts
                .iter()
                .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
                .and_then(|a| a.get("username"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| id.clone())
        })
        .collect();
    let count = state.watched_accounts.lock().unwrap().len();
    crate::native::emit_log(
        &app,
        "warn",
        "kill",
        &format!(
            "Killing Roblox instances launched by MultiRoblox ({} running: {})",
            count,
            if running_names.is_empty() {
                "none".into()
            } else {
                running_names.join(", ")
            }
        ),
        Some(serde_json::json!({ "count": count, "accounts": running_names })),
    );
    Ok(crate::native::kill_all_roblox(&app, &state).await)
}

#[tauri::command]
pub async fn roblox_kill_one(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Value, ()> {
    let accounts = storage::load_accounts(&state);
    let acct = accounts
        .iter()
        .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
        .cloned()
        .unwrap_or(Value::Null);
    let username = acct
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or(&id)
        .to_string();
    let user_id = acct.get("userId").and_then(|v| v.as_str());
    let pid = state.account_pids.lock().unwrap().get(&id).copied();
    crate::native::emit_log(
        &app,
        "warn",
        "kill",
        &format!("Killed Roblox instance for {}", username),
        Some(
            serde_json::json!({ "accountId": id, "username": username, "userId": user_id, "pid": pid }),
        ),
    );
    Ok(crate::native::kill_account_roblox(&app, &state, &id).await)
}

#[tauri::command]
pub async fn roblox_running_count(app: AppHandle, state: State<'_, AppState>) -> Result<u32, ()> {
    Ok(crate::native::count_roblox_processes(&app, &state).await)
}

// Polled by the frontend to self-heal card state if a push event is missed.
#[tauri::command]
pub async fn roblox_watched_ids(state: State<'_, AppState>) -> Result<Vec<String>, ()> {
    Ok(state
        .watched_accounts
        .lock()
        .unwrap()
        .keys()
        .cloned()
        .collect())
}

#[tauri::command]
pub async fn roblox_trim_memory(app: AppHandle, state: State<'_, AppState>) -> Result<Value, ()> {
    Ok(crate::native::trim_roblox_memory(&app, &state).await)
}

#[tauri::command]
pub async fn roblox_trim_account_memory(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Value, ()> {
    Ok(crate::native::trim_account_memory(&app, &state, &id).await)
}

#[tauri::command]
pub fn roblox_set_account_priority(state: State<AppState>, id: String, priority: String) -> Value {
    crate::native::set_account_priority(&state, &id, &priority)
}

#[tauri::command]
pub async fn roblox_get_game_name(
    state: State<'_, AppState>,
    place_id_or_target: String,
    cookie: String,
) -> Result<Option<String>, ()> {
    Ok(crate::roblox_api::get_game_name(&state, &place_id_or_target, &cookie).await)
}

#[tauri::command]
pub async fn roblox_get_json(state: State<'_, AppState>, url: String) -> Result<Value, String> {
    crate::roblox_api::get_json_public(&state, &url).await
}

// Resolves a username to a "placeId:jobId" target string, which the launch
// path already understands as "join this exact running server".
#[tauri::command]
pub async fn roblox_follow_user(
    state: State<'_, AppState>,
    cookie: String,
    username: String,
) -> Result<Value, String> {
    crate::roblox_api::follow_user_target(&state, &cookie, &username).await
}

#[tauri::command]
pub async fn altgen_generate(
    state: State<'_, AppState>,
    api_key: String,
    quantity: i64,
) -> Result<Value, String> {
    crate::roblox_api::altgen_generate(&state, &api_key, quantity).await
}

#[tauri::command]
pub async fn roblox_launch(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    cookie: String,
    target: Option<String>,
) -> Result<Value, ()> {
    // Registered before queueing on launch_lock so a launch still waiting its
    // turn behind another one can be cancelled as well.
    let cancel = crate::native::register_launch(&state, &id);
    let _guard = state.launch_lock.lock().await;
    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        crate::native::finish_launch(&state, &id);
        return Ok(serde_json::json!({ "success": false, "cancelled": true, "error": "Launch cancelled" }));
    }
    let result =
        crate::native::do_launch(&app, &state, &id, &cookie, target.as_deref().unwrap_or("")).await;
    crate::native::finish_launch(&state, &id);
    Ok(result)
}

#[tauri::command]
pub fn roblox_launch_cancel(state: State<AppState>, id: String) -> Value {
    serde_json::json!({ "ok": crate::native::cancel_launch(&state, &id) })
}

// ---- browser login ----
#[tauri::command]
pub async fn roblox_open_login(app: AppHandle, state: State<'_, AppState>) -> Result<Value, ()> {
    let r = crate::login::open_login(&app, &state).await;
    Ok(
        serde_json::json!({ "success": r.success, "cookie": r.cookie, "username": r.username, "userId": r.user_id, "error": r.error }),
    )
}

#[tauri::command]
pub async fn roblox_login_with_credentials(
    app: AppHandle,
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> Result<Value, ()> {
    let r = crate::login::open_login_with_credentials(&app, &state, &username, &password).await;
    Ok(serde_json::json!({ "success": r.success, "cookie": r.cookie, "username": r.username, "userId": r.user_id, "error": r.error }))
}

#[tauri::command]
pub fn login_cancel(state: State<AppState>) {
    crate::login::cancel_login(&state);
}

#[tauri::command]
pub async fn roblox_open_account_browser(
    app: AppHandle,
    state: State<'_, AppState>,
    cookie: String,
) -> Result<Value, ()> {
    match crate::login::open_account_in_browser(&app, &state, &cookie).await {
        Ok(()) => Ok(serde_json::json!({ "ok": true })),
        Err(e) => Ok(serde_json::json!({ "ok": false, "error": e })),
    }
}

// ---- tracking (screenshots -> Discord webhook) ----
#[tauri::command]
pub async fn tracking_capture_preview(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<Value, ()> {
    match crate::tracking::capture_preview_b64(&app, &state, &id).await {
        Ok(b64) => Ok(serde_json::json!({ "ok": true, "dataUrl": format!("data:image/png;base64,{}", b64) })),
        Err(e) => Ok(serde_json::json!({ "ok": false, "error": e })),
    }
}

// Lets the UI reject a bad URL as it's typed instead of silently failing on
// the next capture. The backend check in capture_and_send is the real gate.
#[tauri::command]
pub fn tracking_validate_webhook(url: String) -> Value {
    match crate::tracking::validate_webhook_url(&url) {
        Ok(()) => serde_json::json!({ "ok": true }),
        Err(e) => serde_json::json!({ "ok": false, "error": e }),
    }
}

fn parse_region(region: &Value) -> Option<(f64, f64, f64, f64)> {
    Some((region.get("x")?.as_f64()?, region.get("y")?.as_f64()?, region.get("w")?.as_f64()?, region.get("h")?.as_f64()?))
}

#[tauri::command]
pub async fn tracking_capture_and_send(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    username: String,
    webhook_url: String,
    regions: Option<Vec<Value>>,
) -> Result<Value, ()> {
    let crops: Vec<(f64, f64, f64, f64)> = regions.unwrap_or_default().iter().filter_map(parse_region).collect();
    match crate::tracking::capture_and_send(&app, &state, &id, &username, &webhook_url, crops).await {
        Ok(()) => Ok(serde_json::json!({ "ok": true })),
        Err(e) => Ok(serde_json::json!({ "ok": false, "error": e })),
    }
}

// Full reset from the lock screen; stops the helper first and waits for it
// to actually exit (it holds its own exe file open, which would otherwise
// make deleting the folder fail on Windows), wipes the entire app-data
// folder, then clears the in-memory state that would otherwise still point
// at it. The frontend only reloads the webview afterwards, not the Rust
// process, so the helper has to be brought back up here.
#[tauri::command]
pub async fn clear_app_data(app: AppHandle, state: State<'_, AppState>) -> Result<Value, ()> {
    crate::native::stop_all_native_helpers(&state).await;
    let dir = crate::paths::app_data_dir();
    let result = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&dir)).await;
    crate::native::reset_state_for_wipe(&state);
    // Re-extracts the embedded helper into the now-empty folder and starts
    // exactly one again, holding the mutex as before.
    crate::native::start_mutex_holder(&app, &state).await;
    match result {
        Ok(Ok(())) => Ok(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Ok(serde_json::json!({ "ok": false, "error": e.to_string() })),
        Err(e) => Ok(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

// ---- misc ----
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(serde::Serialize)]
pub struct UpdateInfo {
    pub has_update: bool,
    pub latest_version: String,
    pub url: String,
}

fn parse_version(v: &str) -> Vec<u32> {
    v.trim_start_matches(|c| c == 'v' || c == 'V')
        .split('.')
        .filter_map(|s| s.split('-').next().unwrap_or("").parse::<u32>().ok())
        .collect()
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    let lat = parse_version(latest);
    let cur = parse_version(current);
    !lat.is_empty() && lat > cur
}

#[tauri::command]
pub async fn check_for_updates(state: State<'_, AppState>) -> Result<UpdateInfo, ()> {
    let current_version = env!("CARGO_PKG_VERSION");
    let resp = state
        .http
        .get("https://api.github.com/repos/sjrom/MultiRoblox-RAM/releases/latest")
        .header("User-Agent", format!("MultiRoblox/{current_version}"))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;

    let res = match resp {
        Ok(r) if r.status().is_success() => r.json::<serde_json::Value>().await.ok(),
        _ => None,
    };

    if let Some(data) = res {
        let tag = data.get("tag_name").and_then(|v| v.as_str()).unwrap_or("");
        let raw_url = data.get("html_url").and_then(|v| v.as_str()).unwrap_or("");
        let url = if raw_url.starts_with("https://github.com/") {
            raw_url
        } else {
            "https://github.com/sjrom/MultiRoblox-RAM/releases/latest"
        };
        if is_newer_version(tag, current_version) {
            return Ok(UpdateInfo {
                has_update: true,
                latest_version: tag.to_string(),
                url: url.to_string(),
            });
        }
    }

    Ok(UpdateInfo {
        has_update: false,
        latest_version: String::new(),
        url: String::new(),
    })
}

#[derive(serde::Serialize)]
pub struct DefenderInfo {
    pub command: String,
    pub app_data_dir: String,
    pub exe_dir: String,
}

#[tauri::command]
pub fn get_defender_info() -> DefenderInfo {
    let app_data = crate::paths::app_data_dir();
    let current_exe = std::env::current_exe().unwrap_or_default();
    let exe_dir = current_exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let exe_name = current_exe
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.to_ascii_lowercase().starts_with("multiroblox"))
        .unwrap_or("MultiRoblox*.exe");

    let app_data_str = app_data.to_string_lossy().to_string();
    let exe_dir_str = exe_dir.to_string_lossy().to_string();

    let command = format!(
        "Add-MpPreference -ExclusionPath '{}', '{}' -ExclusionProcess 'RobloxNative.exe', '{}'",
        exe_dir_str, app_data_str, exe_name
    );

    DefenderInfo {
        command,
        app_data_dir: app_data_str,
        exe_dir: exe_dir_str,
    }
}

#[tauri::command]
pub async fn add_defender_exclusion(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let info = get_defender_info();
        // Visible execution without -WindowStyle Hidden, transparently showing progress and auto-closing.
        let ps_code = format!(
            "& {{ \
                Write-Host 'Adding MultiRoblox exclusions to Windows Defender...' -ForegroundColor Cyan; \
                {}; \
                Write-Host '[OK] Exclusions added successfully! Closing in 2 seconds...' -ForegroundColor Green; \
                Start-Sleep -Seconds 2; \
            }}",
            info.command
        );
        let escaped_ps = ps_code.replace('\'', "''");
        let launcher = format!(
            "Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile -Command {}'",
            escaped_ps
        );

        let status = tokio::task::spawn_blocking(move || {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &launcher])
                .creation_flags(CREATE_NO_WINDOW)
                .status()
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

        if status.success() {
            *state.helper_next_attempt.lock().unwrap() = 0;
            crate::native::start_mutex_holder(&app, &state).await;
            Ok(())
        } else {
            Err(format!("Exit code: {:?}", status.code()))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (&app, &state);
        Ok(())
    }
}

#[tauri::command]
pub fn open_defender_settings() -> Result<(), String> {
    tauri_plugin_opener::open_url("windowsdefender://exclusions", None::<&str>)
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub struct RuntimeEnvInfo {
    pub dotnet_ok: bool,
    pub roblox_installed: bool,
}

pub fn is_dotnet4_installed() -> bool {
    if !cfg!(windows) {
        return true;
    }
    let win = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".to_string());
    let p64 = std::path::Path::new(&win).join(r"Microsoft.NET\Framework64\v4.0.30319\mscorlib.dll");
    let p32 = std::path::Path::new(&win).join(r"Microsoft.NET\Framework\v4.0.30319\mscorlib.dll");
    p64.exists() || p32.exists()
}

#[tauri::command]
pub fn check_runtime_env() -> RuntimeEnvInfo {
    RuntimeEnvInfo {
        dotnet_ok: is_dotnet4_installed(),
        roblox_installed: crate::native::has_roblox_install(),
    }
}

#[cfg(windows)]
pub fn ensure_webview2_runtime() -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;

    #[link(name = "advapi32")]
    extern "system" {
        fn RegGetValueW(
            hkey: isize,
            lpSubKey: *const u16,
            lpValue: *const u16,
            dwFlags: u32,
            pdwType: *mut u32,
            pvData: *mut u16,
            pcbData: *mut u32,
        ) -> i32;
    }
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(
            hWnd: isize,
            lpText: *const u16,
            lpCaption: *const u16,
            uType: u32,
        ) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    const HKEY_LOCAL_MACHINE: isize = -2147483646i32 as isize;
    const HKEY_CURRENT_USER: isize = -2147483647i32 as isize;
    const RRF_RT_REG_SZ: u32 = 0x0000_0002;

    let val_name = wide("pv");
    let keys = [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        ),
        (
            HKEY_CURRENT_USER,
            r"Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        ),
    ];

    for (root, subkey) in keys {
        let sub = wide(subkey);
        let mut buf = [0u16; 64];
        let mut size = (buf.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                root,
                sub.as_ptr(),
                val_name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                &mut size,
            )
        };
        if status == 0 {
            let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
            let ver = String::from_utf16_lossy(&buf[..len]);
            if !ver.is_empty() && ver != "0.0.0.0" {
                return true;
            }
        }
    }

    // MB_YESNO (0x04) | MB_ICONWARNING (0x30) | MB_SYSTEMMODAL (0x1000)
    let title = wide("MultiRoblox - Missing Runtime Component");
    let msg = wide(
        "Microsoft Edge WebView2 Runtime is required to run MultiRoblox.\n\
         MultiRoblox 需要安裝 Microsoft Edge WebView2 Runtime 才能顯示介面。\n\n\
         Would you like to open the official Microsoft download page now?\n\
         是否立即開啟微軟官方下載頁面？",
    );
    let resp = unsafe { MessageBoxW(0, msg.as_ptr(), title.as_ptr(), 0x1034) };
    if resp == 6 {
        // IDYES = 6
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", "https://go.microsoft.com/fwlink/p/?LinkId=2124703"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        assert!(is_newer_version("v1.2.11", "1.2.10"));
        assert!(is_newer_version("1.3.0", "1.2.10"));
        assert!(is_newer_version("2.0.0", "1.2.10"));
        assert!(!is_newer_version("v1.2.10", "1.2.10"));
        assert!(!is_newer_version("v1.2.9", "1.2.10"));
        assert!(!is_newer_version("1.2.9", "1.2.10"));
        assert!(!is_newer_version("", "1.2.10"));
    }
}
