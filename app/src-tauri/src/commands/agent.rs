//! M4 命令层：agent 状态/加载/卸载/清空、Git agent 统一环境、解锁后自动加载。

use crate::agent::{self, unify, AgentKeyResolved};
use crate::commands::{recover_lock, AppState};
use crate::error::{AppError, Result};
use crate::store;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub running: bool,
    /// 当前走的是 Git 自带 ssh-agent（带 SSH_AUTH_SOCK）。
    pub using_fallback: bool,
    pub ssh: Option<String>,
    pub ssh_add: Option<String>,
    pub auth_sock: Option<String>,
    pub keys: Vec<AgentKeyResolved>,
    pub unify: unify::AgentUnifyStatus,
}

/// 查询 agent 状态并按指纹反查身份。
#[tauri::command(async)]
pub fn agent_status(state: State<'_, AppState>) -> Result<AgentStatus> {
    let env = recover_lock(&state.agent_env).clone();
    let using_fallback = env.auth_sock.is_some();
    let agent_keys = match agent::list(&env) {
        Ok(k) => k,
        Err(_) => {
            return Ok(AgentStatus {
                running: false,
                using_fallback,
                ssh: env.ssh.clone(),
                ssh_add: env.ssh_add.clone(),
                auth_sock: env.auth_sock.clone(),
                keys: vec![],
                unify: unify::inspect(&env, false),
            })
        }
    };
    let unify_status = unify::inspect(&env, true);
    // 反查需要 vault 数据（若已解锁）。
    let vault = recover_lock(&state.vault);
    let resolved = match vault.as_ref() {
        Some(v) if v.is_unlocked() => {
            let data = store::load_data(v)?;
            agent::reverse_lookup(&agent_keys, &data.keys, &data.identities)
        }
        _ => agent_keys
            .into_iter()
            .map(|ak| AgentKeyResolved {
                agent: ak,
                identity_name: None,
                key_name: None,
            })
            .collect(),
    };
    Ok(AgentStatus {
        running: true,
        using_fallback,
        ssh: env.ssh.clone(),
        ssh_add: env.ssh_add.clone(),
        auth_sock: env.auth_sock.clone(),
        keys: resolved,
        unify: unify_status,
    })
}

/// 确保 Git 自带 ssh-agent 可用。
#[tauri::command(async)]
pub fn agent_ensure(state: State<'_, AppState>) -> Result<AgentStatus> {
    ready_env(&state)?;
    agent_status(state)
}

/// 复用已就绪的 Git agent；否则启动并写回状态。
fn ready_env(state: &State<AppState>) -> Result<crate::agent::AgentEnv> {
    {
        let env = recover_lock(&state.agent_env).clone();
        if env.auth_sock.is_some() && agent::is_ready(&env) {
            return Ok(env);
        }
    }
    let env = agent::ensure()?;
    *recover_lock(&state.agent_env) = env.clone();
    Ok(env)
}

/// 启动时只拉起 Git ssh-agent，不改用户环境变量（写入必须经前端二次确认）。
/// 必须在后台线程执行：`ssh-add`/`ssh-agent` 在异常套接字上可能长时间不返回，
/// 放在 Tauri setup 主线程会冻住窗口消息泵，表现为白屏 +「未响应」。
pub fn bootstrap_git_agent(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("gam-agent-bootstrap".into())
        .spawn(move || {
            match agent::ensure() {
                Ok(env) => {
                    let state = app.state::<AppState>();
                    *recover_lock(&state.agent_env) = env.clone();
                    log::info!(
                        "Git ssh-agent 已就绪：sock={}",
                        env.auth_sock.as_deref().unwrap_or("-")
                    );
                }
                Err(e) => log::warn!("启动 Git ssh-agent 失败：{e}"),
            }
        })
        .ok();
}

/// 写入 git config / 用户环境 / 终端 profile。`confirmed` 必须为 true。
#[tauri::command(async)]
pub fn agent_unify_env(state: State<'_, AppState>, confirmed: bool) -> Result<unify::AgentUnifyReport> {
    if !confirmed {
        return Err(AppError::Invalid(
            "未确认写入用户环境，已取消。不会修改系统环境变量。".into(),
        ));
    }
    let env = ready_env(&state)?;
    let sock = env
        .auth_sock
        .as_deref()
        .ok_or_else(|| AppError::Other("Git ssh-agent 尚未运行，请先点「确保运行」。".into()))?;
    if !agent::auth_sock_target_exists(sock) {
        return Err(AppError::Other(
            "SSH_AUTH_SOCK 指向的套接字不存在，已拒绝写入用户环境。请先点「确保运行」。".into(),
        ));
    }
    unify::apply(&env)
}

/// 加载单把密钥。
#[tauri::command(async)]
pub fn agent_load(state: State<'_, AppState>, key_id: String) -> Result<()> {
    let env = ready_env(&state)?;
    let vault = recover_lock(&state.vault);
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    agent::load_key(v, &env, &key_id)?;
    crate::util::audit(v.root(), &format!("agent 加载密钥 key_id={key_id}"));
    Ok(())
}

/// 按身份加载其绑定的密钥。
#[tauri::command(async)]
pub fn agent_load_identity(state: State<'_, AppState>, identity_id: String) -> Result<()> {
    let env = ready_env(&state)?;
    let vault = recover_lock(&state.vault);
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    let data = store::load_data(v)?;
    let identity = data
        .identities
        .iter()
        .find(|i| i.id == identity_id)
        .ok_or_else(|| AppError::Invalid("身份不存在".into()))?;
    let key_id = identity
        .key_id
        .clone()
        .ok_or_else(|| AppError::Invalid("该身份未绑定密钥".into()))?;
    agent::load_key(v, &env, &key_id)?;
    Ok(())
}

/// 解锁后自动加载所有身份的密钥（状态灯转绿）。
#[tauri::command(async)]
pub fn agent_load_all(state: State<'_, AppState>) -> Result<u32> {
    let env = ready_env(&state)?;
    let vault = recover_lock(&state.vault);
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    let data = store::load_data(v)?;
    let mut loaded = 0u32;
    for identity in &data.identities {
        if let Some(key_id) = &identity.key_id {
            if agent::load_key(v, &env, key_id).is_ok() {
                loaded += 1;
            }
        }
    }
    crate::util::audit(v.root(), &format!("agent 自动加载 {loaded} 把密钥"));
    Ok(loaded)
}

/// 卸载某把密钥（按 key_id 找公钥再 ssh-add -d）。
#[tauri::command(async)]
pub fn agent_unload(state: State<'_, AppState>, key_id: String) -> Result<()> {
    let env = ready_env(&state)?;
    let vault = recover_lock(&state.vault);
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    let data = store::load_data(v)?;
    let record = data
        .keys
        .iter()
        .find(|k| k.id == key_id)
        .ok_or_else(|| AppError::Invalid("密钥不存在".into()))?;
    agent::unload_public(&env, &record.public_openssh)
}

/// 清空 agent 全部密钥。
#[tauri::command(async)]
pub fn agent_clear(state: State<'_, AppState>) -> Result<()> {
    let env = ready_env(&state)?;
    agent::clear(&env)
}
