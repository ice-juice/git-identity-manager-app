//! M4 命令层：agent 状态/加载/卸载/清空、免提权 fallback、解锁后自动加载。

use crate::agent::{self, AgentKeyResolved};
use crate::commands::AppState;
use crate::error::{AppError, Result};
use crate::store;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub running: bool,
    /// 当前走的是服务态还是免提权 fallback。
    pub using_fallback: bool,
    pub keys: Vec<AgentKeyResolved>,
}

/// 查询 agent 状态并按指纹反查身份。
#[tauri::command]
pub fn agent_status(state: State<AppState>) -> Result<AgentStatus> {
    let env = state.agent_env.lock().unwrap().clone();
    let using_fallback = env.auth_sock.is_some();
    let agent_keys = match agent::list(&env) {
        Ok(k) => k,
        Err(_) => {
            return Ok(AgentStatus {
                running: false,
                using_fallback,
                keys: vec![],
            })
        }
    };
    // 反查需要 vault 数据（若已解锁）。
    let vault = state.vault.lock().unwrap();
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
        keys: resolved,
    })
}

/// 确保 agent 可用：先探测 Windows OpenSSH，不行则启动 Git 自带 ssh-agent。
#[tauri::command]
pub fn agent_ensure(state: State<AppState>) -> Result<AgentStatus> {
    ready_env(&state)?;
    agent_status(state)
}

/// 复用已就绪的 agent；否则启动系统或 Git ssh-agent 并写回状态。
fn ready_env(state: &State<AppState>) -> Result<crate::agent::AgentEnv> {
    {
        let env = state.agent_env.lock().unwrap().clone();
        if agent::is_ready(&env) {
            return Ok(env);
        }
    }
    let env = agent::ensure()?;
    *state.agent_env.lock().unwrap() = env.clone();
    Ok(env)
}

/// 加载单把密钥。
#[tauri::command]
pub fn agent_load(state: State<AppState>, key_id: String) -> Result<()> {
    let env = ready_env(&state)?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    agent::load_key(v, &env, &key_id)?;
    crate::util::audit(v.root(), &format!("agent 加载密钥 key_id={key_id}"));
    Ok(())
}

/// 按身份加载其绑定的密钥。
#[tauri::command]
pub fn agent_load_identity(state: State<AppState>, identity_id: String) -> Result<()> {
    let env = ready_env(&state)?;
    let vault = state.vault.lock().unwrap();
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
#[tauri::command]
pub fn agent_load_all(state: State<AppState>) -> Result<u32> {
    let env = ready_env(&state)?;
    let vault = state.vault.lock().unwrap();
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
#[tauri::command]
pub fn agent_unload(state: State<AppState>, key_id: String) -> Result<()> {
    let env = ready_env(&state)?;
    let vault = state.vault.lock().unwrap();
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
#[tauri::command]
pub fn agent_clear(state: State<AppState>) -> Result<()> {
    let env = ready_env(&state)?;
    agent::clear(&env)
}
