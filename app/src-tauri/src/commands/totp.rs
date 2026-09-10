//! TOTP CRUD、生成验证码、导入与密钥取回。

use crate::commands::{ensure_reveal_authorized, ensure_writes_allowed, AppState};
use crate::error::{AppError, Result};
use crate::icons;
use crate::model::{GroupMeta, TotpEntry};
use crate::qrscan;
use crate::store;
use crate::totp;
use crate::util;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TotpList {
    pub entries: Vec<TotpEntry>,
    pub groups: Vec<GroupMeta>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TotpUpsertArgs {
    pub id: Option<String>,
    pub issuer: String,
    pub account: String,
    pub secret: Option<String>,
    pub note: Option<String>,
    pub url: Option<String>,
    pub group: Option<String>,
    pub algorithm: Option<String>,
    pub digits: Option<u8>,
    pub period: Option<u32>,
    pub icon: Option<String>,
    pub sort_order: Option<i32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TotpCode {
    pub code: String,
    pub period: u32,
    pub remaining_seconds: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TotpSecretReveal {
    pub secret_base32: String,
    pub otpauth_uri: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTotpPreview {
    pub issuer: String,
    pub account: String,
    pub algorithm: String,
    pub digits: u8,
    pub period: u32,
    pub suggested_icon: Option<String>,
    /// 来自用户刚粘贴/扫到的 otpauth，不是保险库里已存的种子。
    pub secret: String,
}

fn now() -> String {
    util::now_rfc3339()
}

#[tauri::command]
pub fn totp_list(state: State<AppState>) -> Result<TotpList> {
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let data = store::load_totp(v)?;
    Ok(TotpList {
        entries: data.entries,
        groups: data.groups,
    })
}

fn publish(app: AppHandle) {
    crate::sync::scheduler::kick_publish(app);
}

#[tauri::command]
pub fn totp_add(app: AppHandle, state: State<AppState>, args: TotpUpsertArgs) -> Result<TotpEntry> {
    ensure_writes_allowed(&state)?;
    let secret = args
        .secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Invalid("请填写 TOTP 密钥".into()))?;
    let secret = totp::normalize_secret(secret)?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let mut data = store::load_totp(v)?;
    let mut secrets = store::load_secrets(v)?;
    let now = now();
    let icon = args.icon.clone().or_else(|| icons::suggest_builtin(&args.issuer));
    let entry = TotpEntry {
        id: uuid::Uuid::new_v4().to_string(),
        issuer: args.issuer.trim().to_string(),
        account: args.account.trim().to_string(),
        note: empty_none(args.note),
        url: empty_none(args.url),
        group: empty_none(args.group),
        algorithm: args.algorithm.unwrap_or_else(|| "SHA1".into()),
        digits: args.digits.unwrap_or(6).clamp(6, 8),
        period: args.period.unwrap_or(30).max(1),
        icon,
        sort_order: args.sort_order.unwrap_or(0),
        created_at: now.clone(),
        updated_at: now,
    };
    if entry.issuer.is_empty() || entry.account.is_empty() {
        return Err(AppError::Invalid("平台名与账号不能为空".into()));
    }
    secrets.totp_seeds.insert(entry.id.clone(), secret);
    data.entries.push(entry.clone());
    store::save_totp(v, &data)?;
    store::save_secrets(v, &secrets)?;
    util::audit(v.root(), &format!("新增 TOTP id={}", entry.id));
    drop(vault);
    publish(app);
    Ok(entry)
}

#[tauri::command]
pub fn totp_update(app: AppHandle, state: State<AppState>, args: TotpUpsertArgs) -> Result<TotpEntry> {
    ensure_writes_allowed(&state)?;
    let id = args.id.clone().ok_or_else(|| AppError::Invalid("缺少 id".into()))?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let mut data = store::load_totp(v)?;
    let mut secrets = store::load_secrets(v)?;
    let entry = data
        .entries
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or_else(|| AppError::Invalid("TOTP 条目不存在".into()))?;
    entry.issuer = args.issuer.trim().to_string();
    entry.account = args.account.trim().to_string();
    entry.note = empty_none(args.note);
    entry.url = empty_none(args.url);
    entry.group = empty_none(args.group);
    if let Some(alg) = args.algorithm {
        entry.algorithm = alg;
    }
    if let Some(d) = args.digits {
        entry.digits = d.clamp(6, 8);
    }
    if let Some(p) = args.period {
        entry.period = p.max(1);
    }
    if args.icon.is_some() {
        entry.icon = args.icon;
    }
    if let Some(s) = args.sort_order {
        entry.sort_order = s;
    }
    entry.updated_at = now();
    if let Some(secret) = args.secret.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        secrets.totp_seeds.insert(id.clone(), totp::normalize_secret(secret)?);
    }
    let out = entry.clone();
    store::save_totp(v, &data)?;
    store::save_secrets(v, &secrets)?;
    util::audit(v.root(), &format!("更新 TOTP id={id}"));
    drop(vault);
    publish(app);
    Ok(out)
}

#[tauri::command]
pub fn totp_delete(app: AppHandle, state: State<AppState>, id: String) -> Result<()> {
    ensure_writes_allowed(&state)?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let mut data = store::load_totp(v)?;
    let mut secrets = store::load_secrets(v)?;
    let before = data.entries.len();
    data.entries.retain(|e| e.id != id);
    if data.entries.len() == before {
        return Err(AppError::Invalid("TOTP 条目不存在".into()));
    }
    data.deleted_entries.insert(id.clone(), now());
    secrets.totp_seeds.remove(&id);
    store::save_totp(v, &data)?;
    store::save_secrets(v, &secrets)?;
    util::audit(v.root(), &format!("删除 TOTP id={id}"));
    drop(vault);
    publish(app);
    Ok(())
}

#[tauri::command]
pub fn totp_save_groups(app: AppHandle, state: State<AppState>, groups: Vec<GroupMeta>) -> Result<()> {
    ensure_writes_allowed(&state)?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let mut data = store::load_totp(v)?;
    data.groups = groups;
    store::save_totp(v, &data)?;
    drop(vault);
    publish(app);
    Ok(())
}

#[tauri::command]
pub fn totp_generate_code(state: State<AppState>, id: String, password: Option<String>) -> Result<TotpCode> {
    ensure_reveal_authorized(&state, password.as_deref())?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let data = store::load_totp(v)?;
    let entry = data
        .entries
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| AppError::Invalid("TOTP 条目不存在".into()))?;
    let secrets = store::load_secrets(v)?;
    let secret = secrets
        .totp_seeds
        .get(&id)
        .ok_or_else(|| AppError::Invalid("缺少 TOTP 种子".into()))?;
    let unix = totp::now_unix();
    let code = totp::generate_code(secret, &entry.algorithm, entry.digits, entry.period, unix)?;
    util::audit(v.root(), &format!("查看 TOTP 验证码 id={id}"));
    Ok(TotpCode {
        code,
        period: entry.period,
        remaining_seconds: totp::remaining_seconds(entry.period, unix),
    })
}

#[tauri::command]
pub fn totp_parse_uri(uri: String) -> Result<ParsedTotpPreview> {
    let p = totp::parse_otpauth(&uri)?;
    Ok(preview_from_parsed(&p))
}

#[tauri::command]
pub fn totp_import_from_image(path: String) -> Result<ParsedTotpPreview> {
    let bytes = std::fs::read(&path)?;
    let p = qrscan::import_from_image(&bytes)?;
    Ok(preview_from_parsed(&p))
}

#[tauri::command]
pub fn totp_scan_screen() -> Result<Vec<qrscan::ScreenHit>> {
    qrscan::scan_screen()
}

#[tauri::command]
pub fn totp_reveal_secret(state: State<AppState>, id: String, password: String) -> Result<TotpSecretReveal> {
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    v.verify_password(&password)?;
    let data = store::load_totp(v)?;
    let entry = data
        .entries
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| AppError::Invalid("TOTP 条目不存在".into()))?;
    let secrets = store::load_secrets(v)?;
    let secret = secrets
        .totp_seeds
        .get(&id)
        .cloned()
        .ok_or_else(|| AppError::Invalid("缺少 TOTP 种子".into()))?;
    let otpauth_uri = totp::build_otpauth(entry, &secret);
    util::audit(v.root(), &format!("取回 TOTP 原始密钥 id={id}"));
    Ok(TotpSecretReveal {
        secret_base32: secret,
        otpauth_uri,
    })
}

#[tauri::command]
pub fn totp_export_qr(state: State<AppState>, id: String, password: String) -> Result<String> {
    let revealed = totp_reveal_secret(state, id, password)?;
    qrscan::render_otpauth_png_b64(&revealed.otpauth_uri)
}

fn preview_from_parsed(p: &totp::ParsedOtpauth) -> ParsedTotpPreview {
    ParsedTotpPreview {
        suggested_icon: icons::suggest_builtin(&p.issuer),
        issuer: p.issuer.clone(),
        account: p.account.clone(),
        algorithm: p.algorithm.clone(),
        digits: p.digits,
        period: p.period,
        secret: p.secret.clone(),
    }
}

fn empty_none(s: Option<String>) -> Option<String> {
    s.map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
}
