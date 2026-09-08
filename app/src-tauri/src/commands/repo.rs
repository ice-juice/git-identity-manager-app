//! M5 命令层：地址解析/身份推断、仓库扫描/体检/切换、归属标识管理、PAT 上传公钥。

use crate::commands::AppState;
use crate::error::{AppError, Result};
use crate::git::infer::{infer, Inference};
use crate::git::url::parse_repo_url;
use crate::git::{github, repo};
use crate::store;
use crate::sys;
use crate::vault::Vault;
use serde::Serialize;
use std::path::PathBuf;
use tauri::State;

fn with_vault<T>(state: &State<AppState>, f: impl FnOnce(&Vault) -> Result<T>) -> Result<T> {
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    f(v)
}

/// 解析地址并做本地身份推断（纯本地查表，零网络）。
#[tauri::command]
pub fn resolve_url(state: State<AppState>, url: String) -> Result<Inference> {
    let parsed = parse_repo_url(&url)?;
    with_vault(&state, |v| {
        let data = store::load_data(v)?;
        Ok(infer(&parsed, &data.identities, &data.clone_history))
    })
}

/// 扫描根目录下的仓库并逐个体检。
#[tauri::command]
pub fn scan_repos(state: State<AppState>, root: String, max_depth: Option<usize>) -> Result<Vec<repo::RepoInfo>> {
    let depth = max_depth.unwrap_or(5);
    with_vault(&state, |v| {
        let data = store::load_data(v)?;
        let repos = repo::scan(&PathBuf::from(&root), depth);
        Ok(repos
            .iter()
            .map(|p| repo::inspect(p, &data.identities, &data.clone_history))
            .collect())
    })
}

/// 给身份追加一条归属标识（去重、小写化）。
#[tauri::command]
pub fn add_owner(state: State<AppState>, identity_id: String, owner: String) -> Result<()> {
    with_vault(&state, |v| {
        let mut data = store::load_data(v)?;
        let owner_lc = owner.trim().to_lowercase();
        let id = data
            .identities
            .iter_mut()
            .find(|i| i.id == identity_id)
            .ok_or_else(|| AppError::Invalid("身份不存在".into()))?;
        if !id.owners.iter().any(|o| o.eq_ignore_ascii_case(&owner_lc)) {
            id.owners.push(owner_lc);
        }
        store::save_data(v, &data)?;
        Ok(())
    })
}

/// 切换某仓库的身份：改 remote 为别名地址 + 设提交身份 + 学习归属。
#[tauri::command]
pub fn switch_repo_identity(state: State<AppState>, repo_path: String, identity_id: String) -> Result<String> {
    with_vault(&state, |v| {
        let mut data = store::load_data(v)?;
        let identity = data
            .identities
            .iter()
            .find(|i| i.id == identity_id)
            .cloned()
            .ok_or_else(|| AppError::Invalid("身份不存在".into()))?;

        // 读当前 remote，解析出 repo_path。
        let (out, _e, code) = sys::run("git", &["-C", &repo_path, "remote", "get-url", "origin"])?;
        if code != 0 {
            return Err(AppError::Invalid("无法读取该仓库的 origin".into()));
        }
        let parsed = parse_repo_url(out.trim())?;
        let new_url = crate::git::url::rewrite_to_alias(&identity.host_alias, &parsed.repo_path);

        repo::switch_identity(
            &repo_path,
            &new_url,
            identity.git_user_name.as_deref(),
            identity.email.as_deref(),
        )?;

        // 学习 owner → identity。
        data.clone_history
            .insert(parsed.owner.to_lowercase(), identity.id.clone());
        store::save_data(v, &data)?;
        crate::util::audit(v.root(), &format!("切换仓库身份 {repo_path} → {}", identity.name));
        Ok(new_url)
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneResult {
    pub dest: String,
    pub used_url: String,
    pub identity_name: String,
}

/// 克隆仓库：用别名地址克隆，克隆后写入仓库级提交身份并学习归属。
#[tauri::command]
pub fn clone_repo(
    state: State<AppState>,
    url: String,
    dest_dir: String,
    identity_id: String,
) -> Result<CloneResult> {
    let parsed = parse_repo_url(&url)?;
    with_vault(&state, |v| {
        let mut data = store::load_data(v)?;
        let identity = data
            .identities
            .iter()
            .find(|i| i.id == identity_id)
            .cloned()
            .ok_or_else(|| AppError::Invalid("身份不存在".into()))?;
        let used_url = crate::git::url::rewrite_to_alias(&identity.host_alias, &parsed.repo_path);
        let target = PathBuf::from(&dest_dir).join(&parsed.repo);
        let target_str = target.display().to_string();

        let (_o, e, code) = sys::run("git", &["clone", &used_url, &target_str])?;
        if code != 0 {
            return Err(AppError::Other(format!("git clone 失败：{e}")));
        }
        // 写仓库级提交身份。
        repo::switch_identity(
            &target_str,
            &used_url,
            identity.git_user_name.as_deref(),
            identity.email.as_deref(),
        )?;
        data.clone_history
            .insert(parsed.owner.to_lowercase(), identity.id.clone());
        store::save_data(v, &data)?;
        crate::util::audit(v.root(), &format!("克隆 {} 使用身份 {}", parsed.repo_path, identity.name));

        Ok(CloneResult {
            dest: target_str,
            used_url,
            identity_name: identity.name,
        })
    })
}

/// 保存 GitHub PAT（存入 vault，绝不落明文）。
#[tauri::command]
pub fn set_github_pat(state: State<AppState>, token: String) -> Result<()> {
    with_vault(&state, |v| {
        let mut secrets = store::load_secrets(v)?;
        secrets.github_pat = Some(token);
        store::save_secrets(v, &secrets)?;
        crate::util::audit(v.root(), "保存 GitHub PAT");
        Ok(())
    })
}

/// 校验 PAT 并返回账号名。
#[tauri::command]
pub fn test_github_pat(state: State<AppState>) -> Result<String> {
    with_vault(&state, |v| {
        let secrets = store::load_secrets(v)?;
        let token = secrets
            .github_pat
            .ok_or_else(|| AppError::Invalid("尚未配置 PAT".into()))?;
        github::whoami(&token)
    })
}

/// 拉取 PAT 账号所属组织（供批量导入归属标识）。
#[tauri::command]
pub fn list_github_orgs(state: State<AppState>) -> Result<Vec<String>> {
    with_vault(&state, |v| {
        let secrets = store::load_secrets(v)?;
        let token = secrets
            .github_pat
            .ok_or_else(|| AppError::Invalid("尚未配置 PAT".into()))?;
        github::list_orgs(&token)
    })
}

/// 用 PAT 上传某把密钥的公钥到 GitHub。
#[tauri::command]
pub fn upload_public_key(state: State<AppState>, key_id: String, title: String) -> Result<()> {
    with_vault(&state, |v| {
        let secrets = store::load_secrets(v)?;
        let token = secrets
            .github_pat
            .ok_or_else(|| AppError::Invalid("尚未配置 PAT，可改用复制公钥手动添加".into()))?;
        let data = store::load_data(v)?;
        let record = data
            .keys
            .iter()
            .find(|k| k.id == key_id)
            .ok_or_else(|| AppError::Invalid("密钥不存在".into()))?;
        github::upload_public_key(&token, &title, &record.public_openssh)?;
        crate::util::audit(v.root(), &format!("PAT 上传公钥 {}", record.name));
        Ok(())
    })
}
