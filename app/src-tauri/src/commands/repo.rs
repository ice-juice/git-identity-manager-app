//! M5 命令层：地址解析/身份推断、仓库扫描/体检/切换、归属标识管理、PAT 上传公钥。

use crate::agent::AgentEnv;
use crate::commands::{recover_lock, AppState};
use crate::error::{AppError, Result};
use crate::git::infer::{infer, Inference};
use crate::git::url::parse_repo_url;
use crate::git::{github, repo};
use crate::model::ManagedRepo;
use crate::store;
use crate::sys;
use crate::vault::Vault;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use tauri::{AppHandle, State};

fn with_vault<T>(state: &State<AppState>, f: impl FnOnce(&Vault) -> Result<T>) -> Result<T> {
    let vault = recover_lock(&state.vault);
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

fn iso_now() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRepoView {
    pub id: String,
    pub path: String,
    pub name: String,
    pub remote_url: Option<String>,
    pub identity_id: Option<String>,
    pub identity_name: Option<String>,
    pub added_at: String,
    pub source: String,
    pub exists: bool,
    pub current_alias: Option<String>,
    pub needs_alias_fix: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportScanResult {
    pub imported: u32,
    pub updated: u32,
    pub skipped_no_remote: u32,
    pub repos: Vec<ManagedRepoView>,
}

fn identity_name_of(data: &crate::model::VaultData, id: Option<&str>) -> Option<String> {
    id.and_then(|iid| {
        data.identities
            .iter()
            .find(|i| i.id == iid)
            .map(|i| i.name.clone())
    })
}

fn to_view(data: &crate::model::VaultData, rec: &ManagedRepo) -> ManagedRepoView {
    let exists = PathBuf::from(&rec.path).is_dir();
    let mut current_alias = None;
    let mut needs_alias_fix = false;
    let mut remote_url = rec.remote_url.clone();
    if exists {
        let live = repo::inspect(std::path::Path::new(&rec.path), &data.identities, &data.clone_history);
        if live.remote_url.is_some() {
            remote_url = live.remote_url;
        }
        current_alias = live.current_alias;
        needs_alias_fix = live.needs_alias_fix;
    }
    ManagedRepoView {
        id: rec.id.clone(),
        path: rec.path.clone(),
        name: rec.name.clone(),
        remote_url,
        identity_id: rec.identity_id.clone(),
        identity_name: identity_name_of(data, rec.identity_id.as_deref()),
        added_at: rec.added_at.clone(),
        source: rec.source.clone(),
        exists,
        current_alias,
        needs_alias_fix,
    }
}

fn current_machine_id() -> String {
    crate::app_config::AppConfig::current_machine_id()
}

fn upsert_from_info(
    data: &mut crate::model::VaultData,
    info: &repo::RepoInfo,
    source: &str,
) -> (bool, String) {
    let inferred_id = info.inferred_identity.as_ref().and_then(|name| {
        data.identities
            .iter()
            .find(|i| i.name == *name)
            .map(|i| i.id.clone())
    });
    repo::upsert_managed_repo(
        &mut data.repos,
        ManagedRepo {
            id: uuid::Uuid::new_v4().to_string(),
            path: info.path.clone(),
            name: repo::display_repo_name(&info.path),
            remote_url: info.remote_url.clone(),
            identity_id: inferred_id,
            added_at: iso_now(),
            source: source.into(),
            machine_id: current_machine_id(),
        },
    )
}

fn set_origin_url(repo_path: &str, url: &str) -> Result<()> {
    let (_o, _e, code) = sys::run("git", &["-C", repo_path, "remote", "get-url", "origin"])?;
    let (args, fail): (Vec<&str>, &str) = if code == 0 {
        (vec!["-C", repo_path, "remote", "set-url", "origin", url], "更新 origin 失败")
    } else {
        (vec!["-C", repo_path, "remote", "add", "origin", url], "添加 origin 失败")
    };
    let (_o, e, code) = sys::run("git", args.as_slice())?;
    if code != 0 {
        return Err(AppError::Other(format!("{fail}：{}", e.trim())));
    }
    Ok(())
}

/// 扫描根目录下的仓库并逐个体检（不入库，供预览）。
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

/// 扫描并把带 remote 的仓库登记进程序数据库。
#[tauri::command]
pub fn scan_and_import_repos(
    app: AppHandle,
    state: State<AppState>,
    root: String,
    max_depth: Option<usize>,
) -> Result<ImportScanResult> {
    crate::commands::ensure_writes_allowed(&state)?;
    let depth = max_depth.unwrap_or(5);
    let r = with_vault(&state, |v| {
        let mut data = store::load_data(v)?;
        let found = repo::scan(&PathBuf::from(&root), depth);
        let mut imported = 0u32;
        let mut updated = 0u32;
        let mut skipped_no_remote = 0u32;
        for p in found {
            let info = repo::inspect(&p, &data.identities, &data.clone_history);
            if info.remote_url.is_none() {
                skipped_no_remote += 1;
                continue;
            }
            let (is_new, _) = upsert_from_info(&mut data, &info, "scan");
            if is_new {
                imported += 1;
            } else {
                updated += 1;
            }
        }
        let machine_id = current_machine_id();
        crate::model::claim_unowned_repos(&mut data, &machine_id);
        crate::model::keep_repos_for_machine(&mut data, &machine_id);
        store::save_data(v, &data)?;
        crate::util::audit(
            v.root(),
            &format!("扫描导入仓库 imported={imported} updated={updated} skipped={skipped_no_remote}"),
        );
        let snapshot = data.repos.clone();
        let repos = snapshot.iter().map(|r| to_view(&data, r)).collect();
        Ok(ImportScanResult {
            imported,
            updated,
            skipped_no_remote,
            repos,
        })
    })?;
    crate::sync::scheduler::kick_publish(app);
    Ok(r)
}

/// 列出已登记仓库，并回读磁盘上的 origin。
#[tauri::command(async)]
pub fn list_managed_repos(state: State<'_, AppState>) -> Result<Vec<ManagedRepoView>> {
    let (mut data, vault) = {
        let guard = recover_lock(&state.vault);
        let v = guard.as_ref().ok_or(AppError::Locked)?;
        if !v.is_unlocked() {
            return Err(AppError::Locked);
        }
        (store::load_data(v)?, v.clone())
    };
    let machine_id = current_machine_id();
    let mut dirty = crate::model::claim_unowned_repos(&mut data, &machine_id);
    if crate::model::keep_repos_for_machine(&mut data, &machine_id) {
        dirty = true;
    }
    let identities = data.identities.clone();
    let history = data.clone_history.clone();
    let mut views = Vec::with_capacity(data.repos.len());
    for rec in data.repos.iter_mut() {
        let exists = PathBuf::from(&rec.path).is_dir();
        let mut current_alias = None;
        let mut needs_alias_fix = false;
        let mut remote_url = rec.remote_url.clone();
        // 目录不存在时绝不调用 git，避免无效路径把 UI 卡住。
        if exists {
            let live = repo::inspect(std::path::Path::new(&rec.path), &identities, &history);
            if live.remote_url.is_some() && live.remote_url != rec.remote_url {
                rec.remote_url = live.remote_url.clone();
                dirty = true;
            }
            if live.remote_url.is_some() {
                remote_url = live.remote_url;
            }
            current_alias = live.current_alias;
            needs_alias_fix = live.needs_alias_fix;
        }
        views.push(ManagedRepoView {
            id: rec.id.clone(),
            path: rec.path.clone(),
            name: rec.name.clone(),
            remote_url,
            identity_id: rec.identity_id.clone(),
            identity_name: identities
                .iter()
                .find(|i| rec.identity_id.as_deref() == Some(i.id.as_str()))
                .map(|i| i.name.clone()),
            added_at: rec.added_at.clone(),
            source: rec.source.clone(),
            exists,
            current_alias,
            needs_alias_fix,
        });
    }
    if dirty {
        store::save_data(&vault, &data)?;
    }
    Ok(views)
}

/// 从管理列表移除（不删除磁盘目录）。
#[tauri::command]
pub async fn remove_managed_repo(app: AppHandle, state: State<'_, AppState>, repo_id: String) -> Result<()> {
    crate::commands::ensure_writes_allowed(&state)?;
    with_vault(&state, |v| {
        let mut data = store::load_data(v)?;
        let machine_id = current_machine_id();
        crate::model::claim_unowned_repos(&mut data, &machine_id);
        crate::model::keep_repos_for_machine(&mut data, &machine_id);
        if !data.repos.iter().any(|r| r.id == repo_id) {
            return Err(AppError::Invalid("仓库不在管理列表中".into()));
        }
        data.repos.retain(|r| r.id != repo_id);
        data.deleted_repos.insert(repo_id.clone(), iso_now());
        store::save_data(v, &data)?;
        crate::util::audit(v.root(), &format!("移除已登记仓库 {repo_id}"));
        Ok(())
    })?;
    crate::sync::scheduler::kick_publish(app);
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetRepoRemoteArgs {
    pub repo_id: String,
    pub remote_url: String,
    pub identity_id: Option<String>,
}

/// 更换已登记仓库的 origin URL；可选同时绑定身份并改写为别名地址。
#[tauri::command]
pub fn set_repo_remote(app: AppHandle, state: State<AppState>, args: SetRepoRemoteArgs) -> Result<ManagedRepoView> {
    crate::commands::ensure_writes_allowed(&state)?;
    let r = with_vault(&state, |v| {
        let mut data = store::load_data(v)?;
        let idx = data
            .repos
            .iter()
            .position(|r| r.id == args.repo_id)
            .ok_or_else(|| AppError::Invalid("仓库不在管理列表中".into()))?;
        let repo_path = data.repos[idx].path.clone();
        if !PathBuf::from(&repo_path).is_dir() {
            return Err(AppError::Invalid("仓库目录不存在，无法改 remote".into()));
        }

        let raw = args.remote_url.trim();
        if raw.is_empty() {
            return Err(AppError::Invalid("remote URL 不能为空".into()));
        }
        let parsed = parse_repo_url(raw)?;
        let used_url = if let Some(iid) = &args.identity_id {
            let identity = data
                .identities
                .iter()
                .find(|i| i.id == *iid)
                .cloned()
                .ok_or_else(|| AppError::Invalid("身份不存在".into()))?;
            let url = crate::git::url::rewrite_to_alias(&identity.host_alias, &parsed.repo_path);
            repo::switch_identity(
                &repo_path,
                &url,
                identity.git_user_name.as_deref(),
                identity.email.as_deref(),
            )?;
            data.clone_history
                .insert(parsed.owner.to_lowercase(), identity.id.clone());
            data.repos[idx].identity_id = Some(identity.id);
            url
        } else {
            set_origin_url(&repo_path, raw)?;
            let inf = infer(&parsed, &data.identities, &data.clone_history);
            if let Some(rec) = inf.recommended {
                data.repos[idx].identity_id = Some(rec.identity_id);
            }
            raw.to_string()
        };

        data.repos[idx].remote_url = Some(used_url);
        let rec = data.repos[idx].clone();
        let view = to_view(&data, &rec);
        store::save_data(v, &data)?;
        crate::util::audit(v.root(), &format!("更新仓库 remote {}", repo_path));
        Ok(view)
    })?;
    crate::sync::scheduler::kick_publish(app);
    Ok(r)
}

/// 用资源管理器打开仓库目录。
#[tauri::command]
pub fn open_repo_dir(path: String) -> Result<()> {
    let p = PathBuf::from(path.trim());
    if !p.is_dir() {
        return Err(AppError::Invalid("目录不存在，无法打开".into()));
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&p)
            .spawn()
            .map_err(|e| AppError::Io(format!("打开目录失败：{e}")))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&p)
            .spawn()
            .map_err(|e| AppError::Io(format!("打开目录失败：{e}")))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&p)
            .spawn()
            .map_err(|e| AppError::Io(format!("打开目录失败：{e}")))?;
    }
    Ok(())
}

/// 给身份追加一条归属标识（去重、小写化）。
#[tauri::command]
pub fn add_owner(app: AppHandle, state: State<AppState>, identity_id: String, owner: String) -> Result<()> {
    crate::commands::ensure_writes_allowed(&state)?;
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
            id.updated_at = {
                use time::format_description::well_known::Rfc3339;
                time::OffsetDateTime::now_utc()
                    .format(&Rfc3339)
                    .unwrap_or_default()
            };
        }
        store::save_data(v, &data)?;
        Ok(())
    })?;
    crate::sync::scheduler::kick_publish(app);
    Ok(())
}

/// 切换某仓库的身份：改 remote 为别名地址 + 设提交身份 + 学习归属。
#[tauri::command]
pub fn switch_repo_identity(app: AppHandle, state: State<AppState>, repo_path: String, identity_id: String) -> Result<String> {
    crate::commands::ensure_writes_allowed(&state)?;
    let r = with_vault(&state, |v| {
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
        let ident_id = identity.id.clone();
        repo::upsert_managed_repo(
            &mut data.repos,
            ManagedRepo {
                id: uuid::Uuid::new_v4().to_string(),
                path: repo_path.clone(),
                name: repo::display_repo_name(&repo_path),
                remote_url: Some(new_url.clone()),
                identity_id: Some(ident_id),
                added_at: iso_now(),
                source: "manual".into(),
                machine_id: current_machine_id(),
            },
        );
        store::save_data(v, &data)?;
        crate::util::audit(v.root(), &format!("切换仓库身份 {repo_path} → {}", identity.name));
        Ok(new_url)
    })?;
    crate::sync::scheduler::kick_publish(app);
    Ok(r)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneResult {
    pub dest: String,
    pub used_url: String,
    pub identity_name: String,
    pub mode: String,
}

/// 探测选定文件夹适合 clone 还是 init，不落盘、不执行 git。
#[tauri::command]
pub fn inspect_clone_target(dest_dir: String, repo_name: String) -> Result<repo::ClonePlan> {
    if dest_dir.trim().is_empty() {
        return Err(AppError::Invalid("请先选择目标文件夹".into()));
    }
    Ok(repo::plan_clone_or_init(&PathBuf::from(dest_dir.trim()), repo_name.trim()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneOrInitArgs {
    pub url: String,
    pub dest_dir: String,
    pub identity_id: String,
    /// clone | init | addRemote
    pub mode: String,
}

fn run_git(
    env: &AgentEnv,
    args: &[&str],
    proxy: Option<&crate::app_config::NetworkProxy>,
) -> Result<(String, String, i32)> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    crate::agent::apply_to_command(&mut cmd, env);
    let ssh = crate::agent::ssh_bin(env);
    if env.ssh.is_some() || ssh != "ssh" {
        let home_cfg = crate::sys::home_included_config();
        let ssh_cmd = if home_cfg.is_file() {
            let cfg = home_cfg.to_string_lossy().replace('\\', "/");
            format!("\"{ssh}\" -F \"{cfg}\" -o BatchMode=yes -o StrictHostKeyChecking=accept-new")
        } else {
            format!("\"{ssh}\" -o BatchMode=yes -o StrictHostKeyChecking=accept-new")
        };
        cmd.env("GIT_SSH_COMMAND", ssh_cmd);
    }
    if let Some(p) = proxy {
        crate::net::apply_git_command(&mut cmd, p)?;
    }
    let output = cmd
        .output()
        .map_err(|e| AppError::Io(format!("执行 git 失败：{e}")))?;
    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    ))
}

fn apply_local_identity(repo_path: &str, name: Option<&str>, email: Option<&str>) -> Result<()> {
    if let Some(n) = name {
        sys::run("git", &["-C", repo_path, "config", "user.name", n])?;
    }
    if let Some(e) = email {
        sys::run("git", &["-C", repo_path, "config", "user.email", e])?;
    }
    Ok(())
}

/// 按探测结果把仓库落到本地：空目录 clone，已有项目 init，已有裸仓库补 remote。
#[tauri::command]
pub fn clone_repo(app: AppHandle, state: State<AppState>, args: CloneOrInitArgs) -> Result<CloneResult> {
    crate::commands::ensure_writes_allowed(&state)?;
    let parsed = parse_repo_url(&args.url)?;
    let dest = PathBuf::from(args.dest_dir.trim());
    let plan = repo::plan_clone_or_init(&dest, &parsed.repo);
    if !plan.can_proceed {
        return Err(AppError::Invalid(plan.message));
    }
    let mode = args.mode.trim();
    if mode != plan.suggested_mode {
        return Err(AppError::Invalid(format!(
            "选定文件夹当前应执行 {}，与请求的 {} 不一致。请重新选择目录。",
            plan.suggested_mode, mode
        )));
    }

    let env = {
        let current = recover_lock(&state.agent_env).clone();
        if crate::agent::is_ready(&current) {
            current
        } else {
            match crate::agent::ensure() {
                Ok(started) => {
                    *recover_lock(&state.agent_env) = started.clone();
                    started
                }
                Err(_) => current,
            }
        }
    };

    let proxy = {
        let cfg = recover_lock(&state.config);
        crate::net::effective(&cfg)
    };

    let r = with_vault(&state, |v| {
        let mut data = store::load_data(v)?;
        let identity = data
            .identities
            .iter()
            .find(|i| i.id == args.identity_id)
            .cloned()
            .ok_or_else(|| AppError::Invalid("身份不存在".into()))?;
        let used_url = crate::git::url::rewrite_to_alias(&identity.host_alias, &parsed.repo_path);
        let target_str = plan.target_path.clone();

        if let Some(key_id) = &identity.key_id {
            let _ = crate::agent::load_key(v, &env, key_id);
        }

        match mode {
            "clone" => {
                if let Some(parent) = PathBuf::from(&target_str).parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let (_o, e, code) = run_git(&env, &["clone", &used_url, &target_str], proxy.as_ref())?;
                if code != 0 {
                    return Err(AppError::Other(format!("git clone 失败：{}", e.trim())));
                }
                apply_local_identity(
                    &target_str,
                    identity.git_user_name.as_deref(),
                    identity.email.as_deref(),
                )?;
            }
            "init" => {
                std::fs::create_dir_all(&target_str)?;
                let (_o, e, code) = sys::run("git", &["-C", &target_str, "init"])?;
                if code != 0 {
                    return Err(AppError::Other(format!("git init 失败：{}", e.trim())));
                }
                let (_o, e, code) = sys::run("git", &["-C", &target_str, "remote", "add", "origin", &used_url])?;
                if code != 0 {
                    return Err(AppError::Other(format!("绑定远程失败：{}", e.trim())));
                }
                apply_local_identity(
                    &target_str,
                    identity.git_user_name.as_deref(),
                    identity.email.as_deref(),
                )?;
            }
            "addRemote" => {
                let (_o, e, code) = sys::run("git", &["-C", &target_str, "remote", "add", "origin", &used_url])?;
                if code != 0 {
                    return Err(AppError::Other(format!("绑定远程失败：{}", e.trim())));
                }
                apply_local_identity(
                    &target_str,
                    identity.git_user_name.as_deref(),
                    identity.email.as_deref(),
                )?;
            }
            other => return Err(AppError::Invalid(format!("未知操作：{other}"))),
        }

        data.clone_history
            .insert(parsed.owner.to_lowercase(), identity.id.clone());
        repo::upsert_managed_repo(
            &mut data.repos,
            ManagedRepo {
                id: uuid::Uuid::new_v4().to_string(),
                path: target_str.clone(),
                name: parsed.repo.clone(),
                remote_url: Some(used_url.clone()),
                identity_id: Some(identity.id.clone()),
                added_at: iso_now(),
                source: mode.to_string(),
                machine_id: current_machine_id(),
            },
        );
        store::save_data(v, &data)?;
        crate::util::audit(
            v.root(),
            &format!("{mode} {} 使用身份 {}", parsed.repo_path, identity.name),
        );

        Ok(CloneResult {
            dest: target_str,
            used_url,
            identity_name: identity.name,
            mode: mode.to_string(),
        })
    })?;
    crate::sync::scheduler::kick_publish(app);
    Ok(r)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubPatStatus {
    pub configured: bool,
}

/// 查询是否已保存 GitHub PAT（不回传令牌本身）。
#[tauri::command(async)]
pub fn github_pat_status(state: State<'_, AppState>) -> Result<GithubPatStatus> {
    with_vault(&state, |v| {
        let secrets = store::load_secrets(v)?;
        Ok(GithubPatStatus {
            configured: secrets
                .github_pat
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false),
        })
    })
}

/// 保存 GitHub PAT（存入 vault，绝不落明文）。
#[tauri::command]
pub fn set_github_pat(app: AppHandle, state: State<AppState>, token: String) -> Result<()> {
    crate::commands::ensure_writes_allowed(&state)?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(AppError::Invalid("PAT 不能为空".into()));
    }
    with_vault(&state, |v| {
        let mut secrets = store::load_secrets(v)?;
        secrets.github_pat = Some(token);
        store::save_secrets(v, &secrets)?;
        crate::util::audit(v.root(), "保存 GitHub PAT");
        Ok(())
    })?;
    crate::sync::scheduler::kick_publish(app);
    Ok(())
}

/// 清除已保存的 GitHub PAT。
#[tauri::command]
pub fn clear_github_pat(app: AppHandle, state: State<AppState>) -> Result<()> {
    crate::commands::ensure_writes_allowed(&state)?;
    with_vault(&state, |v| {
        let mut secrets = store::load_secrets(v)?;
        secrets.github_pat = None;
        store::save_secrets(v, &secrets)?;
        crate::util::audit(v.root(), "清除 GitHub PAT");
        Ok(())
    })?;
    crate::sync::scheduler::kick_publish(app);
    Ok(())
}

/// 校验 PAT 并返回账号名。
#[tauri::command(async)]
pub fn test_github_pat(state: State<'_, AppState>) -> Result<String> {
    let proxy = crate::net::effective(&recover_lock(&state.config));
    with_vault(&state, |v| {
        let secrets = store::load_secrets(v)?;
        let token = secrets
            .github_pat
            .ok_or_else(|| AppError::Invalid("尚未配置 PAT".into()))?;
        github::whoami(&token, proxy.as_ref())
    })
}

/// 拉取 PAT 账号所属组织（供批量导入归属标识）。
#[tauri::command(async)]
pub fn list_github_orgs(state: State<'_, AppState>) -> Result<Vec<String>> {
    let proxy = crate::net::effective(&recover_lock(&state.config));
    with_vault(&state, |v| {
        let secrets = store::load_secrets(v)?;
        let token = secrets
            .github_pat
            .ok_or_else(|| AppError::Invalid("尚未配置 PAT".into()))?;
        github::list_orgs(&token, proxy.as_ref())
    })
}

/// 用 PAT 上传某把密钥的公钥到 GitHub。
#[tauri::command]
pub fn upload_public_key(state: State<AppState>, key_id: String, title: String) -> Result<()> {
    crate::commands::ensure_writes_allowed(&state)?;
    let proxy = crate::net::effective(&recover_lock(&state.config));
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
        github::upload_public_key(&token, &title, &record.public_openssh, proxy.as_ref())?;
        crate::util::audit(v.root(), &format!("PAT 上传公钥 {}", record.name));
        Ok(())
    })
}
