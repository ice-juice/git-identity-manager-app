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
use tauri::State;

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

    let record = KeyRecord {
        id: key_id,
        name: name.unwrap_or_else(|| default_key_name(comment)),
        algorithm: "ed25519".into(),
        fingerprint: g.fingerprint,
        public_openssh: g.public_openssh,
        bits: Some(256),
        has_passphrase: true,
        weak: false,
        source_path: None,
        imported_at: now_iso8601(),
    };
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

/// 生成新密钥（供密钥管理界面单独使用）。
#[tauri::command]
pub fn generate_key(state: State<AppState>, comment: String, name: Option<String>) -> Result<KeyRecord> {
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
pub fn preview_config(entry: ManagedEntry) -> Result<ConfigPreview> {
    let path = sys::ssh_dir().join("config");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
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
pub fn apply_config(entry: ManagedEntry) -> Result<ApplyResult> {
    let path = sys::ssh_dir().join("config");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let backup = util::backup_file(&path)?;
    let new = managed::upsert(&old, entry.clone());
    crate::vault::atomic_write(&path, new.as_bytes())?;

    let verified = verify_with_ssh_g(&entry);
    Ok(ApplyResult {
        backup_path: backup.map(|p| p.display().to_string()),
        verified,
    })
}

/// 用 `ssh -G <alias>` 校验解析出的 hostname 与预期一致。
fn verify_with_ssh_g(entry: &ManagedEntry) -> bool {
    match sys::run("ssh", &["-G", &entry.alias]) {
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
pub fn create_identity(state: State<AppState>, args: CreateIdentityArgs) -> Result<CreateIdentityResult> {
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
        let comment = args
            .key_comment
            .clone()
            .unwrap_or_else(|| format!("{}@gam", args.name));
        let rec = generate_and_store_key(v, &comment, Some(format!("id_ed25519_{}", args.name)))?;
        data.keys.push(rec.clone());
        rec
    };

    // 投放文件到 ~/.ssh。
    let plat = platform::current();
    let dir = sys::ssh_dir();
    std::fs::create_dir_all(&dir)?;
    let priv_path = dir.join(format!("id_ed25519_{}", args.name));
    let pub_path = dir.join(format!("id_ed25519_{}.pub", args.name));

    // 公钥总是落盘。
    std::fs::write(&pub_path, format!("{}\n", record.public_openssh.trim()))?;

    let identity_file = if args.strict_mode {
        // 严格模式：磁盘只留 .pub，IdentityFile 指向 .pub。
        pub_path.clone()
    } else {
        // 默认模式：投放带口令的私钥密文，收紧 ACL。
        let priv_bytes = store::load_key(v, &record.id)?;
        std::fs::write(&priv_path, &priv_bytes)?;
        plat.secure_key_file(&priv_path)?;
        priv_path.clone()
    };

    // 写 config（备份 + 原子 + 校验）。
    let entry = ManagedEntry {
        alias: args.host_alias.clone(),
        host_name: args.real_host.clone(),
        user: args.user.clone().unwrap_or_else(|| "git".into()),
        identity_file: identity_file.display().to_string(),
        identities_only: true,
    };
    let cfg_path = dir.join("config");
    let old_cfg = std::fs::read_to_string(&cfg_path).unwrap_or_default();
    let backup = util::backup_file(&cfg_path)?;
    let new_cfg = managed::upsert(&old_cfg, entry.clone());
    crate::vault::atomic_write(&cfg_path, new_cfg.as_bytes())?;
    let verified = verify_with_ssh_g(&entry);

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
    };
    data.identities.push(identity.clone());
    store::save_data(v, &data)?;
    util::audit(
        v.root(),
        &format!("新建身份 {} (alias={})", identity.name, identity.host_alias),
    );

    Ok(CreateIdentityResult {
        identity,
        public_openssh: record.public_openssh,
        config_backup: backup.map(|p| p.display().to_string()),
        config_verified: verified,
        identity_file: identity_file.display().to_string(),
    })
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
