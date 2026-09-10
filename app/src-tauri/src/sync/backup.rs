//! 本地加密备份导出与导入（M6）
//!
//! 离线通道：把整个工作空间（身份、密钥元数据、仓库、私钥密文、PAT/口令）
//! 打包成单个端到端加密的 `.gambackup` 文件，使用用户指定的备份密码加密保护。
//! 适合换机离线迁移、冷备份以及故障恢复。

use crate::error::{AppError, Result};
use crate::model::{AccountData, KeyRecord, Secrets, TotpData, VaultData};
use crate::platform::PlatformOps;
use crate::store;
use crate::vault::crypto::{self, SALT_LEN, XNONCE_LEN};
use crate::vault::header::KdfParams;
use crate::vault::kdf;
use crate::vault::Vault;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const BACKUP_MAGIC: &[u8] = b"GAMBACKUP\x01";

/// 导出的机密数据结构体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPayload {
    pub version: u32,
    pub workspace_id: String,
    pub created_at: String,
    pub data: VaultData,
    pub secrets: Secrets,
    /// keyId -> base64(privateKeyBytes)
    pub private_keys: HashMap<String, String>,
    #[serde(default)]
    pub totp_data: TotpData,
    #[serde(default)]
    pub account_data: AccountData,
    /// iconHash -> base64(webp)
    #[serde(default)]
    pub icons: HashMap<String, String>,
}

/// 前端展示的备份摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    pub workspace_id: String,
    pub created_at: String,
    pub identity_count: usize,
    pub key_count: usize,
    pub repo_count: usize,
    pub has_github_pat: bool,
}

fn iso_now() -> String {
    let now = time::OffsetDateTime::now_utc();
    let format = time::format_description::well_known::Rfc3339;
    now.format(&format).unwrap_or_else(|_| "unknown".into())
}

/// 导出加密备份包
pub fn export_backup(vault: &Vault, dest_path: &Path, password: &str) -> Result<BackupSummary> {
    if !vault.is_unlocked() {
        return Err(AppError::Locked);
    }
    if password.trim().is_empty() {
        return Err(AppError::Invalid("备份加密密码不能为空".into()));
    }

    let mut data = store::load_data(vault)?;
    crate::sys::portableize_vault_data(&mut data);
    let secrets = store::load_secrets(vault)?;

    // 收集所有私钥明文（内存中临时持有，base64 编码打入 payload）
    let mut private_keys = HashMap::new();
    for key in &data.keys {
        if store::key_exists(vault, &key.id) {
            match store::load_key(vault, &key.id) {
                Ok(raw) => {
                    private_keys.insert(key.id.clone(), B64.encode(raw));
                }
                Err(e) => {
                    log::warn!("导出时读取密钥 {} 失败，跳过: {:?}", key.id, e);
                }
            }
        }
    }

    let summary = BackupSummary {
        workspace_id: vault.workspace_id().to_string(),
        created_at: iso_now(),
        identity_count: data.identities.len(),
        key_count: data.keys.len(),
        repo_count: data.repos.len(),
        has_github_pat: secrets.github_pat.is_some(),
    };

    let totp_data = store::load_totp(vault).unwrap_or_default();
    let account_data = store::load_accounts(vault).unwrap_or_default();
    let mut icons = HashMap::new();
    for hash in store::list_icon_hashes(vault) {
        if let Ok(raw) = store::load_icon(vault, &hash) {
            icons.insert(hash, B64.encode(raw));
        }
    }

    let payload = BackupPayload {
        version: 1,
        workspace_id: vault.workspace_id().to_string(),
        created_at: summary.created_at.clone(),
        data,
        secrets,
        private_keys,
        totp_data,
        account_data,
        icons,
    };

    let payload_bytes = serde_json::to_vec(&payload)?;

    // KDF 派生
    let salt = crypto::new_salt();
    let nonce = crypto::new_nonce();
    // 使用标称安全的快速 Argon2 参数：32MiB, 3 次迭代
    let kdf_params = KdfParams::new(32 * 1024, 3, 1);
    let kek = kdf::derive_kek(password.as_bytes(), &salt, &kdf_params)?;

    let ciphertext = crypto::aead_encrypt(&kek, &nonce, &payload_bytes)?;

    // 打包格式：
    // [MAGIC 10B] [MEM_KIB 4B] [ITERS 4B] [PAR 4B] [SALT 16B] [NONCE 24B] [CIPHERTEXT...]
    let mut out = Vec::with_capacity(
        BACKUP_MAGIC.len() + 12 + SALT_LEN + XNONCE_LEN + ciphertext.len(),
    );
    out.extend_from_slice(BACKUP_MAGIC);
    out.extend_from_slice(&kdf_params.mem_kib.to_le_bytes());
    out.extend_from_slice(&kdf_params.iters.to_le_bytes());
    out.extend_from_slice(&kdf_params.parallelism.to_le_bytes());
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);

    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::vault::atomic_write(dest_path, &out)?;

    Ok(summary)
}

/// 解析并解密备份文件，返回摘要
pub fn inspect_backup(src_path: &Path, password: &str) -> Result<BackupPayload> {
    let raw = std::fs::read(src_path)?;
    let min_len = BACKUP_MAGIC.len() + 12 + SALT_LEN + XNONCE_LEN + 16;
    if raw.len() < min_len || !raw.starts_with(BACKUP_MAGIC) {
        return Err(AppError::Invalid("不是有效的本地加密备份文件".into()));
    }

    let mut cursor = BACKUP_MAGIC.len();

    let mem_kib = u32::from_le_bytes(raw[cursor..cursor + 4].try_into().unwrap());
    cursor += 4;
    let iters = u32::from_le_bytes(raw[cursor..cursor + 4].try_into().unwrap());
    cursor += 4;
    let parallelism = u32::from_le_bytes(raw[cursor..cursor + 4].try_into().unwrap());
    cursor += 4;

    let kdf_params = KdfParams::new(mem_kib, iters, parallelism);

    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&raw[cursor..cursor + SALT_LEN]);
    cursor += SALT_LEN;

    let mut nonce = [0u8; XNONCE_LEN];
    nonce.copy_from_slice(&raw[cursor..cursor + XNONCE_LEN]);
    cursor += XNONCE_LEN;

    let ciphertext = &raw[cursor..];

    let kek = kdf::derive_kek(password.as_bytes(), &salt, &kdf_params)?;
    let plaintext = crypto::aead_decrypt(&kek, &nonce, ciphertext)
        .map_err(|_| AppError::Invalid("备份密码错误或文件已损坏".into()))?;

    let payload: BackupPayload = serde_json::from_slice(&plaintext)
        .map_err(|e| AppError::Invalid(format!("备份数据反序列化失败: {e}")))?;

    Ok(payload)
}

/// 导入备份到当前解锁的 Vault
pub fn import_backup(
    vault: &Vault,
    src_path: &Path,
    password: &str,
    merge: bool,
) -> Result<BackupSummary> {
    if !vault.is_unlocked() {
        return Err(AppError::Locked);
    }

    let payload = inspect_backup(src_path, password)?;

    let mut current_data = if merge {
        store::load_data(vault)?
    } else {
        VaultData::default()
    };
    let mut current_secrets = if merge {
        store::load_secrets(vault)?
    } else {
        Secrets::default()
    };

    // 1. 恢复私钥与密码
    for (key_id, b64_bytes) in &payload.private_keys {
        if let Ok(key_bytes) = B64.decode(b64_bytes) {
            store::save_key(vault, key_id, &key_bytes)?;
        }
    }
    for (key_id, pass) in &payload.secrets.key_passphrases {
        current_secrets.key_passphrases.insert(key_id.clone(), pass.clone());
    }
    if payload.secrets.github_pat.is_some() && (!merge || current_secrets.github_pat.is_none()) {
        current_secrets.github_pat = payload.secrets.github_pat.clone();
    }
    let current_totp = if merge {
        store::load_totp(vault).unwrap_or_default()
    } else {
        TotpData::default()
    };
    let current_acc = if merge {
        store::load_accounts(vault).unwrap_or_default()
    } else {
        AccountData::default()
    };
    current_secrets = crate::model::merge_secrets_with_meta(
        current_secrets,
        &payload.secrets,
        Some(&current_totp),
        Some(&payload.totp_data),
        Some(&current_acc),
        Some(&payload.account_data),
    );
    store::save_totp(vault, &crate::model::merge_totp_data(current_totp, payload.totp_data.clone()))?;
    store::save_accounts(
        vault,
        &crate::model::merge_account_data(current_acc, payload.account_data.clone()),
    )?;

    for (hash, b64) in &payload.icons {
        if let Ok(bytes) = B64.decode(b64) {
            let _ = store::save_icon(vault, hash, &bytes);
        }
    }

    // 2. 恢复 keys
    for key in payload.data.keys {
        if let Some(existing) = current_data.keys.iter_mut().find(|k| k.id == key.id) {
            *existing = key;
        } else {
            current_data.keys.push(key);
        }
    }

    // 3. 恢复 identities
    for ident in payload.data.identities {
        if let Some(existing) = current_data.identities.iter_mut().find(|i| i.id == ident.id) {
            *existing = ident;
        } else {
            current_data.identities.push(ident);
        }
    }

    // 4. 恢复 repos
    for repo in payload.data.repos {
        if let Some(existing) = current_data.repos.iter_mut().find(|r| r.id == repo.id) {
            *existing = repo;
        } else {
            current_data.repos.push(repo);
        }
    }

    // 5. 恢复 clone_history
    for (k, v) in payload.data.clone_history {
        current_data.clone_history.insert(k, v);
    }

    crate::sys::portableize_vault_data(&mut current_data);
    let machine_id = crate::app_config::AppConfig::current_machine_id();
    for repo in &mut current_data.repos {
        repo.machine_id = machine_id.clone();
    }
    store::save_data(vault, &current_data)?;
    store::save_secrets(vault, &current_secrets)?;

    // 自动部署私钥到工作空间 ssh-keys/ 目录下，保证 OpenSSH 能立刻识别使用
    for key in &current_data.keys {
        if store::key_exists(vault, &key.id) {
            if let Ok(priv_bytes) = store::load_key(vault, &key.id) {
                let _ = deploy_key_to_workspace(vault, key, &priv_bytes);
            }
        }
    }

    Ok(BackupSummary {
        workspace_id: payload.workspace_id,
        created_at: payload.created_at,
        identity_count: current_data.identities.len(),
        key_count: current_data.keys.len(),
        repo_count: current_data.repos.len(),
        has_github_pat: current_secrets.github_pat.is_some(),
    })
}

/// 把恢复或拉取的密钥写入工作空间 `ssh-keys/` 并收紧 ACL
pub fn deploy_key_to_workspace(vault: &Vault, key: &KeyRecord, priv_bytes: &[u8]) -> Result<()> {
    let dir = crate::sys::workspace_ssh_keys_dir(vault.root());
    std::fs::create_dir_all(&dir)?;
    let priv_path = dir.join(&key.name);
    let pub_path = dir.join(format!("{}.pub", key.name));
    let _ = std::fs::write(&pub_path, format!("{}\n", key.public_openssh.trim()));
    std::fs::write(&priv_path, priv_bytes)?;
    let _ = crate::platform::current().secure_key_file(&priv_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::KeyRecord;
    use std::path::PathBuf;

    fn test_vault() -> (Vault, PathBuf) {
        let dir = std::env::temp_dir().join(format!("gam-test-backup-{}", uuid::Uuid::new_v4()));
        let kdf = KdfParams::new(crate::vault::kdf::MEM_FLOOR_KIB, 2, 1);
        let (v, _) = Vault::init(&dir, "testpass123", kdf).unwrap();
        (v, dir)
    }

    #[test]
    fn test_export_and_import_roundtrip() {
        let (vault, dir1) = test_vault();
        let mut data = store::load_data(&vault).unwrap();
        data.keys.push(KeyRecord {
            id: "key-1".into(),
            name: "id_ed25519".into(),
            algorithm: "ed25519".into(),
            fingerprint: "SHA256:test".into(),
            public_openssh: "ssh-ed25519 AAA... test".into(),
            bits: Some(256),
            has_passphrase: false,
            weak: false,
            source_path: None,
            deployed_path: None,
            imported_at: "2026-09-09".into(),
        });
        store::save_data(&vault, &data).unwrap();
        store::save_key(&vault, "key-1", b"fake-private-key").unwrap();

        let backup_dir = std::env::temp_dir().join(format!("gam-test-dest-{}", uuid::Uuid::new_v4()));
        let backup_file = backup_dir.join("backup.gambackup");

        // 导出
        let summary = export_backup(&vault, &backup_file, "backup-password-123").unwrap();
        assert_eq!(summary.key_count, 1);
        assert!(backup_file.exists());

        // 查看
        let inspected = inspect_backup(&backup_file, "backup-password-123").unwrap();
        assert_eq!(inspected.data.keys.len(), 1);
        assert_eq!(inspected.private_keys.get("key-1").unwrap(), &B64.encode(b"fake-private-key"));

        // 错误密码拒绝
        assert!(inspect_backup(&backup_file, "wrong-password").is_err());

        // 导入到全新 vault
        let (vault2, dir2) = test_vault();
        let import_summary = import_backup(&vault2, &backup_file, "backup-password-123", false).unwrap();
        assert_eq!(import_summary.key_count, 1);

        let data2 = store::load_data(&vault2).unwrap();
        assert_eq!(data2.keys.len(), 1);
        assert_eq!(data2.keys[0].name, "id_ed25519");
        assert_eq!(store::load_key(&vault2, "key-1").unwrap(), b"fake-private-key");

        let _ = std::fs::remove_dir_all(&dir1);
        let _ = std::fs::remove_dir_all(&dir2);
        let _ = std::fs::remove_dir_all(&backup_dir);
    }
}
