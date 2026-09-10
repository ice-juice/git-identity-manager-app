//! 端到端加密云端同步引擎（v1.1）
//!
//! 核心原则：
//! 1. 零知识端到端加密：云端永远只能看到经过 XChaCha20-Poly1305 加密的密文与 HMAC 随机化散列的对象名。
//! 2. 身份、密钥文件名、用户名、仓库地址等一律不出现在云端。
//! 3. 事务性原子提交："先传加密对象，后写 manifest" 保证网络异常时不损坏已有快照。
//! 4. SSH 分两层：本机 `ssh/config` 展开成当前工作空间绝对路径给 OpenSSH；
//!    云端 / 快照 / 比对只存 `%GAM_WORKSPACE%/ssh-keys/...`。换机重写不得原样上传。

use crate::error::{AppError, Result};
use crate::model::{Secrets, VaultData};
use crate::store;
use crate::sync::backup::deploy_key_to_workspace;
use crate::sync::s3::S3Client;
use crate::vault::crypto::{self, KEY_LEN, LABEL_SYNC_OBJECT, XNONCE_LEN};
use crate::vault::envelope;
use crate::vault::header::VaultHeader;
use crate::vault::recovery;
use crate::vault::Vault;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

/// 始终保留的最近快照份数。
pub const MAX_RECENT_SNAPSHOTS: usize = 10;
/// 额外保留「每天第一份」的天数（含今天）。
pub const DAILY_SNAPSHOT_DAYS: i64 = 14;
const HISTORY_INDEX_KEY: &str = "history/index.enc";

/// 云端 manifest 文件名
const MANIFEST_FILE_KEY: &str = "manifest.enc";
/// 工作空间头部（仅信封，无明文机密）。固定路径，换机时尚无 MK，不能走 HMAC 对象名。
pub const VAULT_HEADER_KEY: &str = "vault-header.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CloudVaultHeader {
    pub version: u32,
    pub header: VaultHeader,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudRestorePreview {
    pub workspace_id: String,
    pub updated_at: Option<String>,
    pub identity_count: usize,
    pub key_count: usize,
    pub repo_count: usize,
    pub has_manifest: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncManifest {
    pub version: u32,
    pub workspace_id: String,
    pub updated_at: String,
    pub client_name: String,
    /// 逻辑路径（如 "data/identities.json", "keys/k1.key"） -> 对象条目
    pub objects: HashMap<String, SyncObjectEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncObjectEntry {
    /// 云端散列对象名（HMAC-SHA256 结果）
    pub object_name: String,
    /// 明文 SHA256，用于快速对比一致性
    pub sha256: String,
    pub size: usize,
    pub updated_at: String,
}

/// 云同步状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncStatus {
    pub remote_exists: bool,
    pub remote_updated_at: Option<String>,
    pub remote_workspace_id: Option<String>,
    pub local_identity_count: usize,
    pub local_key_count: usize,
    pub local_repo_count: usize,
    pub status: String, // "synced" | "local_ahead" | "remote_ahead" | "not_synced" | "different_workspace"
    /// 云端是否已有换机恢复所需的工作空间头部。
    #[serde(default)]
    pub header_ready: bool,
}

/// 同步执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub synced_at: String,
    pub objects_transferred: usize,
    pub identity_count: usize,
    pub key_count: usize,
    pub repo_count: usize,
    pub message: String,
}

fn iso_now() -> String {
    let now = time::OffsetDateTime::now_utc();
    let format = time::format_description::well_known::Rfc3339;
    now.format(&format).unwrap_or_else(|_| "unknown".into())
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let mut s = String::with_capacity(64);
    for b in hasher.finalize() {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// 对称加密：nonce || ciphertext
fn encrypt_payload(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>> {
    let nonce = crypto::new_nonce();
    let ct = crypto::aead_encrypt(key, &nonce, plaintext)?;
    let mut out = Vec::with_capacity(XNONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// 对称解密
fn decrypt_payload(key: &[u8; KEY_LEN], data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < XNONCE_LEN {
        return Err(AppError::Crypto);
    }
    let (n, ct) = data.split_at(XNONCE_LEN);
    let mut nonce = [0u8; XNONCE_LEN];
    nonce.copy_from_slice(n);
    crypto::aead_decrypt(key, &nonce, ct)
}

/// 读取并解密云端 manifest
pub fn fetch_remote_manifest(
    vault: &Vault,
    s3: &S3Client,
) -> Result<Option<SyncManifest>> {
    let key = vault.subkey(LABEL_SYNC_OBJECT)?;
    fetch_remote_manifest_with_key(&key, s3)
}

fn fetch_remote_manifest_with_key(
    key: &[u8; KEY_LEN],
    s3: &S3Client,
) -> Result<Option<SyncManifest>> {
    let raw = match s3.get_object(MANIFEST_FILE_KEY)? {
        Some(b) => b,
        None => return Ok(None),
    };

    let plain = decrypt_payload(key, &raw)
        .map_err(|_| AppError::Invalid("云端清单解密失败：可能使用了不同的恢复密钥或工作空间".into()))?;

    let manifest: SyncManifest = serde_json::from_slice(&plain)
        .map_err(|e| AppError::Invalid(format!("解析云端清单失败: {e}")))?;
    Ok(Some(manifest))
}

fn fetch_remote_vault_data(
    vault: &Vault,
    s3: &S3Client,
    enc_key: &[u8; KEY_LEN],
) -> Result<Option<VaultData>> {
    let Some(manifest) = fetch_remote_manifest(vault, s3)? else {
        return Ok(None);
    };
    fetch_remote_vault_data_with(s3, enc_key, &manifest)
}

fn fetch_remote_vault_data_with(
    s3: &S3Client,
    enc_key: &[u8; KEY_LEN],
    manifest: &SyncManifest,
) -> Result<Option<VaultData>> {
    let Some(entry) = manifest.objects.get("data/identities.json") else {
        return Ok(None);
    };
    let Some(raw) = s3.get_object(&format!("obj/{}", entry.object_name))? else {
        return Ok(None);
    };
    let plain = decrypt_payload(enc_key, &raw)?;
    let data = serde_json::from_slice(&plain)
        .map_err(|e| AppError::Invalid(format!("解析云端身份数据失败: {e}")))?;
    Ok(Some(data))
}

fn parse_cloud_vault_header(raw: &[u8]) -> Result<VaultHeader> {
    if let Ok(wrapped) = serde_json::from_slice::<CloudVaultHeader>(raw) {
        let mut header = wrapped.header;
        header.kdf.clamp_to_safe_bounds();
        return Ok(header);
    }
    let mut header: VaultHeader = serde_json::from_slice(raw)
        .map_err(|e| AppError::Invalid(format!("云端工作空间头部损坏: {e}")))?;
    header.kdf.clamp_to_safe_bounds();
    Ok(header)
}

/// 把本地 vault 头部上传到固定路径，供新设备用恢复密钥解开 MK。
pub fn upload_vault_header(vault: &Vault, s3: &S3Client) -> Result<()> {
    let payload = CloudVaultHeader {
        version: 1,
        header: vault.header().clone(),
    };
    let bytes = serde_json::to_vec(&payload)?;
    s3.put_object(VAULT_HEADER_KEY, &bytes)?;
    match s3.get_object(VAULT_HEADER_KEY)? {
        Some(got) if got == bytes => Ok(()),
        Some(_) => Err(AppError::Invalid(
            "云端换机头部已写入但回读内容不一致，请重试推送".into(),
        )),
        None => Err(AppError::Invalid(
            "云端换机头部上传后无法读回。请检查存储桶权限是否允许读取刚写入的对象".into(),
        )),
    }
}

pub fn vault_header_exists(s3: &S3Client) -> bool {
    s3.object_exists(VAULT_HEADER_KEY)
}

pub fn fetch_vault_header(s3: &S3Client) -> Result<VaultHeader> {
    let raw = s3.get_object(VAULT_HEADER_KEY)?.ok_or_else(|| {
        AppError::Invalid(
            "云端没有工作空间头部，无法用恢复密钥换机还原。请确认旧设备已用本版本成功推送（成功提示应包含「换机恢复头部」），且新设备填写的 Bucket 与路径前缀与旧设备完全一致。"
                .into(),
        )
    })?;
    parse_cloud_vault_header(&raw)
}

fn unlock_header_with_recovery(header: &VaultHeader, recovery_key: &str) -> Result<crypto::MasterKey> {
    let secret = recovery::parse(recovery_key)?;
    envelope::unwrap_with_recovery(&header.envelopes.recovery, &secret)
}

/// 用恢复密钥验证云端头部并预览清单（不落盘）。
pub fn preview_cloud_restore(s3: &S3Client, recovery_key: &str) -> Result<CloudRestorePreview> {
    let header = fetch_vault_header(s3)?;
    let mk = unlock_header_with_recovery(&header, recovery_key)?;
    let workspace_id = header.workspace_id.clone();
    let vault = Vault::from_header_unlocked(header, mk);

    let manifest = match fetch_remote_manifest(&vault, s3) {
        Ok(m) => m,
        Err(_) => {
            return Ok(CloudRestorePreview {
                workspace_id,
                updated_at: None,
                identity_count: 0,
                key_count: 0,
                repo_count: 0,
                has_manifest: false,
            });
        }
    };

    let Some(manifest) = manifest else {
        return Ok(CloudRestorePreview {
            workspace_id,
            updated_at: None,
            identity_count: 0,
            key_count: 0,
            repo_count: 0,
            has_manifest: false,
        });
    };

    let mut identity_count = 0usize;
    let mut key_count = manifest
        .objects
        .keys()
        .filter(|k| k.starts_with("keys/") && k.ends_with(".key"))
        .count();
    let mut repo_count = 0usize;

    if let Some(entry) = manifest.objects.get("data/identities.json") {
        if let Some(raw) = s3.get_object(&format!("obj/{}", entry.object_name))? {
            let enc_key = vault.subkey(LABEL_SYNC_OBJECT)?;
            if let Ok(plain) = decrypt_payload(&enc_key, &raw) {
                if let Ok(data) = serde_json::from_slice::<VaultData>(&plain) {
                    identity_count = data.identities.len();
                    key_count = data.keys.len();
                    repo_count = data.repos.len();
                }
            }
        }
    }

    Ok(CloudRestorePreview {
        workspace_id,
        updated_at: Some(manifest.updated_at),
        identity_count,
        key_count,
        repo_count,
        has_manifest: true,
    })
}

/// 在目标目录重建工作空间并拉取云端数据。
pub fn restore_from_cloud(
    root: &std::path::Path,
    new_password: &str,
    recovery_key: &str,
    s3: &S3Client,
    include_repos: bool,
) -> Result<(Vault, SyncResult)> {
    let header = fetch_vault_header(s3)?;
    let mk = unlock_header_with_recovery(&header, recovery_key)?;
    let mem = Vault::from_header_unlocked(header.clone(), mk.clone());
    if fetch_remote_manifest(&mem, s3)?.is_none() {
        return Err(AppError::Invalid(
            "已用恢复密钥解开云端头部，但尚未找到加密清单。请先在旧设备上完成一次云同步推送。".into(),
        ));
    }

    let vault = Vault::restore(root, header, mk, new_password)?;
    let result = match pull_from_cloud_inner(&vault, s3, true, include_repos).map(|(r, _)| r) {
        Ok(r) => r,
        Err(e) => SyncResult {
            synced_at: iso_now(),
            objects_transferred: 0,
            identity_count: 0,
            key_count: 0,
            repo_count: 0,
            message: format!(
                "工作空间已用恢复密钥重建，但拉取云端数据失败：{e}。进入应用后可在「云同步」中重试。"
            ),
        },
    };
    if let Err(e) = upload_vault_header(&vault, s3) {
        log::warn!("恢复后回写云端头部失败（不影响本次还原）: {e}");
    }
    Ok((vault, result))
}

/// 仅本地资产计数，不访问云端。供页面首屏即时渲染。
pub fn local_sync_status(vault: &Vault, configured: bool) -> Result<CloudSyncStatus> {
    if !vault.is_unlocked() {
        return Err(AppError::Locked);
    }
    let data = store::load_data(vault)?;
    let machine_id = crate::app_config::AppConfig::current_machine_id();
    Ok(CloudSyncStatus {
        remote_exists: false,
        remote_updated_at: None,
        remote_workspace_id: None,
        local_identity_count: data.identities.len(),
        local_key_count: data.keys.len(),
        local_repo_count: data
            .repos
            .iter()
            .filter(|r| r.machine_id == machine_id || r.machine_id.is_empty())
            .count(),
        status: if configured {
            "checking".into()
        } else {
            "unconfigured".into()
        },
        header_ready: false,
    })
}

/// 查询云端与本地同步比对状态（含完整方向判断，可能额外下载身份密文）。
pub fn get_sync_status(vault: &Vault, s3: &S3Client) -> Result<CloudSyncStatus> {
    get_sync_status_inner(vault, s3, false)
}

/// 轻量查询：只拉清单 + 探测头部，不下载身份密文。适合进页后台刷新。
pub fn get_sync_status_lite(vault: &Vault, s3: &S3Client) -> Result<CloudSyncStatus> {
    get_sync_status_inner(vault, s3, true)
}

fn get_sync_status_inner(vault: &Vault, s3: &S3Client, lite: bool) -> Result<CloudSyncStatus> {
    if !vault.is_unlocked() {
        return Err(AppError::Locked);
    }

    let data = store::load_data(vault)?;
    let machine_id = crate::app_config::AppConfig::current_machine_id();
    let local_identity_count = data.identities.len();
    let local_key_count = data.keys.len();
    let local_repo_count = data
        .repos
        .iter()
        .filter(|r| r.machine_id == machine_id || r.machine_id.is_empty())
        .count();
    let enc_key = vault.subkey(LABEL_SYNC_OBJECT)?;

    // 头部探测与清单拉取并行，避免串行两次往返。
    let (header_ready, remote) = std::thread::scope(|scope| {
        let header_h = scope.spawn(|| vault_header_exists(s3));
        let remote_h = scope.spawn(|| fetch_remote_manifest_with_key(&enc_key, s3));
        let header_ready = header_h.join().unwrap_or(false);
        let remote = remote_h.join().unwrap_or(Ok(None));
        (header_ready, remote)
    });

    let remote = match remote {
        Ok(m) => m,
        Err(_) => {
            return Ok(CloudSyncStatus {
                remote_exists: false,
                remote_updated_at: None,
                remote_workspace_id: None,
                local_identity_count,
                local_key_count,
                local_repo_count,
                status: "not_synced".into(),
                header_ready,
            });
        }
    };

    if let Some(m) = remote {
        let is_same_ws = m.workspace_id == vault.workspace_id();
        let status = if !is_same_ws {
            "different_workspace".to_string()
        } else {
            let remote_data = if lite {
                None
            } else {
                fetch_remote_vault_data_with(s3, &enc_key, &m).ok().flatten()
            };

            if let Some(remote_data) = remote_data {
                // 构建本地数据在“推送到云端”时产生的预期云端 VaultData 结构：
                // 即：以本地数据为主，将云端其他机器的仓库记录（machine_id != current_machine_id）组合进来。
                let mut projected = crate::sys::vault_data_for_sync(&data);
                let (composed_repos, composed_deleted_repos) =
                    crate::model::compose_cloud_repos(&projected, &remote_data, &machine_id);
                projected.repos = composed_repos;
                projected.deleted_repos = composed_deleted_repos;

                let local_id_json = serde_json::to_vec(&projected)?;
                let local_id_hash = sha256_hex(&local_id_json);
                let remote_id_hash = m
                    .objects
                    .get("data/identities.json")
                    .map(|e| e.sha256.as_str())
                    .unwrap_or_default();

                let local_ssh = ssh_text_for_sync(vault).unwrap_or_default();
                let local_ssh_hash = sha256_hex(local_ssh.as_bytes());
                let remote_ssh_hash = m
                    .objects
                    .get("ssh/config")
                    .map(|e| e.sha256.as_str())
                    .unwrap_or_default();

                let ssh_matched = m.objects.get("ssh/config").is_none() || local_ssh_hash == remote_ssh_hash;

                if local_id_hash == remote_id_hash && ssh_matched && !extra_objects_diverged(vault, &m) {
                    "synced".to_string()
                } else if is_remote_ahead(&data, &remote_data, &machine_id) {
                    "remote_ahead".to_string()
                } else {
                    "local_ahead".to_string()
                }
            } else {
                let local_id_json = serde_json::to_vec(&crate::sys::vault_data_for_sync(&data))?;
                let local_id_hash = sha256_hex(&local_id_json);
                let remote_id_hash = m
                    .objects
                    .get("data/identities.json")
                    .map(|e| e.sha256.as_str())
                    .unwrap_or_default();

                if local_id_hash == remote_id_hash && !extra_objects_diverged(vault, &m) {
                    "synced".to_string()
                } else {
                    "local_ahead".to_string()
                }
            }
        };

        Ok(CloudSyncStatus {
            remote_exists: true,
            remote_updated_at: Some(m.updated_at),
            remote_workspace_id: Some(m.workspace_id),
            local_identity_count,
            local_key_count,
            local_repo_count,
            status,
            header_ready,
        })
    } else {
        Ok(CloudSyncStatus {
            remote_exists: false,
            remote_updated_at: None,
            remote_workspace_id: None,
            local_identity_count,
            local_key_count,
            local_repo_count,
            status: "not_synced".into(),
            header_ready,
        })
    }
}

fn is_remote_ahead(local: &VaultData, remote: &VaultData, machine_id: &str) -> bool {
    // 1. 远程是否有本地缺失或更新的身份
    for remote_item in &remote.identities {
        if local.deleted_identities.contains_key(&remote_item.id) {
            continue;
        }
        match local.identities.iter().find(|i| i.id == remote_item.id) {
            None => return true,
            Some(local_item) => {
                if crate::model::timestamp_newer_or_eq(&remote_item.updated_at, &local_item.updated_at)
                    && remote_item.updated_at != local_item.updated_at
                {
                    return true;
                }
            }
        }
    }

    // 2. 远程是否有本地缺失的密钥
    for remote_key in &remote.keys {
        if !local.keys.iter().any(|k| k.id == remote_key.id) {
            return true;
        }
    }

    // 3. 远程是否有本地尚未应用的删除墓碑（身份或本机器仓库）
    for (id, ts) in &remote.deleted_identities {
        if local.identities.iter().any(|i| i.id == *id) {
            return true;
        }
        match local.deleted_identities.get(id) {
            None => return true,
            Some(local_ts) if ts > local_ts => return true,
            _ => {}
        }
    }

    for (id, ts) in &remote.deleted_repos {
        if local.repos.iter().any(|r| r.id == *id && (r.machine_id == machine_id || r.machine_id.is_empty())) {
            return true;
        }
        match local.deleted_repos.get(id) {
            None => return true,
            Some(local_ts) if ts > local_ts => return true,
            _ => {}
        }
    }

    // 4. 远程是否有属于本机器、但本地缺失的仓库登记
    for remote_repo in &remote.repos {
        if remote_repo.machine_id == machine_id {
            if !local.repos.iter().any(|r| r.id == remote_repo.id) {
                return true;
            }
        }
    }

    // 5. 远程是否有本地缺失的 clone_history
    for (k, _) in &remote.clone_history {
        if !local.clone_history.contains_key(k) {
            return true;
        }
    }

    false
}

/// 推送到云端（Push）
pub fn push_to_cloud(vault: &Vault, s3: &S3Client) -> Result<SyncResult> {
    if !vault.is_unlocked() {
        return Err(AppError::Locked);
    }

    let enc_key = vault.subkey(LABEL_SYNC_OBJECT)?;
    let now_str = iso_now();

    // 1. 收集本地需同步对象（出机前先去掉本机盘符，避免换机路径互相覆盖）
    let mut data = store::load_data(vault)?;
    let original_keys = data.keys.clone();
    crate::sys::portableize_vault_data(&mut data);
    let machine_id = crate::app_config::AppConfig::current_machine_id();
    let repos_changed = crate::model::claim_unowned_repos(&mut data, &machine_id)
        | crate::model::keep_repos_for_machine(&mut data, &machine_id);
    if data.keys != original_keys || repos_changed {
        store::save_data(vault, &data)?;
    }
    let secrets = store::load_secrets(vault)?;

    let mut upload_data = data.clone();
    if let Ok(Some(remote_data)) = fetch_remote_vault_data(vault, s3, &enc_key) {
        let (repos, deleted_repos) =
            crate::model::compose_cloud_repos(&upload_data, &remote_data, &machine_id);
        upload_data.repos = repos;
        upload_data.deleted_repos = deleted_repos;
    }

    let mut logical_objects: HashMap<String, Vec<u8>> = HashMap::new();

    // (1) 身份与密钥元数据 + 各机器仓库全集（本机只嵌自己的那一份）
    logical_objects.insert("data/identities.json".into(), serde_json::to_vec(&upload_data)?);
    // (2) 机密口令
    logical_objects.insert("data/secrets.json".into(), serde_json::to_vec(&secrets)?);
    let totp_data = store::load_totp(vault)?;
    logical_objects.insert("data/totp.json".into(), serde_json::to_vec(&totp_data)?);
    let account_data = store::load_accounts(vault)?;
    logical_objects.insert("data/accounts.json".into(), serde_json::to_vec(&account_data)?);
    for hash in store::list_icon_hashes(vault) {
        if let Ok(bytes) = store::load_icon(vault, &hash) {
            logical_objects.insert(format!("icons/{hash}.webp"), bytes);
        }
    }
    // (3) 所有私钥副本
    for key in &data.keys {
        if store::key_exists(vault, &key.id) {
            if let Ok(priv_bytes) = store::load_key(vault, &key.id) {
                logical_objects.insert(format!("keys/{}.key", key.id), priv_bytes);
            }
        }
    }
    // (4) 工作空间 SSH config：只上传可移植形态，绝不上传本机重写后的绝对路径。
    if let Some(portable) = ssh_text_for_sync(vault) {
        logical_objects.insert("ssh/config".into(), portable.into_bytes());
    }

    let mut manifest_objects = HashMap::new();
    let mut transferred_count = 0;

    // 2. 加密并上传每个对象（按 HMAC 散列命名）
    for (logical_path, plain_bytes) in &logical_objects {
        // 使用 master_key 计算唯一的 HMAC 对象名
        let obj_name = vault.object_name(logical_path)?;
        let cloud_key = format!("obj/{}", obj_name);
        let hash = sha256_hex(plain_bytes);

        // 加密
        let encrypted = encrypt_payload(&enc_key, plain_bytes)?;
        s3.put_object(&cloud_key, &encrypted)?;
        transferred_count += 1;

        manifest_objects.insert(
            logical_path.clone(),
            SyncObjectEntry {
                object_name: obj_name,
                sha256: hash,
                size: plain_bytes.len(),
                updated_at: now_str.clone(),
            },
        );
    }

    // 3. 构建并上传 manifest.enc
    let manifest = SyncManifest {
        version: 1,
        workspace_id: vault.workspace_id().to_string(),
        updated_at: now_str.clone(),
        client_name: whoami_string(),
        objects: manifest_objects,
    };

    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let encrypted_manifest = encrypt_payload(&enc_key, &manifest_bytes)?;
    s3.put_object(MANIFEST_FILE_KEY, &encrypted_manifest)?;

    upload_vault_header(vault, s3).map_err(|e| {
        AppError::Invalid(format!(
            "加密对象已上传，但换机恢复头部写入失败：{e}。新设备将无法用恢复密钥还原，请重试推送。"
        ))
    })?;

    // 4. 保存本地同步快照
    let local_manifest_path = vault.root().join("sync").join(MANIFEST_FILE_KEY);
    crate::vault::atomic_write(&local_manifest_path, &encrypted_manifest)?;

    if let Err(e) = retain_push_snapshot(vault, s3, &now_str, &data, &secrets) {
        log::warn!("保存云端历史快照失败（当前同步仍有效）: {e}");
    }

    Ok(SyncResult {
        synced_at: now_str,
        objects_transferred: transferred_count,
        identity_count: data.identities.len(),
        key_count: data.keys.len(),
        repo_count: data.repos.len(),
        message: format!(
            "已成功推送到云端存储，包含 {} 个加密对象，并已写入换机恢复头部",
            transferred_count
        ),
    })
}

/// 从云端拉取并还原到本地（Pull）
pub fn pull_from_cloud(vault: &Vault, s3: &S3Client) -> Result<SyncResult> {
    pull_from_cloud_inner(vault, s3, true, false).map(|(r, _)| r)
}

fn pull_from_cloud_inner(
    vault: &Vault,
    s3: &S3Client,
    apply_remote_ssh: bool,
    adopt_remote_repos: bool,
) -> Result<(SyncResult, bool)> {
    if !vault.is_unlocked() {
        return Err(AppError::Locked);
    }

    let enc_key = vault.subkey(LABEL_SYNC_OBJECT)?;
    let manifest = fetch_remote_manifest(vault, s3)?
        .ok_or_else(|| AppError::Invalid("云端尚未存在任何同步备份".into()))?;

    let local_identities_before = store::load_data(vault)?.identities;
    let mut transferred_count = 0;
    let mut remote_data_snap: Option<VaultData> = None;
    let mut remote_secrets_snap: Option<Secrets> = None;
    let mut remote_totp_snap: Option<crate::model::TotpData> = None;
    let mut remote_account_snap: Option<crate::model::AccountData> = None;
    let mut remote_ssh_snap: Option<String> = None;
    let mut pulled_key_ids: Vec<String> = Vec::new();

    for (logical_path, entry) in &manifest.objects {
        let cloud_key = format!("obj/{}", entry.object_name);
        let raw_enc = match s3.get_object(&cloud_key)? {
            Some(b) => b,
            None => {
                log::warn!("云端缺失对象 {} ({})", logical_path, entry.object_name);
                continue;
            }
        };

        let plain = decrypt_payload(&enc_key, &raw_enc)?;
        transferred_count += 1;

        if logical_path == "data/identities.json" {
            let remote_data: VaultData = serde_json::from_slice(&plain)
                .map_err(|e| AppError::Invalid(format!("解析云端身份数据失败: {e}")))?;
            remote_data_snap = Some(remote_data);
        } else if logical_path == "data/secrets.json" {
            let remote_secrets: Secrets = serde_json::from_slice(&plain)
                .map_err(|e| AppError::Invalid(format!("解析云端口令数据失败: {e}")))?;
            remote_secrets_snap = Some(remote_secrets);
        } else if logical_path == "data/totp.json" {
            remote_totp_snap = Some(
                serde_json::from_slice(&plain)
                    .map_err(|e| AppError::Invalid(format!("解析云端 TOTP 数据失败: {e}")))?,
            );
        } else if logical_path == "data/accounts.json" {
            remote_account_snap = Some(
                serde_json::from_slice(&plain)
                    .map_err(|e| AppError::Invalid(format!("解析云端账号数据失败: {e}")))?,
            );
        } else if let Some(hash) = logical_path
            .strip_prefix("icons/")
            .and_then(|s| s.strip_suffix(".webp"))
        {
            store::save_icon(vault, hash, &plain)?;
        } else if logical_path == "ssh/config" {
            remote_ssh_snap = Some(String::from_utf8_lossy(&plain).to_string());
        } else if let Some(key_id) = logical_path.strip_prefix("keys/").and_then(|s| s.strip_suffix(".key")) {
            store::save_key(vault, key_id, &plain)?;
            pulled_key_ids.push(key_id.to_string());
        }
    }

    // 网络往返后再读本地，避免同步期间的删除/编辑被旧快照盖回去。
    let mut current_data = store::load_data(vault)?;
    let machine_id = crate::app_config::AppConfig::current_machine_id();
    crate::model::claim_unowned_repos(&mut current_data, &machine_id);
    if let Some(remote_data) = remote_data_snap.clone() {
        current_data = crate::model::merge_vault_data(current_data, remote_data.clone());
        if adopt_remote_repos {
            crate::model::adopt_repos_as_machine(&mut current_data, &remote_data, &machine_id);
        }
    }
    crate::model::keep_repos_for_machine(&mut current_data, &machine_id);
    crate::sys::portableize_vault_data(&mut current_data);
    let mut current_secrets = store::load_secrets(vault)?;
    let local_totp = store::load_totp(vault).unwrap_or_default();
    let local_acc = store::load_accounts(vault).unwrap_or_default();
    if let Some(remote_secrets) = &remote_secrets_snap {
        current_secrets = crate::model::merge_secrets_with_meta(
            current_secrets,
            remote_secrets,
            Some(&local_totp),
            remote_totp_snap.as_ref(),
            Some(&local_acc),
            remote_account_snap.as_ref(),
        );
    }
    let merged_totp = if let Some(remote_totp) = remote_totp_snap.clone() {
        crate::model::merge_totp_data(local_totp.clone(), remote_totp)
    } else {
        local_totp.clone()
    };
    let merged_acc = if let Some(remote_acc) = remote_account_snap.clone() {
        crate::model::merge_account_data(local_acc.clone(), remote_acc)
    } else {
        local_acc.clone()
    };
    // 先落机密再落条目，避免列表已可见但种子还在旧 secrets.enc 上。
    store::save_secrets(vault, &current_secrets)?;
    store::save_totp(vault, &merged_totp)?;
    store::save_accounts(vault, &merged_acc)?;
    for key_id in &pulled_key_ids {
        if let (Some(key_rec), Ok(raw)) = (
            current_data.keys.iter().find(|k| k.id == *key_id),
            store::load_key(vault, key_id),
        ) {
            let _ = deploy_key_to_workspace(vault, key_rec, &raw);
        }
    }

    if let Some(snapshot) = &remote_ssh_snap {
        apply_pulled_ssh(
            vault,
            snapshot,
            apply_remote_ssh,
            &local_identities_before,
            remote_data_snap.as_ref(),
            &current_data,
        )?;
    }

    store::save_data(vault, &current_data)?;
    let ssh_now = read_ssh_config_bytes(vault).unwrap_or_default();
    let remote_hash = match remote_data_snap {
        Some(rd) => Some(stable_state_hash(
            &rd,
            remote_secrets_snap.as_ref().unwrap_or(&Secrets::default()),
            remote_ssh_snap.as_deref().unwrap_or(""),
            remote_totp_snap.as_ref().unwrap_or(&crate::model::TotpData::default()),
            remote_account_snap.as_ref().unwrap_or(&crate::model::AccountData::default()),
        )),
        None => None,
    };
    let need_push = match remote_hash {
        Some(h) => h != stable_state_hash(&current_data, &current_secrets, &ssh_now, &merged_totp, &merged_acc),
        None => local_has_syncable_assets(vault)?,
    };

    Ok((
        SyncResult {
            synced_at: manifest.updated_at,
            objects_transferred: transferred_count,
            identity_count: current_data.identities.len(),
            key_count: current_data.keys.len(),
            repo_count: current_data.repos.len(),
            message: format!("已成功从云端拉取并同步 {} 个加密对象", transferred_count),
        },
        need_push,
    ))
}

fn whoami_string() -> String {
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "user".into());
    let host = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "pc".into());
    format!("{user}@{host}")
}

fn snapshot_id_from_time(iso: &str) -> String {
    iso.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-' => c,
            _ => '-',
        })
        .collect()
}

fn history_object_key(id: &str) -> String {
    format!("history/{id}.enc")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotIndex {
    pub items: Vec<SnapshotMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotMeta {
    pub id: String,
    pub created_at: String,
    pub client_name: String,
    pub identity_count: usize,
    pub key_count: usize,
    pub repo_count: usize,
    /// 属于最近 N 份（列出时计算，不入库）。
    #[serde(default)]
    pub is_recent: bool,
    /// 近 14 天内该日第一份（列出时计算，不入库）。
    #[serde(default)]
    pub is_daily_first: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotPayload {
    pub meta: SnapshotMeta,
    pub data: VaultData,
    pub secrets: Secrets,
    pub private_keys: HashMap<String, String>,
    pub ssh_config: Option<String>,
    #[serde(default)]
    pub totp_data: crate::model::TotpData,
    #[serde(default)]
    pub account_data: crate::model::AccountData,
    #[serde(default)]
    pub icons: HashMap<String, String>,
}

fn load_snapshot_index(vault: &Vault, s3: &S3Client) -> Result<SnapshotIndex> {
    let enc_key = vault.subkey(LABEL_SYNC_OBJECT)?;
    match s3.get_object(HISTORY_INDEX_KEY)? {
        Some(raw) => {
            let plain = decrypt_payload(&enc_key, &raw)
                .map_err(|_| AppError::Invalid("历史快照索引解密失败".into()))?;
            Ok(serde_json::from_slice(&plain)?)
        }
        None => Ok(SnapshotIndex { items: Vec::new() }),
    }
}

fn save_snapshot_index(vault: &Vault, s3: &S3Client, index: &SnapshotIndex) -> Result<()> {
    let enc_key = vault.subkey(LABEL_SYNC_OBJECT)?;
    let bytes = serde_json::to_vec(index)?;
    s3.put_object(HISTORY_INDEX_KEY, &encrypt_payload(&enc_key, &bytes)?)
}

fn snapshot_date_key(iso: &str) -> String {
    iso.get(..10).unwrap_or(iso).to_string()
}

fn utc_date_key(dt: time::OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        dt.year(),
        u8::from(dt.month()),
        dt.day()
    )
}

fn daily_cutoff_key(now: time::OffsetDateTime, days: i64) -> String {
    let days = days.max(1);
    let start = now.saturating_sub(time::Duration::days(days - 1));
    utc_date_key(start)
}

fn snapshot_keep_ids(
    items: &[SnapshotMeta],
    now: time::OffsetDateTime,
    recent_keep: usize,
    daily_days: i64,
) -> (std::collections::HashSet<String>, std::collections::HashSet<String>, std::collections::HashSet<String>) {
    let mut sorted: Vec<&SnapshotMeta> = items.iter().collect();
    sorted.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let mut recent = std::collections::HashSet::new();
    for item in sorted.iter().take(recent_keep) {
        recent.insert(item.id.clone());
    }

    let cutoff = daily_cutoff_key(now, daily_days);
    let mut earliest_by_day: HashMap<String, &SnapshotMeta> = HashMap::new();
    for item in items {
        let day = snapshot_date_key(&item.created_at);
        if day < cutoff {
            continue;
        }
        match earliest_by_day.get(&day) {
            Some(prev) if prev.created_at <= item.created_at => {}
            _ => {
                earliest_by_day.insert(day, item);
            }
        }
    }
    let daily: std::collections::HashSet<String> =
        earliest_by_day.values().map(|m| m.id.clone()).collect();

    let keep: std::collections::HashSet<String> = recent.union(&daily).cloned().collect();
    (keep, recent, daily)
}

/// 保留最近 10 份，以及近 14 天每天的第一份；返回被删除的 id。
pub fn prune_snapshot_index(items: &mut Vec<SnapshotMeta>) -> Vec<String> {
    prune_snapshot_index_at(
        items,
        time::OffsetDateTime::now_utc(),
        MAX_RECENT_SNAPSHOTS,
        DAILY_SNAPSHOT_DAYS,
    )
}

pub fn prune_snapshot_index_at(
    items: &mut Vec<SnapshotMeta>,
    now: time::OffsetDateTime,
    recent_keep: usize,
    daily_days: i64,
) -> Vec<String> {
    let (keep, recent, daily) = snapshot_keep_ids(items, now, recent_keep, daily_days);
    let dropped: Vec<String> = items
        .iter()
        .filter(|m| !keep.contains(&m.id))
        .map(|m| m.id.clone())
        .collect();
    items.retain(|m| keep.contains(&m.id));
    for m in items.iter_mut() {
        m.is_recent = recent.contains(&m.id);
        m.is_daily_first = daily.contains(&m.id);
    }
    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    dropped
}

fn annotate_snapshot_roles(items: &mut [SnapshotMeta], now: time::OffsetDateTime) {
    let (_keep, recent, daily) = snapshot_keep_ids(items, now, MAX_RECENT_SNAPSHOTS, DAILY_SNAPSHOT_DAYS);
    for m in items.iter_mut() {
        m.is_recent = recent.contains(&m.id);
        m.is_daily_first = daily.contains(&m.id);
    }
}

fn collect_private_keys(vault: &Vault, data: &VaultData) -> HashMap<String, String> {
    let mut private_keys = HashMap::new();
    for key in &data.keys {
        if store::key_exists(vault, &key.id) {
            if let Ok(raw) = store::load_key(vault, &key.id) {
                private_keys.insert(key.id.clone(), B64.encode(raw));
            }
        }
    }
    private_keys
}

fn apply_pulled_ssh(
    vault: &Vault,
    remote_ssh: &str,
    apply_remote_ssh: bool,
    local_identities_before: &[crate::model::Identity],
    remote_data: Option<&VaultData>,
    merged: &VaultData,
) -> Result<()> {
    if apply_remote_ssh {
        // persist 会按本机工作空间展开；云端正文本身必须是可移植形态。
        return crate::sys::persist_ssh_config(Some(vault.root()), remote_ssh);
    }
    let local_ssh = read_ssh_config_bytes(vault).unwrap_or_default();
    let local_canonical = crate::sys::canonical_ssh_for_sync(&local_ssh);
    let remote_canonical = crate::sys::canonical_ssh_for_sync(remote_ssh);
    let keep: HashSet<String> = merged.identities.iter().map(|i| i.host_alias.clone()).collect();
    let mut text = crate::ssh::managed::merge_managed_prefer_local(
        &local_canonical,
        &remote_canonical,
        &keep,
    );
    let mut drop_aliases = HashSet::new();
    for ident in local_identities_before.iter().chain(remote_data.map(|d| d.identities.as_slice()).unwrap_or(&[])) {
        if merged.deleted_identities.contains_key(&ident.id) {
            drop_aliases.insert(ident.host_alias.clone());
        }
    }
    for alias in drop_aliases {
        text = crate::ssh::managed::remove(&text, &alias);
    }
    crate::sys::persist_ssh_config(Some(vault.root()), &text)
}

fn read_ssh_config_bytes(vault: &Vault) -> Option<String> {
    let ws = crate::sys::workspace_ssh_config(vault.root());
    if let Ok(t) = std::fs::read_to_string(&ws) {
        return Some(t);
    }
    std::fs::read_to_string(crate::sys::ssh_dir().join("config")).ok()
}

/// 出机 SSH 正文：本机绝对路径先收成占位符，再上传或写入快照。
fn ssh_text_for_sync(vault: &Vault) -> Option<String> {
    read_ssh_config_bytes(vault).map(|text| crate::sys::canonical_ssh_for_sync(&text))
}

fn retain_push_snapshot(
    vault: &Vault,
    s3: &S3Client,
    now_str: &str,
    data: &VaultData,
    secrets: &Secrets,
) -> Result<()> {
    let id = snapshot_id_from_time(now_str);
    if id.is_empty() {
        return Ok(());
    }
    let meta = SnapshotMeta {
        id: id.clone(),
        created_at: now_str.to_string(),
        client_name: whoami_string(),
        identity_count: data.identities.len(),
        key_count: data.keys.len(),
        repo_count: data.repos.len(),
        is_recent: false,
        is_daily_first: false,
    };
    let mut snap_data = data.clone();
    crate::sys::portableize_vault_data(&mut snap_data);
    let payload = SnapshotPayload {
        meta: meta.clone(),
        data: snap_data,
        secrets: secrets.clone(),
        private_keys: collect_private_keys(vault, data),
        ssh_config: ssh_text_for_sync(vault),
        totp_data: store::load_totp(vault).unwrap_or_default(),
        account_data: store::load_accounts(vault).unwrap_or_default(),
        icons: collect_icons(vault),
    };
    let enc_key = vault.subkey(LABEL_SYNC_OBJECT)?;
    let encrypted = encrypt_payload(&enc_key, &serde_json::to_vec(&payload)?)?;
    s3.put_object(&history_object_key(&id), &encrypted)?;

    let mut index = load_snapshot_index(vault, s3)?;
    index.items.retain(|m| m.id != id);
    index.items.push(meta);
    let dropped = prune_snapshot_index(&mut index.items);
    save_snapshot_index(vault, s3, &index)?;
    for old in dropped {
        let _ = s3.delete_object(&history_object_key(&old));
    }
    Ok(())
}

/// 列出云端滚动历史快照（新→旧）。
pub fn list_snapshots(vault: &Vault, s3: &S3Client) -> Result<Vec<SnapshotMeta>> {
    if !vault.is_unlocked() {
        return Err(AppError::Locked);
    }
    let mut index = load_snapshot_index(vault, s3)?;
    index.items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    annotate_snapshot_roles(&mut index.items, time::OffsetDateTime::now_utc());
    Ok(index.items)
}

/// 用指定历史快照覆盖身份/密钥/口令与 SSH config（仓库记录合并保留本机路径）。
pub fn restore_snapshot(vault: &Vault, s3: &S3Client, snapshot_id: &str) -> Result<SyncResult> {
    if !vault.is_unlocked() {
        return Err(AppError::Locked);
    }
    let id = snapshot_id.trim();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(AppError::Invalid("无效的快照编号".into()));
    }
    let enc_key = vault.subkey(LABEL_SYNC_OBJECT)?;
    let raw = s3
        .get_object(&history_object_key(id))?
        .ok_or_else(|| AppError::Invalid("云端找不到该历史快照".into()))?;
    let plain = decrypt_payload(&enc_key, &raw)
        .map_err(|_| AppError::Invalid("历史快照解密失败".into()))?;
    let payload: SnapshotPayload = serde_json::from_slice(&plain)?;
    if payload.meta.id != id && payload.meta.created_at != id {
        return Err(AppError::Invalid("快照编号与内容不一致".into()));
    }

    let mut current_repos = store::load_data(vault)?.repos;
    let mut data = payload.data;
    crate::sys::portableize_vault_data(&mut data);
    for repo in data.repos.drain(..) {
        if let Some(existing) = current_repos.iter_mut().find(|r| r.id == repo.id) {
            *existing = repo;
        } else {
            current_repos.push(repo);
        }
    }
    data.repos = current_repos;
    let machine_id = crate::app_config::AppConfig::current_machine_id();
    crate::model::claim_unowned_repos(&mut data, &machine_id);
    crate::model::keep_repos_for_machine(&mut data, &machine_id);

    store::save_data(vault, &data)?;
    store::save_secrets(vault, &payload.secrets)?;
    store::save_totp(vault, &payload.totp_data)?;
    store::save_accounts(vault, &payload.account_data)?;
    for (hash, b64) in &payload.icons {
        if let Ok(bytes) = B64.decode(b64) {
            let _ = store::save_icon(vault, hash, &bytes);
        }
    }

    for (key_id, b64_bytes) in &payload.private_keys {
        if let Ok(key_bytes) = B64.decode(b64_bytes) {
            store::save_key(vault, key_id, &key_bytes)?;
        }
    }
    for key in &data.keys {
        if store::key_exists(vault, &key.id) {
            if let Ok(priv_bytes) = store::load_key(vault, &key.id) {
                let _ = deploy_key_to_workspace(vault, key, &priv_bytes);
            }
        }
    }
    if let Some(ssh) = &payload.ssh_config {
        crate::sys::persist_ssh_config(Some(vault.root()), ssh)?;
    }

    let created_at = payload.meta.created_at.clone();
    Ok(SyncResult {
        synced_at: created_at.clone(),
        objects_transferred: 1,
        identity_count: data.identities.len(),
        key_count: data.keys.len(),
        repo_count: data.repos.len(),
        message: format!("已从历史快照 {created_at} 恢复"),
    })
}

fn extra_objects_diverged(vault: &Vault, manifest: &SyncManifest) -> bool {
    let totp = store::load_totp(vault).unwrap_or_default();
    let accounts = store::load_accounts(vault).unwrap_or_default();
    let totp_hash = sha256_hex(&serde_json::to_vec(&totp).unwrap_or_default());
    let acc_hash = sha256_hex(&serde_json::to_vec(&accounts).unwrap_or_default());
    let remote_totp = manifest
        .objects
        .get("data/totp.json")
        .map(|e| e.sha256.as_str())
        .unwrap_or("");
    let remote_acc = manifest
        .objects
        .get("data/accounts.json")
        .map(|e| e.sha256.as_str())
        .unwrap_or("");
    (!totp.entries.is_empty() && totp_hash != remote_totp)
        || (!accounts.entries.is_empty() && acc_hash != remote_acc)
        || (manifest.objects.contains_key("data/totp.json") && totp_hash != remote_totp)
        || (manifest.objects.contains_key("data/accounts.json") && acc_hash != remote_acc)
}

fn collect_icons(vault: &Vault) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for hash in store::list_icon_hashes(vault) {
        if let Ok(bytes) = store::load_icon(vault, &hash) {
            out.insert(hash, B64.encode(bytes));
        }
    }
    out
}

fn stable_state_hash(
    data: &VaultData,
    secrets: &Secrets,
    ssh: &str,
    totp: &crate::model::TotpData,
    accounts: &crate::model::AccountData,
) -> String {
    let mut identities: Vec<&_> = data.identities.iter().collect();
    identities.sort_by(|a, b| a.id.cmp(&b.id));
    let mut keys: Vec<crate::model::KeyRecord> = data.keys.clone();
    for key in &mut keys {
        if let Some(p) = &key.deployed_path {
            key.deployed_path = Some(crate::sys::portable_deployed_path(p));
        }
    }
    keys.sort_by(|a, b| a.id.cmp(&b.id));
    let mut history: Vec<(&String, &String)> = data.clone_history.iter().collect();
    history.sort_by(|a, b| a.0.cmp(b.0));
    let mut passes: Vec<(&String, &String)> = secrets.key_passphrases.iter().collect();
    passes.sort_by(|a, b| a.0.cmp(b.0));
    let mut seeds: Vec<(&String, &String)> = secrets.totp_seeds.iter().collect();
    seeds.sort_by(|a, b| a.0.cmp(b.0));
    let mut account_secrets: Vec<_> = secrets.account_secrets.iter().collect();
    account_secrets.sort_by(|a, b| a.0.cmp(b.0));
    let mut totp_entries = totp.entries.clone();
    totp_entries.sort_by(|a, b| a.id.cmp(&b.id));
    let mut account_entries = accounts.entries.clone();
    account_entries.sort_by(|a, b| a.id.cmp(&b.id));
    let payload = serde_json::json!({
        "identities": identities,
        "keys": keys,
        "cloneHistory": history,
        "passphrases": passes,
        "githubPat": secrets.github_pat,
        "totpSeeds": seeds,
        "accountSecrets": account_secrets,
        "totpEntries": totp_entries,
        "accountEntries": account_entries,
        "ssh": crate::sys::canonical_ssh_for_sync(ssh),
    });
    sha256_hex(payload.to_string().as_bytes())
}

fn local_has_syncable_assets(vault: &Vault) -> Result<bool> {
    let data = store::load_data(vault)?;
    let totp = store::load_totp(vault).unwrap_or_default();
    let accounts = store::load_accounts(vault).unwrap_or_default();
    Ok(!data.identities.is_empty()
        || !data.keys.is_empty()
        || !totp.entries.is_empty()
        || !accounts.entries.is_empty())
}

/// 先拉取再按需推送。空本地不会覆盖已有云端。
pub fn pull_then_maybe_push(vault: &Vault, s3: &S3Client) -> Result<SyncResult> {
    if !vault.is_unlocked() {
        return Err(AppError::Locked);
    }
    let remote_before = fetch_remote_manifest(vault, s3)?;
    if remote_before
        .as_ref()
        .is_some_and(|m| m.workspace_id != vault.workspace_id())
    {
        return Err(AppError::Invalid(
            "云端工作空间与本地不一致，已跳过自动同步，请手动确认".into(),
        ));
    }

    if remote_before.is_some() {
        let (pulled, need_push) = pull_from_cloud_inner(vault, s3, false, false)?;
        if need_push && local_has_syncable_assets(vault)? {
            let pushed = push_to_cloud(vault, s3)?;
            return Ok(SyncResult {
                message: format!("已拉取并推送：{}", pushed.message),
                ..pushed
            });
        }
        if let Err(e) = upload_vault_header(vault, s3) {
            log::warn!("同步后补传工作空间头部失败: {e}");
        }
        return Ok(SyncResult {
            message: format!("已从云端拉取，本地无待推送变更。{}", pulled.message),
            ..pulled
        });
    }

    if local_has_syncable_assets(vault)? {
        let pushed = push_to_cloud(vault, s3)?;
        return Ok(SyncResult {
            message: format!("云端尚无备份，已推送：{}", pushed.message),
            ..pushed
        });
    }

    let data = store::load_data(vault)?;
    Ok(SyncResult {
        synced_at: iso_now(),
        objects_transferred: 0,
        identity_count: data.identities.len(),
        key_count: data.keys.len(),
        repo_count: data.repos.len(),
        message: "本地为空且云端无备份，已跳过".into(),
    })
}

/// 本地编辑后：先合并云端新增（不覆盖刚写的 SSH config），再立刻推送。
pub fn publish_after_edit(vault: &Vault, s3: &S3Client) -> Result<SyncResult> {
    if !vault.is_unlocked() {
        return Err(AppError::Locked);
    }
    if !local_has_syncable_assets(vault)? {
        return Ok(SyncResult {
            synced_at: iso_now(),
            objects_transferred: 0,
            identity_count: 0,
            key_count: 0,
            repo_count: 0,
            message: "本地无身份数据，已跳过推送".into(),
        });
    }
    let remote = fetch_remote_manifest(vault, s3)?;
    if remote
        .as_ref()
        .is_some_and(|m| m.workspace_id != vault.workspace_id())
    {
        return Err(AppError::Invalid(
            "云端工作空间与本地不一致，已跳过自动推送，请手动确认".into(),
        ));
    }
    if remote.is_some() {
        let _ = pull_from_cloud_inner(vault, s3, false, false)?;
    }
    let pushed = push_to_cloud(vault, s3)?;
    Ok(SyncResult {
        message: format!("已保存并推送到云端：{}", pushed.message),
        ..pushed
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::crypto::MasterKey;

    #[test]
    fn test_sync_object_hmac_and_encryption() {
        let mk = MasterKey::random();
        let enc_key = mk.subkey(LABEL_SYNC_OBJECT);

        let name1 = mk.object_name("data/identities.json");
        let name2 = mk.object_name("data/identities.json");
        let name3 = mk.object_name("keys/k1.key");
        assert_eq!(name1, name2, "相同逻辑路径 HMAC 名称应确定");
        assert_ne!(name1, name3, "不同逻辑路径 HMAC 名称应不同");
        assert!(!name1.contains("identities"), "HMAC 名称绝不含明文特征");

        let plain = b"{\"identities\":[{\"id\":\"1\"}]}";
        let enc = encrypt_payload(&enc_key, plain).unwrap();
        assert_ne!(&enc[..], &plain[..]);

        let dec = decrypt_payload(&enc_key, &enc).unwrap();
        assert_eq!(dec, plain);
    }

    fn meta(id: &str, created_at: &str) -> SnapshotMeta {
        SnapshotMeta {
            id: id.into(),
            created_at: created_at.into(),
            client_name: "t".into(),
            identity_count: 1,
            key_count: 0,
            repo_count: 0,
            is_recent: false,
            is_daily_first: false,
        }
    }

    #[test]
    fn prune_keeps_recent_and_daily_first() {
        let now = time::OffsetDateTime::parse(
            "2026-01-15T18:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        // 1 月 1 日到 15 日每天一份，另加 15 日下午一份。
        let mut items: Vec<SnapshotMeta> = (1..=15)
            .map(|i| meta(&format!("d{i:02}"), &format!("2026-01-{i:02}T01:00:00Z")))
            .collect();
        items.push(meta("d15b", "2026-01-15T12:00:00Z"));

        let dropped = prune_snapshot_index_at(&mut items, now, 10, 14);
        // 近 14 天（1/2–1/15）的每日首份 + 最近 10 份，1 月 1 日应被丢掉。
        assert!(dropped.contains(&"d01".into()));
        assert!(!items.iter().any(|m| m.id == "d01"));
        assert!(items.iter().any(|m| m.id == "d02" && m.is_daily_first));
        assert!(items.iter().any(|m| m.id == "d15" && m.is_daily_first));
        assert!(items.iter().any(|m| m.id == "d15b" && m.is_recent));
        assert!(items.len() >= 14);
        assert!(items.len() <= 16);
    }

    #[test]
    fn prune_daily_first_is_earliest_of_that_day() {
        let now = time::OffsetDateTime::parse(
            "2026-09-09T20:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        let mut items: Vec<SnapshotMeta> = (0..12)
            .map(|i| meta(&format!("n{i}"), &format!("2026-09-09T{:02}:00:00Z", 8 + i)))
            .collect();
        items.push(meta("old", "2026-08-01T08:00:00Z"));
        let dropped = prune_snapshot_index_at(&mut items, now, 10, 14);
        assert!(dropped.contains(&"old".into()));
        let first = items.iter().find(|m| m.is_daily_first).unwrap();
        assert_eq!(first.id, "n0");
        assert!(first.created_at.starts_with("2026-09-09T08:"));
        assert!(items.iter().any(|m| m.id == "n11" && m.is_recent));
    }

    #[test]
    fn snapshot_id_sanitizes_rfc3339() {
        assert_eq!(
            snapshot_id_from_time("2026-09-09T12:05:28Z"),
            "2026-09-09T12-05-28Z"
        );
    }

    #[test]
    fn cloud_vault_header_roundtrip_and_bare_fallback() {
        use crate::vault::header::KdfParams;
        use crate::vault::kdf::{ITERS_FLOOR, MEM_FLOOR_KIB};
        let root = std::env::temp_dir().join(format!("gam-hdr-{}", uuid::Uuid::new_v4()));
        let kdf = KdfParams::new(MEM_FLOOR_KIB, ITERS_FLOOR, 1);
        let (v, rec) = crate::vault::Vault::init(&root, "pw", kdf).unwrap();
        let wrapped = CloudVaultHeader {
            version: 1,
            header: v.header().clone(),
        };
        let raw = serde_json::to_vec(&wrapped).unwrap();
        let parsed = parse_cloud_vault_header(&raw).unwrap();
        assert_eq!(parsed.workspace_id, v.workspace_id());
        let mk = unlock_header_with_recovery(&parsed, &rec).unwrap();
        assert_eq!(mk.as_bytes(), v.master_key_bytes().unwrap().as_ref());

        let bare = serde_json::to_vec(v.header()).unwrap();
        let parsed_bare = parse_cloud_vault_header(&bare).unwrap();
        assert_eq!(parsed_bare.workspace_id, v.workspace_id());
        std::fs::remove_dir_all(&root).ok();
    }
}
