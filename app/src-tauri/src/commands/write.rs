//! M3 命令层：密钥生成、config 预览/落盘、新建身份、机密二次验证查看。

use crate::commands::AppState;
use crate::error::{AppError, Result};
use crate::model::{Identity, KeyRecord};
use crate::platform::{self, PlatformOps};
use crate::ssh::keygen;
use crate::ssh::managed::{self, ManagedEntry};
use crate::store;
use crate::sys;
use crate::util;
use crate::vault::Vault;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

/// 密钥注释：优先邮箱，否则备注名，再否则提交姓名。
pub fn suggest_key_comment(email: Option<&str>, name: &str, git_user_name: Option<&str>) -> String {
    if let Some(e) = email.map(str::trim).filter(|s| !s.is_empty()) {
        return e.to_string();
    }
    let n = name.trim();
    if !n.is_empty() {
        return format!("{n}@gam");
    }
    if let Some(g) = git_user_name.map(str::trim).filter(|s| !s.is_empty()) {
        let compact = g.split_whitespace().collect::<Vec<_>>().join(".");
        return format!("{compact}@gam");
    }
    String::new()
}

fn persist_workspace_ssh_config(v: &Vault, text: &str) -> Result<()> {
    sys::persist_ssh_config(Some(v.root()), text)
}

fn load_workspace_ssh_config(v: &Vault) -> String {
    sys::read_workspace_ssh_config(v.root()).1
}

/// 按库内身份补齐工作空间 SSH Host。正本若被写成 Include stub 会先清空再重建。
pub fn reconcile_ssh_hosts(v: &Vault) -> Result<u32> {
    let data = store::load_data(v)?;
    let dest = sys::workspace_ssh_config(v.root());
    let mut text = std::fs::read_to_string(&dest).unwrap_or_default();
    if sys::text_is_include_only(&text) {
        text.clear();
    }
    let existing = managed::list(&text);
    let mut added = 0u32;
    for id in &data.identities {
        if id.host_alias.trim().is_empty() {
            continue;
        }
        if existing
            .iter()
            .any(|e| e.alias.eq_ignore_ascii_case(&id.host_alias))
        {
            continue;
        }
        let Some(kid) = &id.key_id else {
            continue;
        };
        let Some(record) = data.keys.iter().find(|k| &k.id == kid) else {
            continue;
        };
        let identity_file = match record
            .deployed_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(p) => p.to_string(),
            None => {
                let stem = sys::key_file_stem(&id.name);
                deploy_openssh_files(v, record, &stem, id.strict_mode)?
            }
        };
        text = managed::upsert(
            &text,
            ManagedEntry {
                alias: id.host_alias.clone(),
                host_name: id.real_host.clone(),
                user: id.user.clone(),
                identity_file,
                identities_only: true,
            },
        );
        added += 1;
    }
    if added > 0 {
        persist_workspace_ssh_config(v, &text)?;
        util::audit(v.root(), &format!("按身份补齐 SSH Host {added} 条"));
    }
    Ok(added)
}

fn ready_agent(state: &State<AppState>) -> Result<crate::agent::AgentEnv> {
    {
        let env = state.agent_env.lock().unwrap().clone();
        if crate::agent::is_ready(&env) {
            return Ok(env);
        }
    }
    let env = crate::agent::ensure()?;
    *state.agent_env.lock().unwrap() = env.clone();
    Ok(env)
}

fn now_iso8601() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

/// 生成密钥并存入 vault（私钥密文 + 口令 + 元数据），返回元数据记录。
fn generate_and_store_key(v: &Vault, comment: &str, name: Option<String>) -> Result<KeyRecord> {
    let g = keygen::generate_ed25519(comment)?;
    let key_id = uuid::Uuid::new_v4().to_string();
    store::save_key(v, &key_id, g.private_openssh_encrypted.as_bytes())?;

    let mut secrets = store::load_secrets(v)?;
    secrets
        .key_passphrases
        .insert(key_id.clone(), g.passphrase.clone());
    store::save_secrets(v, &secrets)?;

    let stem_name = name.clone().unwrap_or_else(|| default_key_name(comment));
    let mut record = KeyRecord {
        id: key_id.clone(),
        name: stem_name.clone(),
        algorithm: "ed25519".into(),
        fingerprint: g.fingerprint,
        public_openssh: g.public_openssh,
        bits: Some(256),
        has_passphrase: true,
        weak: false,
        source_path: None,
        deployed_path: None,
        imported_at: now_iso8601(),
    };
    let deployed = deploy_openssh_files(v, &record, &sys::key_file_stem(&stem_name), false)?;
    record.deployed_path = Some(deployed);
    Ok(record)
}

fn default_key_name(comment: &str) -> String {
    let base = comment.split('@').next().unwrap_or(comment).trim();
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    if cleaned.is_empty() {
        "id_ed25519".into()
    } else {
        format!("id_ed25519_{cleaned}")
    }
}

/// 把 OpenSSH 公钥（以及非严格模式下的带口令私钥）写到工作空间 `ssh-keys/`。
/// 返回 IdentityFile 应指向的真实路径。
fn deploy_openssh_files(v: &Vault, record: &KeyRecord, stem: &str, strict: bool) -> Result<String> {
    let dir = sys::workspace_ssh_keys_dir(v.root());
    std::fs::create_dir_all(&dir)?;
    let priv_path = dir.join(stem);
    let pub_path = dir.join(format!("{stem}.pub"));
    std::fs::write(&pub_path, format!("{}\n", record.public_openssh.trim()))?;

    let target = if strict {
        if priv_path.exists() {
            let _ = std::fs::remove_file(&priv_path);
        }
        pub_path
    } else {
        let priv_bytes = store::load_key(v, &record.id)?;
        std::fs::write(&priv_path, &priv_bytes)?;
        platform::current().secure_key_file(&priv_path)?;
        priv_path
    };
    Ok(sys::identity_file_for_ssh(&target))
}

/// 生成新密钥（供密钥管理界面单独使用）。
#[tauri::command]
pub fn generate_key(app: AppHandle, state: State<AppState>, comment: String, name: Option<String>) -> Result<KeyRecord> {
    crate::commands::ensure_writes_allowed(&state)?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let record = generate_and_store_key(v, &comment, name)?;
    let mut data = store::load_data(v)?;
    data.keys.push(record.clone());
    store::save_data(v, &data)?;
    util::audit(v.root(), &format!("生成密钥 {}", record.name));
    drop(vault);
    crate::sync::scheduler::kick_publish(app);
    Ok(record)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigPreview {
    pub diff: String,
    pub new_text: String,
}

/// 预览把某托管条目写入 config 的 diff（不落盘）。
#[tauri::command]
pub fn preview_config(state: State<AppState>, entry: ManagedEntry) -> Result<ConfigPreview> {
    let ws = state
        .vault
        .lock()
        .unwrap()
        .as_ref()
        .map(|v| v.root().to_path_buf());
    let old = sys::read_canonical_ssh_config(ws.as_deref()).1;
    let new = managed::upsert(&old, entry);
    Ok(ConfigPreview {
        diff: util::unified_diff(&old, &new),
        new_text: new,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub backup_path: Option<String>,
    /// `ssh -G` 反向校验是否与预期一致。
    pub verified: bool,
}

/// 备份并写入 config（原子替换 + ssh -G 反向校验）。
#[tauri::command]
pub fn apply_config(app: AppHandle, state: State<AppState>, entry: ManagedEntry) -> Result<ApplyResult> {
    crate::commands::ensure_writes_allowed(&state)?;
    let ws = state
        .vault
        .lock()
        .unwrap()
        .as_ref()
        .map(|v| v.root().to_path_buf());
    let (path, old) = sys::read_canonical_ssh_config(ws.as_deref());
    let backup = util::backup_file(&path)?;
    let new = managed::upsert(&old, entry.clone());
    let ws = state
        .vault
        .lock()
        .unwrap()
        .as_ref()
        .map(|v| v.root().to_path_buf());
    sys::persist_ssh_config(ws.as_deref(), &new)?;

    let verified = verify_with_ssh_g(&entry, ws.as_deref());
    crate::sync::scheduler::kick_publish(app);
    Ok(ApplyResult {
        backup_path: backup.map(|p| p.display().to_string()),
        verified,
    })
}

/// 用 `ssh -F <工作空间正本> -G <alias>` 校验解析出的 hostname 与预期一致。
fn verify_with_ssh_g(entry: &ManagedEntry, workspace: Option<&std::path::Path>) -> bool {
    let cfg = workspace.map(sys::workspace_ssh_config);
    let cfg_s = cfg.as_ref().map(|p| p.to_string_lossy().replace('\\', "/"));
    let alias = entry.alias.as_str();
    let args: Vec<&str> = match cfg_s.as_deref() {
        Some(c) => vec!["-F", c, "-G", alias],
        None => vec!["-G", alias],
    };
    match sys::run("ssh", &args) {
        Ok((out, _e, _c)) => {
            let want = format!("hostname {}", entry.host_name.to_lowercase());
            out.lines()
                .any(|l| l.trim().to_lowercase() == want)
        }
        Err(_) => false,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIdentityArgs {
    pub name: String,
    pub platform: String,
    pub host_alias: String,
    pub real_host: String,
    pub user: Option<String>,
    pub email: Option<String>,
    pub git_user_name: Option<String>,
    pub strict_mode: bool,
    /// 提供则复用现有密钥；否则生成新密钥。
    pub key_id: Option<String>,
    pub key_comment: Option<String>,
    pub owners: Option<Vec<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIdentityResult {
    pub identity: Identity,
    pub public_openssh: String,
    pub config_backup: Option<String>,
    pub config_verified: bool,
    pub identity_file: String,
}

/// 新建身份：生成/复用密钥 → 投放公钥（严格模式）或私钥（默认）→ 收紧 ACL →
/// 备份并写 config → 登记身份。
#[tauri::command]
pub fn create_identity(app: AppHandle, state: State<AppState>, args: CreateIdentityArgs) -> Result<CreateIdentityResult> {
    crate::commands::ensure_writes_allowed(&state)?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }

    let mut data = store::load_data(v)?;

    // 解析密钥。
    let record = if let Some(id) = &args.key_id {
        data.keys
            .iter()
            .find(|k| &k.id == id)
            .cloned()
            .ok_or_else(|| AppError::Invalid("指定的密钥不存在".into()))?
    } else {
        let comment = {
            let raw = args.key_comment.as_deref().unwrap_or("").trim();
            if !raw.is_empty() {
                raw.to_string()
            } else {
                suggest_key_comment(args.email.as_deref(), &args.name, args.git_user_name.as_deref())
            }
        };
        let rec = generate_and_store_key(v, &comment, Some(format!("id_ed25519_{}", args.name)))?;
        data.keys.push(rec.clone());
        rec
    };

    // 投放 OpenSSH 文件到工作空间 ssh-keys/（不再写到 ~/.ssh）。
    let stem = sys::key_file_stem(&args.name);
    let identity_file = deploy_openssh_files(v, &record, &stem, args.strict_mode)?;
    if let Some(k) = data.keys.iter_mut().find(|k| k.id == record.id) {
        k.deployed_path = Some(identity_file.clone());
    }

    // 写 config（备份 + 原子 + 校验）。
    let entry = ManagedEntry {
        alias: args.host_alias.clone(),
        host_name: args.real_host.clone(),
        user: args.user.clone().unwrap_or_else(|| "git".into()),
        identity_file,
        identities_only: true,
    };
    let cfg_path = sys::workspace_ssh_config(v.root());
    let old_cfg = load_workspace_ssh_config(v);
    let backup = util::backup_file(&cfg_path)?;
    let new_cfg = managed::upsert(&old_cfg, entry.clone());
    persist_workspace_ssh_config(v, &new_cfg)?;
    let verified = verify_with_ssh_g(&entry, Some(v.root()));

    // 登记身份。
    let identity = Identity {
        id: uuid::Uuid::new_v4().to_string(),
        name: args.name.clone(),
        platform: args.platform.clone(),
        host_alias: args.host_alias.clone(),
        real_host: args.real_host.clone(),
        user: args.user.clone().unwrap_or_else(|| "git".into()),
        email: args.email.clone(),
        git_user_name: args.git_user_name.clone(),
        key_id: Some(record.id.clone()),
        owners: args.owners.clone().unwrap_or_default(),
        strict_mode: args.strict_mode,
        updated_at: now_iso8601(),
    };
    data.identities.push(identity.clone());
    store::save_data(v, &data)?;
    if args.strict_mode {
        if let Ok(env) = ready_agent(&state) {
            let _ = crate::agent::load_key(v, &env, &record.id);
        }
    }
    util::audit(
        v.root(),
        &format!("新建身份 {} (alias={})", identity.name, identity.host_alias),
    );
    drop(vault);
    crate::sync::scheduler::kick_publish(app);

    Ok(CreateIdentityResult {
        identity,
        public_openssh: record.public_openssh,
        config_backup: backup.map(|p| p.display().to_string()),
        config_verified: verified,
        identity_file: entry.identity_file.clone(),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageIdentityDraftArgs {
    pub name: String,
    pub host_alias: String,
    pub real_host: String,
    pub user: Option<String>,
    pub strict_mode: bool,
    pub key_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageIdentityDraftResult {
    pub identity_file: String,
    pub config_verified: bool,
    pub key_id: String,
}

/// 向导验证用：部署密钥文件、写入 Host、把私钥加载进 agent，不登记身份。
#[tauri::command]
pub fn stage_identity_draft(
    state: State<AppState>,
    args: StageIdentityDraftArgs,
) -> Result<StageIdentityDraftResult> {
    crate::commands::ensure_writes_allowed(&state)?;
    let alias = args.host_alias.trim();
    let host = args.real_host.trim();
    let key_id = args.key_id.trim();
    if alias.is_empty() || host.is_empty() || key_id.is_empty() {
        return Err(AppError::Invalid("别名、主机或密钥不完整，无法验证".into()));
    }

    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }

    let mut data = store::load_data(v)?;
    if data
        .identities
        .iter()
        .any(|i| i.host_alias.eq_ignore_ascii_case(alias))
    {
        return Err(AppError::Invalid(format!("Host 别名「{alias}」已被占用")));
    }
    let record = data
        .keys
        .iter()
        .find(|k| k.id == key_id)
        .cloned()
        .ok_or_else(|| AppError::Invalid("指定的密钥不存在".into()))?;

    let stem = sys::key_file_stem(&args.name);
    let identity_file = deploy_openssh_files(v, &record, &stem, args.strict_mode)?;
    if let Some(k) = data.keys.iter_mut().find(|k| k.id == record.id) {
        k.deployed_path = Some(identity_file.clone());
    }
    store::save_data(v, &data)?;

    let entry = ManagedEntry {
        alias: alias.to_string(),
        host_name: host.to_string(),
        user: args.user.clone().unwrap_or_else(|| "git".into()),
        identity_file: identity_file.clone(),
        identities_only: true,
    };
    let cfg_path = sys::workspace_ssh_config(v.root());
    let old_cfg = load_workspace_ssh_config(v);
    let _ = util::backup_file(&cfg_path)?;
    persist_workspace_ssh_config(v, &managed::upsert(&old_cfg, entry.clone()))?;
    let verified = verify_with_ssh_g(&entry, Some(v.root()));

    let env = ready_agent(&state)?;
    crate::agent::load_key(v, &env, &record.id)?;
    util::audit(v.root(), &format!("向导验证预加载密钥 {} (alias={alias})", record.name));

    Ok(StageIdentityDraftResult {
        identity_file,
        config_verified: verified,
        key_id: record.id,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbortIdentityDraftArgs {
    pub host_alias: String,
    pub key_id: String,
}

/// 向导取消：撤回尚未登记的 Host，并从 agent 卸下未被其它身份使用的密钥。
#[tauri::command]
pub fn abort_identity_draft(state: State<AppState>, args: AbortIdentityDraftArgs) -> Result<()> {
    let alias = args.host_alias.trim();
    let key_id = args.key_id.trim();
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    let data = store::load_data(v)?;
    let alias_in_use = !alias.is_empty()
        && data
            .identities
            .iter()
            .any(|i| i.host_alias.eq_ignore_ascii_case(alias));
    if !alias.is_empty() && !alias_in_use {
        let old_cfg = load_workspace_ssh_config(v);
        persist_workspace_ssh_config(v, &managed::remove(&old_cfg, alias))?;
    }
    let key_in_use = !key_id.is_empty()
        && data
            .identities
            .iter()
            .any(|i| i.key_id.as_deref() == Some(key_id));
    if !key_id.is_empty() && !key_in_use {
        if let Some(record) = data.keys.iter().find(|k| k.id == key_id) {
            if let Ok(env) = ready_agent(&state) {
                let _ = crate::agent::unload_public(&env, &record.public_openssh);
            }
        }
    }
    Ok(())
}

/// 二次验证后查看密钥口令（敏感，需重输访问密码；写审计）。
#[tauri::command]
pub fn reveal_key_passphrase(
    state: State<AppState>,
    password: String,
    key_id: String,
) -> Result<String> {
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    v.verify_password(&password)?; // 重认证
    let secrets = store::load_secrets(v)?;
    let pass = secrets
        .key_passphrases
        .get(&key_id)
        .cloned()
        .ok_or_else(|| AppError::Invalid("该密钥无已保存口令".into()))?;
    util::audit(v.root(), &format!("二次验证查看密钥口令 key_id={key_id}"));
    Ok(pass)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevealedKeyMaterial {
    pub public_openssh: String,
    pub private_openssh: String,
    pub passphrase: Option<String>,
}

/// 二次验证后查看公钥/私钥（敏感，需重输访问密码；写审计）。
#[tauri::command]
pub fn reveal_key_material(
    state: State<AppState>,
    password: String,
    key_id: String,
) -> Result<RevealedKeyMaterial> {
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }
    v.verify_password(&password)?;
    let data = store::load_data(v)?;
    let record = data
        .keys
        .iter()
        .find(|k| k.id == key_id)
        .ok_or_else(|| AppError::Invalid("密钥不存在".into()))?;
    let raw = store::load_key(v, &key_id)?;
    let private_openssh = String::from_utf8(raw)
        .map_err(|_| AppError::Invalid("私钥不是可显示的 OpenSSH 文本".into()))?;
    let secrets = store::load_secrets(v)?;
    let passphrase = secrets.key_passphrases.get(&key_id).cloned();
    util::audit(v.root(), &format!("二次验证查看密钥材料 key_id={key_id}"));
    Ok(RevealedKeyMaterial {
        public_openssh: record.public_openssh.clone(),
        private_openssh,
        passphrase,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIdentityArgs {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub host_alias: String,
    pub real_host: String,
    pub user: Option<String>,
    pub email: Option<String>,
    pub git_user_name: Option<String>,
    pub strict_mode: bool,
    pub owners: Option<Vec<String>>,
}

/// 修改已有身份信息，并同步更新工作空间 SSH config。
#[tauri::command]
pub fn update_identity(app: AppHandle, state: State<AppState>, args: UpdateIdentityArgs) -> Result<Identity> {
    crate::commands::ensure_writes_allowed(&state)?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }

    let mut data = store::load_data(v)?;
    let idx = data
        .identities
        .iter()
        .position(|i| i.id == args.id)
        .ok_or_else(|| AppError::Invalid("身份不存在".into()))?;

    let old_alias = data.identities[idx].host_alias.clone();
    let key_id = data.identities[idx].key_id.clone();

    // 检查重名与别名冲突（排除自身）
    if data
        .identities
        .iter()
        .any(|i| i.id != args.id && i.name.eq_ignore_ascii_case(&args.name))
    {
        return Err(AppError::Invalid(format!("备注名「{}」已存在", args.name)));
    }
    if data
        .identities
        .iter()
        .any(|i| i.id != args.id && i.host_alias.eq_ignore_ascii_case(&args.host_alias))
    {
        return Err(AppError::Invalid(format!("Host 别名「{}」已被占用", args.host_alias)));
    }

    // 重新部署密钥文件（如果严格模式改变或名字改变）
    let identity_file = if let Some(ref kid) = key_id {
        if let Some(record) = data.keys.iter().find(|k| &k.id == kid) {
            let stem = sys::key_file_stem(&args.name);
            deploy_openssh_files(v, record, &stem, args.strict_mode)?
        } else {
            "".to_string()
        }
    } else {
        "".to_string()
    };

    let config_path = sys::workspace_ssh_config(v.root());
    let old_config = load_workspace_ssh_config(v);
    let mut intermediate = if old_alias != args.host_alias {
        managed::remove(&old_config, &old_alias)
    } else {
        old_config
    };

    if !identity_file.is_empty() {
        let entry = ManagedEntry {
            alias: args.host_alias.clone(),
            host_name: args.real_host.clone(),
            user: args.user.clone().unwrap_or_else(|| "git".into()),
            identity_file,
            identities_only: true,
        };
        intermediate = managed::upsert(&intermediate, entry);
    }
    let _ = util::backup_file(&config_path);
    persist_workspace_ssh_config(v, &intermediate)?;

    let mut updated = data.identities[idx].clone();
    updated.name = args.name;
    updated.platform = args.platform;
    updated.host_alias = args.host_alias;
    updated.real_host = args.real_host;
    updated.user = args.user.unwrap_or_else(|| "git".into());
    updated.email = args.email;
    updated.git_user_name = args.git_user_name;
    updated.strict_mode = args.strict_mode;
    updated.updated_at = now_iso8601();
    if let Some(owners) = args.owners {
        updated.owners = owners;
    }

    data.identities[idx] = updated.clone();
    store::save_data(v, &data)?;
    util::audit(v.root(), &format!("更新身份 {} (alias={})", updated.name, updated.host_alias));
    drop(vault);
    crate::sync::scheduler::kick_publish(app);

    Ok(updated)
}

/// 删除已有身份，并从工作空间 SSH config 托管区块中移除。
#[tauri::command]
pub fn delete_identity(app: AppHandle, state: State<AppState>, identity_id: String) -> Result<()> {
    crate::commands::ensure_writes_allowed(&state)?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    if !v.is_unlocked() {
        return Err(AppError::Locked);
    }

    let mut data = store::load_data(v)?;
    let idx = data
        .identities
        .iter()
        .position(|i| i.id == identity_id)
        .ok_or_else(|| AppError::Invalid("身份不存在".into()))?;

    let removed = data.identities.remove(idx);
    data.deleted_identities
        .insert(removed.id.clone(), now_iso8601());

    let config_path = sys::workspace_ssh_config(v.root());
    let old_config = load_workspace_ssh_config(v);
    let new_config = managed::remove(&old_config, &removed.host_alias);
    let _ = util::backup_file(&config_path);
    persist_workspace_ssh_config(v, &new_config)?;

    store::save_data(v, &data)?;
    util::audit(v.root(), &format!("删除身份 {} (alias={})", removed.name, removed.host_alias));
    drop(vault);
    crate::sync::scheduler::kick_publish(app);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_comment_prefers_email_then_name_then_git_user() {
        assert_eq!(
            suggest_key_comment(Some("a@b.com"), "nova", Some("Nova Reyes")),
            "a@b.com"
        );
        assert_eq!(suggest_key_comment(None, "nova-labs", Some("Nova")), "nova-labs@gam");
        assert_eq!(suggest_key_comment(Some("  "), "", Some("Nova Reyes")), "Nova.Reyes@gam");
        assert_eq!(suggest_key_comment(None, "", None), "");
    }
}
