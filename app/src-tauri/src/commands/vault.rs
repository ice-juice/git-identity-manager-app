//! vault 相关 IPC 命令。

use crate::autostart;
use crate::commands::AppState;
use crate::error::{AppError, Result};
use crate::session;
use crate::vault::crypto::MasterKey;
use crate::vault::header::KdfParams;
use crate::vault::kdf::{calibrate, DEFAULT_TARGET_MS};
use crate::vault::Vault;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub initialized: bool,
    pub unlocked: bool,
    pub workspace_path: Option<String>,
    pub workspace_id: Option<String>,
    pub auto_lock_minutes: u32,
    pub launch_at_login: bool,
    pub grace_days: u32,
    pub grace_active: bool,
    pub grace_expires_at: Option<String>,
    /// `tray` / `quit` / None（每次询问）。
    pub close_action: Option<String>,
    /// 启动云同步未完成时禁止修改。
    pub writes_locked: bool,
    pub startup_note: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitResult {
    pub recovery_key: String,
    pub workspace_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathCheck {
    /// 命中同步盘时的中文告警，None 表示安全。
    pub warning: Option<String>,
}

/// 查询当前状态（前端启动时首先调用）。
#[tauri::command]
pub fn vault_status(state: State<AppState>) -> VaultStatus {
    let cfg = state.config.lock().unwrap();
    let vault = state.vault.lock().unwrap();
    let initialized = cfg
        .workspace_path
        .as_ref()
        .map(|p| Vault::exists(&PathBuf::from(p)))
        .unwrap_or(false);
    let workspace_id = vault.as_ref().map(|v| v.workspace_id().to_string());
    let grace = session::info(workspace_id.as_deref());
    VaultStatus {
        initialized,
        unlocked: vault.as_ref().map(|v| v.is_unlocked()).unwrap_or(false),
        workspace_path: cfg.workspace_path.clone(),
        workspace_id,
        auto_lock_minutes: cfg.auto_lock_minutes,
        launch_at_login: cfg.launch_at_login || autostart::is_enabled(),
        grace_days: cfg.grace_days,
        grace_active: grace.active,
        grace_expires_at: grace.expires_at,
        close_action: cfg.close_action.clone(),
        writes_locked: state.writes_locked.load(std::sync::atomic::Ordering::SeqCst),
        startup_note: state.startup_note.lock().ok().and_then(|n| n.clone()),
    }
}

/// 检测路径是否位于常见同步盘目录下（应避免）。
#[tauri::command]
pub fn check_workspace_path(path: String) -> PathCheck {
    let lower = path.to_lowercase();
    let hits = [
        ("onedrive", "OneDrive"),
        ("dropbox", "Dropbox"),
        ("google drive", "Google Drive"),
        ("googledrive", "Google Drive"),
        ("icloud", "iCloud"),
        ("坚果云", "坚果云"),
        ("nutstore", "坚果云"),
        ("百度网盘", "百度网盘"),
        ("baidunetdisk", "百度网盘"),
    ];
    for (needle, label) in hits {
        if lower.contains(needle) {
            return PathCheck {
                warning: Some(format!(
                    "该路径疑似位于「{}」同步盘内。端到端加密备份应走应用内的云同步通道，请勿让第三方同步盘接管整个工作空间目录。",
                    label
                )),
            };
        }
    }
    PathCheck { warning: None }
}

/// 初始化工作空间。会做 Argon2id 运行时标定（略慢，属正常）。
#[tauri::command]
pub fn vault_init(state: State<AppState>, path: String, password: String) -> Result<InitResult> {
    let root = PathBuf::from(&path);
    let kdf: KdfParams = calibrate(DEFAULT_TARGET_MS);
    let (vault, recovery_key) = Vault::init(&root, &password, kdf)?;
    let workspace_id = vault.workspace_id().to_string();

    // 记录工作空间路径（非机密）。
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.workspace_path = Some(path);
        cfg.save()?;
    }
    *state.vault.lock().unwrap() = Some(vault);
    state.unlock_guard.lock().unwrap().reset();
    grant_grace_if_configured(&state);
    let _ = crate::sys::adopt_ssh_config(&root);

    Ok(InitResult {
        recovery_key,
        workspace_id,
    })
}

/// 将已解锁的 Vault 登记为本机工作空间（初始化或换机恢复后调用）。
pub(crate) fn adopt_unlocked_vault(
    state: &AppState,
    vault: crate::vault::Vault,
    workspace_path: String,
    cloud_sync: Option<crate::sync::s3::S3Config>,
) -> Result<()> {
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.workspace_path = Some(workspace_path.clone());
        if let Some(sync) = cloud_sync {
            cfg.cloud_sync = Some(sync);
        }
        cfg.save()?;
    }
    let root = PathBuf::from(&workspace_path);
    *state.vault.lock().unwrap() = Some(vault);
    state.unlock_guard.lock().unwrap().reset();
    grant_grace_if_configured(state);
    let _ = crate::sys::adopt_ssh_config(&root);
    Ok(())
}

fn ensure_loaded(state: &AppState) -> Result<()> {
    let cfg_path = {
        let cfg = state.config.lock().unwrap();
        cfg.workspace_path.clone()
    };
    let path = cfg_path.ok_or(AppError::NotInitialized)?;
    let mut vault = state.vault.lock().unwrap();
    if vault.is_none() {
        *vault = Some(Vault::load(&PathBuf::from(path))?);
    }
    Ok(())
}

/// 用访问密码解锁（带限速）。
#[tauri::command]
pub fn vault_unlock(app: AppHandle, state: State<AppState>, password: String) -> Result<()> {
    {
        let guard = state.unlock_guard.lock().unwrap();
        if guard.remaining_ms() > 0 {
            return Err(AppError::RateLimited);
        }
    }
    ensure_loaded(&state)?;
    let mut vault = state.vault.lock().unwrap();
    let v = vault.as_mut().ok_or(AppError::NotInitialized)?;
    match v.unlock_with_password(&password) {
        Ok(()) => {
            drop(vault);
            state.unlock_guard.lock().unwrap().reset();
            grant_grace_if_configured(&state);
            begin_write_lock(&state, &app, "正在从云端同步，可浏览、暂不可修改");
            schedule_after_unlock(app);
            Ok(())
        }
        Err(e) => {
            state.unlock_guard.lock().unwrap().record_failure();
            Err(e)
        }
    }
}

/// 用恢复密钥解锁（忘记密码/换机）。
#[tauri::command]
pub fn vault_unlock_recovery(app: AppHandle, state: State<AppState>, recovery_key: String) -> Result<()> {
    ensure_loaded(&state)?;
    let mut vault = state.vault.lock().unwrap();
    let v = vault.as_mut().ok_or(AppError::NotInitialized)?;
    v.unlock_with_recovery(&recovery_key)?;
    drop(vault);
    grant_grace_if_configured(&state);
    begin_write_lock(&state, &app, "正在从云端同步，可浏览、暂不可修改");
    schedule_after_unlock(app);
    Ok(())
}

/// 锁定：清零内存中的 MK。
#[tauri::command]
pub fn vault_lock(state: State<AppState>) {
    lock_in_memory(&state);
}

pub fn lock_in_memory(state: &AppState) {
    if let Some(v) = state.vault.lock().unwrap().as_mut() {
        v.lock();
    }
    crate::commands::clear_reveal_grace(state);
    state.writes_locked.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut n) = state.startup_note.lock() {
        *n = None;
    }
}

/// 按免验证设置尝试静默解锁。`grace_days == 0` 或会话过期时返回 false。
pub fn try_grace_unlock_silent(state: &AppState) -> bool {
    {
        let vault = state.vault.lock().unwrap();
        if vault.as_ref().is_some_and(|v| v.is_unlocked()) {
            return true;
        }
    }
    let days = state.config.lock().unwrap().grace_days;
    if days == 0 {
        return false;
    }
    if ensure_loaded(state).is_err() {
        return false;
    }
    let workspace_id = {
        let vault = state.vault.lock().unwrap();
        match vault.as_ref() {
            Some(v) => v.workspace_id().to_string(),
            None => return false,
        }
    };
    let Ok(mk) = session::try_restore(&workspace_id) else {
        return false;
    };
    {
        let mut vault = state.vault.lock().unwrap();
        let Some(v) = vault.as_mut() else {
            return false;
        };
        if v.unlock_with_master_key(MasterKey::from_bytes(mk)).is_err() {
            return false;
        }
    }
    true
}

/// 修改访问密码（需正确旧密码）。
#[tauri::command]
pub fn change_password(state: State<AppState>, old_password: String, new_password: String) -> Result<()> {
    ensure_loaded(&state)?;
    let mut vault = state.vault.lock().unwrap();
    let v = vault.as_mut().ok_or(AppError::NotInitialized)?;
    v.change_password(&old_password, &new_password)?;
    drop(vault);
    session::clear();
    grant_grace_if_configured(&state);
    Ok(())
}

/// 轮换恢复密钥（需已解锁），返回新恢复密钥。
#[tauri::command]
pub fn rotate_recovery_key(app: AppHandle, state: State<AppState>) -> Result<InitResult> {
    let mut vault = state.vault.lock().unwrap();
    let v = vault.as_mut().ok_or(AppError::Locked)?;
    let recovery_key = v.rotate_recovery_key()?;
    let workspace_id = v.workspace_id().to_string();
    drop(vault);
    crate::sync::scheduler::kick_publish(app);
    Ok(InitResult {
        recovery_key,
        workspace_id,
    })
}

fn grant_grace_if_configured(state: &AppState) {
    let days = state.config.lock().unwrap().grace_days;
    if days == 0 {
        session::clear();
        return;
    }
    let vault = state.vault.lock().unwrap();
    let Some(v) = vault.as_ref() else {
        return;
    };
    if let Ok(mk) = v.master_key_bytes() {
        let _ = session::grant(v.workspace_id(), &mk, days);
    }
}

fn load_agent_best_effort(state: &AppState) {
    {
        let env = state.agent_env.lock().unwrap().clone();
        if !crate::agent::is_ready(&env) {
            if let Ok(started) = crate::agent::ensure() {
                *state.agent_env.lock().unwrap() = started;
            }
        }
    }
    let env = state.agent_env.lock().unwrap().clone();
    let vault = state.vault.lock().unwrap();
    let Some(v) = vault.as_ref() else {
        return;
    };
    if !v.is_unlocked() {
        return;
    }
    if let Ok(data) = crate::store::load_data(v) {
        for identity in &data.identities {
            if let Some(key_id) = &identity.key_id {
                let _ = crate::agent::load_key(v, &env, key_id);
            }
        }
    }
}

/// 启动时尝试用未过期的本机会话解锁并加载 agent。
#[tauri::command]
pub fn vault_try_grace_unlock(app: AppHandle, state: State<AppState>) -> Result<bool> {
    let ok = try_grace_unlock_silent(&state);
    if ok {
        begin_write_lock(&state, &app, "正在从云端同步，可浏览、暂不可修改");
        schedule_after_unlock(app);
    }
    Ok(ok)
}

fn begin_write_lock(state: &AppState, app: &AppHandle, note: &str) {
    state.writes_locked.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut n) = state.startup_note.lock() {
        *n = Some(note.to_string());
    }
    let _ = app.emit(
        "writes-lock",
        serde_json::json!({ "locked": true, "note": note }),
    );
}

fn end_write_lock(state: &AppState, app: &AppHandle) {
    state.writes_locked.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut n) = state.startup_note.lock() {
        *n = None;
    }
    let _ = app.emit("writes-lock", serde_json::json!({ "locked": false, "note": null }));
    let _ = app.emit("startup-ready", serde_json::json!({}));
}

fn set_startup_note(state: &AppState, app: &AppHandle, note: &str) {
    if let Ok(mut n) = state.startup_note.lock() {
        *n = Some(note.to_string());
    }
    let _ = app.emit(
        "writes-lock",
        serde_json::json!({ "locked": true, "note": note }),
    );
}

/// 解锁后的慢活：先拉云端，再补 SSH / 加载 agent。不挡进入主界面。
pub(crate) fn schedule_after_unlock(app: AppHandle) {
    let state = app.state::<AppState>();
    begin_write_lock(&state, &app, "正在从云端同步，可浏览、暂不可修改");
    if state.bootstrap_busy.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        if let Some(p) = state.config.lock().ok().and_then(|c| c.workspace_path.clone()) {
            let _ = crate::sys::adopt_ssh_config(std::path::Path::new(&p));
        }
        set_startup_note(&state, &app, "正在从云端拉取身份数据…");
        let _ = crate::sync::scheduler::run(&app, "startup");
        set_startup_note(&state, &app, "正在补齐 SSH 配置…");
        {
            let vault = match state.vault.lock() {
                Ok(g) => g,
                Err(_) => {
                    state.bootstrap_busy.store(false, std::sync::atomic::Ordering::SeqCst);
                    end_write_lock(&state, &app);
                    return;
                }
            };
            if let Some(v) = vault.as_ref() {
                if v.is_unlocked() {
                    let _ = crate::commands::write::reconcile_ssh_hosts(v);
                }
            }
        }
        set_startup_note(&state, &app, "正在加载 ssh-agent…");
        load_agent_best_effort(&state);
        state.bootstrap_busy.store(false, std::sync::atomic::Ordering::SeqCst);
        let still_unlocked = state
            .vault
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|v| v.is_unlocked()))
            .unwrap_or(false);
        if still_unlocked {
            end_write_lock(&state, &app);
            crate::update::scheduler::kick_after_unlock(app.clone());
        } else {
            state.writes_locked.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    });
}

#[tauri::command]
pub fn set_launch_at_login(state: State<AppState>, enabled: bool) -> Result<()> {
    autostart::set_enabled(enabled)?;
    let mut cfg = state.config.lock().unwrap();
    cfg.launch_at_login = enabled;
    cfg.save()
}

#[tauri::command]
pub fn set_grace_days(state: State<AppState>, days: u32) -> Result<()> {
    let days = session::clamp_days(days);
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.grace_days = days;
        cfg.save()?;
    }
    if days == 0 {
        session::clear();
    } else {
        grant_grace_if_configured(&state);
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryResetReport {
    pub steps: Vec<String>,
}

fn wipe_workspace_files(root: &std::path::Path) -> Vec<String> {
    let mut steps = Vec::new();
    for name in ["vault.json", "data", "keys", "backups", "sync", "ssh-keys", "ssh", "audit.log"] {
        let path = root.join(name);
        if path.is_dir() {
            if std::fs::remove_dir_all(&path).is_ok() {
                steps.push(format!("已删除目录 {}", path.display()));
            }
        } else if path.is_file() && std::fs::remove_file(&path).is_ok() {
            steps.push(format!("已删除 {}", path.display()));
        }
    }
    steps
}

/// 一键清空本机程序状态，回到未初始化。需二次确认：`confirmed=true` 且短语为「清空」。
#[tauri::command]
pub fn factory_reset(
    state: State<AppState>,
    confirmed: bool,
    confirm_phrase: String,
) -> Result<FactoryResetReport> {
    if !confirmed {
        return Err(AppError::Invalid("请先确认要清空还原本程序".into()));
    }
    if confirm_phrase.trim() != "清空" {
        return Err(AppError::Invalid("请输入「清空」以确认不可恢复的还原".into()));
    }

    let mut steps = Vec::new();
    let workspace = state.config.lock().unwrap().workspace_path.clone();

    let agent_snapshot = state.agent_env.lock().ok().map(|g| g.clone());
    if let Some(env) = agent_snapshot {
        if crate::agent::is_ready(&env) {
            let _ = crate::agent::clear(&env);
            steps.push("已清空 ssh-agent 中的密钥".into());
        }
    }
    *state.agent_env.lock().unwrap() = crate::agent::AgentEnv::default();

    match crate::agent::unify::revert() {
        Ok(more) => steps.extend(more),
        Err(e) => steps.push(format!("还原用户环境时部分失败：{e}")),
    }
    match crate::sys::revert_home_ssh_bridge() {
        Ok(more) => steps.extend(more),
        Err(e) => steps.push(format!("还原 ~/.ssh 时部分失败：{e}")),
    }
    let pid = crate::sys::ssh_dir().join("agent").join("git-account-manager.pid");
    if pid.is_file() {
        let _ = std::fs::remove_file(&pid);
        steps.push("已删除本机 Git agent pid 记录".into());
    }

    if let Some(path) = workspace {
        steps.extend(wipe_workspace_files(&PathBuf::from(path)));
    }

    session::clear();
    let _ = autostart::set_enabled(false);
    steps.push("已关闭开机自启动并清除免验证会话".into());

    *state.vault.lock().unwrap() = None;
    {
        let mut cfg = crate::app_config::AppConfig::default();
        cfg.ensure_machine_id();
        cfg.save()?;
        *state.config.lock().unwrap() = cfg;
    }
    state.unlock_guard.lock().unwrap().reset();
    state.writes_locked.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut note) = state.startup_note.lock() {
        *note = None;
    }
    steps.push("已重置本机应用配置，程序回到未初始化状态".into());

    Ok(FactoryResetReport { steps })
}
