//! 隐私账号 CRUD、密码 reveal 与历史版本。

use crate::app_config;
use crate::commands::{ensure_reveal_authorized, ensure_writes_allowed, AppState};
use crate::error::{AppError, Result};
use crate::icons;
use crate::model::{AccountEntry, AccountSecret, GroupMeta, PasswordHistoryItem};
use crate::store;
use crate::util;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

fn publish(app: AppHandle) {
    crate::sync::scheduler::kick_publish(app);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountList {
    pub entries: Vec<AccountEntry>,
    pub groups: Vec<GroupMeta>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUpsertArgs {
    pub id: Option<String>,
    pub platform: String,
    pub username: String,
    pub password: Option<String>,
    pub display_name: Option<String>,
    pub url: Option<String>,
    pub note: Option<String>,
    pub group: Option<String>,
    pub tags: Option<Vec<String>>,
    pub icon: Option<String>,
    pub pinned: Option<bool>,
    pub sort_order: Option<i32>,
    pub totp_ref: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMeta {
    pub index: usize,
    pub replaced_at: String,
}

fn now() -> String {
    util::now_rfc3339()
}

#[tauri::command]
pub fn account_list(state: State<AppState>) -> Result<AccountList> {
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let data = store::load_accounts(v)?;
    let secrets = store::load_secrets(v)?;
    let entries = data
        .entries
        .into_iter()
        .map(|mut e| {
            e.has_password = secrets
                .account_secrets
                .get(&e.id)
                .map(|s| !s.password.is_empty())
                .unwrap_or(false);
            e
        })
        .collect();
    Ok(AccountList {
        entries,
        groups: data.groups,
    })
}

fn missing_password() -> AppError {
    AppError::Other("这条账号的密码已丢失，请编辑并重新填入密码。".into())
}

#[tauri::command]
pub fn account_add(app: AppHandle, state: State<AppState>, args: AccountUpsertArgs) -> Result<AccountEntry> {
    ensure_writes_allowed(&state)?;
    let password = args
        .password
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Invalid("请填写密码".into()))?
        .to_string();
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let mut data = store::load_accounts(v)?;
    let mut secrets = store::load_secrets(v)?;
    let platform = args.platform.trim().to_string();
    let username = args.username.trim().to_string();
    if platform.is_empty() || username.is_empty() {
        return Err(AppError::Invalid("平台与用户名不能为空".into()));
    }
    let now = now();
    let icon = args.icon.clone().or_else(|| icons::suggest_builtin(&platform));
    let entry = AccountEntry {
        id: uuid::Uuid::new_v4().to_string(),
        platform,
        username,
        display_name: empty_none(args.display_name),
        url: empty_none(args.url),
        note: empty_none(args.note),
        group: empty_none(args.group),
        tags: args.tags.unwrap_or_default(),
        icon,
        pinned: args.pinned.unwrap_or(false),
        sort_order: args.sort_order.unwrap_or(0),
        totp_ref: empty_none(args.totp_ref),
        last_used_at: None,
        created_at: now.clone(),
        updated_at: now,
        has_password: true,
    };
    secrets.account_secrets.insert(
        entry.id.clone(),
        AccountSecret {
            password,
            extra_fields: Default::default(),
            history: vec![],
        },
    );
    data.entries.push(entry.clone());
    store::save_secrets(v, &secrets)?;
    store::save_accounts(v, &data)?;
    util::audit(v.root(), &format!("新增隐私账号 id={}", entry.id));
    drop(vault);
    publish(app);
    Ok(entry)
}

#[tauri::command]
pub fn account_update(app: AppHandle, state: State<AppState>, args: AccountUpsertArgs) -> Result<AccountEntry> {
    ensure_writes_allowed(&state)?;
    let id = args.id.clone().ok_or_else(|| AppError::Invalid("缺少 id".into()))?;
    let limit = {
        let cfg = state.config.lock().unwrap();
        app_config::clamp_account_history_limit(cfg.account_history_limit)
    };
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let mut data = store::load_accounts(v)?;
    let mut secrets = store::load_secrets(v)?;
    let entry = data
        .entries
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or_else(|| AppError::Invalid("账号不存在".into()))?;
    entry.platform = args.platform.trim().to_string();
    entry.username = args.username.trim().to_string();
    entry.display_name = empty_none(args.display_name);
    entry.url = empty_none(args.url);
    entry.note = empty_none(args.note);
    entry.group = empty_none(args.group);
    if let Some(tags) = args.tags {
        entry.tags = tags;
    }
    if args.icon.is_some() {
        entry.icon = args.icon;
    }
    if let Some(p) = args.pinned {
        entry.pinned = p;
    }
    if let Some(s) = args.sort_order {
        entry.sort_order = s;
    }
    entry.totp_ref = empty_none(args.totp_ref);
    entry.updated_at = now();

    if let Some(new_pw) = args.password.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let secret = secrets.account_secrets.entry(id.clone()).or_default();
        if secret.password != new_pw {
            if !secret.password.is_empty() {
                secret.history.insert(
                    0,
                    PasswordHistoryItem {
                        password: secret.password.clone(),
                        replaced_at: now(),
                    },
                );
                if secret.history.len() > limit as usize {
                    secret.history.truncate(limit as usize);
                }
            }
            secret.password = new_pw.to_string();
        }
    } else if secrets
        .account_secrets
        .get(&id)
        .map(|s| s.password.is_empty())
        .unwrap_or(true)
    {
        return Err(AppError::Invalid("这条记录没有密码，请重新填入".into()));
    }
    let mut out = entry.clone();
    out.has_password = secrets
        .account_secrets
        .get(&id)
        .map(|s| !s.password.is_empty())
        .unwrap_or(false);
    store::save_secrets(v, &secrets)?;
    store::save_accounts(v, &data)?;
    util::audit(v.root(), &format!("更新隐私账号 id={id}"));
    drop(vault);
    publish(app);
    Ok(out)
}

#[tauri::command]
pub fn account_delete(app: AppHandle, state: State<AppState>, id: String) -> Result<()> {
    ensure_writes_allowed(&state)?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let mut data = store::load_accounts(v)?;
    let mut secrets = store::load_secrets(v)?;
    let before = data.entries.len();
    data.entries.retain(|e| e.id != id);
    if data.entries.len() == before {
        return Err(AppError::Invalid("账号不存在".into()));
    }
    data.deleted_entries.insert(id.clone(), now());
    secrets.account_secrets.remove(&id);
    store::save_secrets(v, &secrets)?;
    store::save_accounts(v, &data)?;
    util::audit(v.root(), &format!("删除隐私账号 id={id}"));
    drop(vault);
    publish(app);
    Ok(())
}

#[tauri::command]
pub fn account_save_groups(app: AppHandle, state: State<AppState>, groups: Vec<GroupMeta>) -> Result<()> {
    ensure_writes_allowed(&state)?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let mut data = store::load_accounts(v)?;
    data.groups = groups;
    store::save_accounts(v, &data)?;
    drop(vault);
    publish(app);
    Ok(())
}

#[tauri::command]
pub fn account_reveal_password(state: State<AppState>, id: String, password: Option<String>) -> Result<String> {
    ensure_reveal_authorized(&state, password.as_deref())?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let secrets = store::load_secrets(v)?;
    let pw = secrets
        .account_secrets
        .get(&id)
        .map(|s| s.password.clone())
        .filter(|s| !s.is_empty())
        .ok_or_else(missing_password)?;
    util::audit(v.root(), &format!("查看账号密码 id={id}"));
    Ok(pw)
}

#[tauri::command]
pub fn account_touch(state: State<AppState>, id: String) -> Result<()> {
    if state.writes_locked.load(std::sync::atomic::Ordering::SeqCst) {
        return Ok(());
    }
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let mut data = store::load_accounts(v)?;
    if let Some(e) = data.entries.iter_mut().find(|e| e.id == id) {
        e.last_used_at = Some(now());
        store::save_accounts(v, &data)?;
    }
    Ok(())
}

#[tauri::command]
pub fn account_history_list(state: State<AppState>, id: String) -> Result<Vec<HistoryMeta>> {
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let secrets = store::load_secrets(v)?;
    let items = secrets
        .account_secrets
        .get(&id)
        .map(|s| {
            s.history
                .iter()
                .enumerate()
                .map(|(index, h)| HistoryMeta {
                    index,
                    replaced_at: h.replaced_at.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(items)
}

#[tauri::command]
pub fn account_reveal_history(
    state: State<AppState>,
    id: String,
    index: usize,
    password: Option<String>,
) -> Result<String> {
    ensure_reveal_authorized(&state, password.as_deref())?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let secrets = store::load_secrets(v)?;
    let item = secrets
        .account_secrets
        .get(&id)
        .and_then(|s| s.history.get(index))
        .ok_or_else(|| AppError::Invalid("历史版本不存在".into()))?;
    util::audit(v.root(), &format!("查看账号历史密码 id={id} index={index}"));
    Ok(item.password.clone())
}

#[tauri::command]
pub fn account_rollback_history(app: AppHandle, state: State<AppState>, id: String, index: usize) -> Result<()> {
    ensure_writes_allowed(&state)?;
    let limit = {
        let cfg = state.config.lock().unwrap();
        app_config::clamp_account_history_limit(cfg.account_history_limit)
    };
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let mut secrets = store::load_secrets(v)?;
    let mut data = store::load_accounts(v)?;
    let secret = secrets
        .account_secrets
        .get_mut(&id)
        .ok_or_else(|| AppError::Invalid("账号机密不存在".into()))?;
    if index >= secret.history.len() {
        return Err(AppError::Invalid("历史版本不存在".into()));
    }
    let old = secret.history[index].password.clone();
    if !secret.password.is_empty() {
        secret.history.insert(
            0,
            PasswordHistoryItem {
                password: secret.password.clone(),
                replaced_at: now(),
            },
        );
    }
    secret.password = old;
    if secret.history.len() > limit as usize {
        secret.history.truncate(limit as usize);
    }
    if let Some(e) = data.entries.iter_mut().find(|e| e.id == id) {
        e.updated_at = now();
    }
    store::save_secrets(v, &secrets)?;
    store::save_accounts(v, &data)?;
    util::audit(v.root(), &format!("回滚账号密码 id={id}"));
    drop(vault);
    publish(app);
    Ok(())
}

#[tauri::command]
pub fn account_clear_history(app: AppHandle, state: State<AppState>, id: String) -> Result<()> {
    ensure_writes_allowed(&state)?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let mut secrets = store::load_secrets(v)?;
    if let Some(s) = secrets.account_secrets.get_mut(&id) {
        s.history.clear();
        store::save_secrets(v, &secrets)?;
        util::audit(v.root(), &format!("清空账号密码历史 id={id}"));
        drop(vault);
        publish(app);
    }
    Ok(())
}

fn empty_none(s: Option<String>) -> Option<String> {
    s.map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
}
