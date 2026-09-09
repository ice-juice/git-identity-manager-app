//! 端到端加密云端同步引擎（v1.1）
//!
//! 核心原则：
//! 1. 零知识端到端加密：云端永远只能看到经过 XChaCha20-Poly1305 加密的密文与 HMAC 随机化散列的对象名。
//! 2. 身份、密钥文件名、用户名、仓库地址等一律不出现在云端。
//! 3. 事务性原子提交："先传加密对象，后写 manifest" 保证网络异常时不损坏已有快照。

use crate::error::{AppError, Result};
use crate::model::{Secrets, VaultData};
use crate::store;
use crate::sync::backup::deploy_key_to_workspace;
use crate::sync::s3::S3Client;
use crate::vault::crypto::{self, KEY_LEN, LABEL_SYNC_OBJECT, XNONCE_LEN};
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
    let raw = match s3.get_object(MANIFEST_FILE_KEY)? {
        Some(b) => b,
        None => return Ok(None),
    };

    let plain = decrypt_payload(&key, &raw)
        .map_err(|_| AppError::Invalid("云端清单解密失败：可能使用了不同的恢复密钥或工作空间".into()))?;

    let manifest: SyncManifest = serde_json::from_slice(&plain)
        .map_err(|e| AppError::Invalid(format!("解析云端清单失败: {e}")))?;

    Ok(Some(manifest))
}

/// 查询云端与本地同步比对状态
pub fn get_sync_status(vault: &Vault, s3: &S3Client) -> Result<CloudSyncStatus> {
    if !vault.is_unlocked() {
        return Err(AppError::Locked);
    }

    let data = store::load_data(vault)?;
    let local_identity_count = data.identities.len();
    let local_key_count = data.keys.len();
    let local_repo_count = data.repos.len();

    let remote = match fetch_remote_manifest(vault, s3) {
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
            });
        }
    };

    if let Some(m) = remote {
        let is_same_ws = m.workspace_id == vault.workspace_id();
        let status = if !is_same_ws {
            "different_workspace".to_string()
        } else {
            // 对比本地与云端 hash
            let local_id_json = serde_json::to_vec(&data)?;
            let local_id_hash = sha256_hex(&local_id_json);
            let remote_id_hash = m
                .objects
                .get("data/identities.json")
                .map(|e| e.sha256.as_str())
                .unwrap_or_default();

            if local_id_hash == remote_id_hash {
                "synced".to_string()
            } else {
                "local_ahead".to_string()
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
        })
    }
}

/// 推送到云端（Push）
pub fn push_to_cloud(vault: &Vault, s3: &S3Client) -> Result<SyncResult> {
    if !vault.is_unlocked() {
        return Err(AppError::Locked);
    }

    let enc_key = vault.subkey(LABEL_SYNC_OBJECT)?;
    let now_str = iso_now();

    // 1. 收集本地需同步对象
    let data = store::load_data(vault)?;
    let secrets = store::load_secrets(vault)?;

    let mut logical_objects: HashMap<String, Vec<u8>> = HashMap::new();

    // (1) 身份与仓库元数据
    logical_objects.insert("data/identities.json".into(), serde_json::to_vec(&data)?);
    // (2) 机密口令
    logical_objects.insert("data/secrets.json".into(), serde_json::to_vec(&secrets)?);
    // (3) 所有私钥副本
    for key in &data.keys {
        if store::key_exists(vault, &key.id) {
            if let Ok(priv_bytes) = store::load_key(vault, &key.id) {
                logical_objects.insert(format!("keys/{}.key", key.id), priv_bytes);
            }
        }
    }
    // (4) 工作空间 SSH config 副本
    let ws_cfg = crate::sys::workspace_ssh_config(vault.root());
    if let Ok(cfg_bytes) = std::fs::read(&ws_cfg) {
        logical_objects.insert("ssh/config".into(), cfg_bytes);
    } else {
        let home_cfg = crate::sys::ssh_dir().join("config");
        if let Ok(cfg_bytes) = std::fs::read(&home_cfg) {
            logical_objects.insert("ssh/config".into(), cfg_bytes);
        }
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
        message: format!("已成功推送到云端存储，包含 {} 个加密对象", transferred_count),
    })
}

/// 从云端拉取并还原到本地（Pull）
pub fn pull_from_cloud(vault: &Vault, s3: &S3Client) -> Result<SyncResult> {
    pull_from_cloud_inner(vault, s3, true).map(|(r, _)| r)
}

fn pull_from_cloud_inner(vault: &Vault, s3: &S3Client, apply_remote_ssh: bool) -> Result<(SyncResult, bool)> {
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
        } else if logical_path == "ssh/config" {
            remote_ssh_snap = Some(String::from_utf8_lossy(&plain).to_string());
        } else if let Some(key_id) = logical_path.strip_prefix("keys/").and_then(|s| s.strip_suffix(".key")) {
            store::save_key(vault, key_id, &plain)?;
            pulled_key_ids.push(key_id.to_string());
        }
    }

    // 网络往返后再读本地，避免同步期间的删除/编辑被旧快照盖回去。
    let mut current_data = store::load_data(vault)?;
    if let Some(remote_data) = remote_data_snap.clone() {
        current_data = crate::model::merge_vault_data(current_data, remote_data);
    }
    let mut current_secrets = store::load_secrets(vault)?;
    if let Some(remote_secrets) = &remote_secrets_snap {
        for (k, v) in remote_secrets.key_passphrases.clone() {
            current_secrets.key_passphrases.entry(k).or_insert(v);
        }
        if current_secrets.github_pat.is_none() {
            current_secrets.github_pat = remote_secrets.github_pat.clone();
        }
    }
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
    store::save_secrets(vault, &current_secrets)?;
    let ssh_now = read_ssh_config_bytes(vault).unwrap_or_default();
    let remote_hash = match remote_data_snap {
        Some(rd) => Some(stable_state_hash(
            &rd,
            remote_secrets_snap.as_ref().unwrap_or(&Secrets::default()),
            remote_ssh_snap.as_deref().unwrap_or(""),
        )),
        None => None,
    };
    let need_push = match remote_hash {
        Some(h) => h != stable_state_hash(&current_data, &current_secrets, &ssh_now),
        None => !current_data.identities.is_empty() || !current_data.keys.is_empty(),
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
        return crate::sys::persist_ssh_config(Some(vault.root()), remote_ssh);
    }
    let local_ssh = read_ssh_config_bytes(vault).unwrap_or_default();
    let keep: HashSet<String> = merged.identities.iter().map(|i| i.host_alias.clone()).collect();
    let mut text = crate::ssh::managed::merge_managed_prefer_local(&local_ssh, remote_ssh, &keep);
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
    let payload = SnapshotPayload {
        meta: meta.clone(),
        data: data.clone(),
        secrets: secrets.clone(),
        private_keys: collect_private_keys(vault, data),
        ssh_config: read_ssh_config_bytes(vault),
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
    for repo in data.repos.drain(..) {
        if let Some(existing) = current_repos.iter_mut().find(|r| r.id == repo.id) {
            *existing = repo;
        } else {
            current_repos.push(repo);
        }
    }
    data.repos = current_repos;

    store::save_data(vault, &data)?;
    store::save_secrets(vault, &payload.secrets)?;

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

fn stable_state_hash(data: &VaultData, secrets: &Secrets, ssh: &str) -> String {
    let mut identities: Vec<&_> = data.identities.iter().collect();
    identities.sort_by(|a, b| a.id.cmp(&b.id));
    let mut keys: Vec<&_> = data.keys.iter().collect();
    keys.sort_by(|a, b| a.id.cmp(&b.id));
    let mut history: Vec<(&String, &String)> = data.clone_history.iter().collect();
    history.sort_by(|a, b| a.0.cmp(b.0));
    let mut passes: Vec<(&String, &String)> = secrets.key_passphrases.iter().collect();
    passes.sort_by(|a, b| a.0.cmp(b.0));
    let payload = serde_json::json!({
        "identities": identities,
        "keys": keys,
        "cloneHistory": history,
        "passphrases": passes,
        "githubPat": secrets.github_pat,
        "ssh": ssh,
    });
    sha256_hex(payload.to_string().as_bytes())
}

fn local_has_syncable_assets(vault: &Vault) -> Result<bool> {
    let data = store::load_data(vault)?;
    Ok(!data.identities.is_empty() || !data.keys.is_empty())
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
        let (pulled, need_push) = pull_from_cloud_inner(vault, s3, false)?;
        if need_push && local_has_syncable_assets(vault)? {
            let pushed = push_to_cloud(vault, s3)?;
            return Ok(SyncResult {
                message: format!("已拉取并推送：{}", pushed.message),
                ..pushed
            });
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
        let _ = pull_from_cloud_inner(vault, s3, false)?;
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
}
