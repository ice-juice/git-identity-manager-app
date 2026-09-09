//! 领域模型：身份、密钥记录、机密集合、以及加密落盘的数据容器。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 一把密钥的元数据（公开信息，可展示；私钥密文单独存 keys/<id>.enc）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeyRecord {
    pub id: String,
    pub name: String,
    pub algorithm: String,
    pub fingerprint: String,
    pub public_openssh: String,
    pub bits: Option<u32>,
    pub has_passphrase: bool,
    /// 是否弱密钥（RSA < 2048）。
    pub weak: bool,
    pub source_path: Option<String>,
    /// 工作空间 `ssh-keys/` 下可供 OpenSSH 使用的真实路径（默认是私钥，严格模式是 .pub）。
    #[serde(default)]
    pub deployed_path: Option<String>,
    pub imported_at: String,
}

/// 一个 Git 身份。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub host_alias: String,
    pub real_host: String,
    pub user: String,
    pub email: Option<String>,
    pub git_user_name: Option<String>,
    pub key_id: Option<String>,
    /// 归属标识（owner 前缀，支持通配 `vortaq-*`）。M5 使用。
    pub owners: Vec<String>,
    /// 严格模式：私钥仅存 agent，磁盘只留 .pub。
    pub strict_mode: bool,
    /// 最近一次编辑时间（RFC3339），多端合并时较新的覆盖较旧的。
    #[serde(default)]
    pub updated_at: String,
}

/// 纳入程序统一管理的本地 Git 仓库。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRepo {
    pub id: String,
    pub path: String,
    pub name: String,
    pub remote_url: Option<String>,
    pub identity_id: Option<String>,
    pub added_at: String,
    /// scan | clone | init | addRemote | manual
    pub source: String,
}

/// 加密落盘的元数据容器（data/identities.enc）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VaultData {
    pub identities: Vec<Identity>,
    pub keys: Vec<KeyRecord>,
    /// 自动学习的克隆历史：owner(小写) → identity_id。
    #[serde(default)]
    pub clone_history: HashMap<String, String>,
    /// 已登记的本地仓库。
    #[serde(default)]
    pub repos: Vec<ManagedRepo>,
    /// 已删除身份：id → 删除时间。多端拉取时用于真正去掉对端已删的身份。
    #[serde(default)]
    pub deleted_identities: HashMap<String, String>,
    /// 已删除仓库：id → 删除时间。避免云端旧快照把本机刚移除的登记合并回来。
    #[serde(default)]
    pub deleted_repos: HashMap<String, String>,
}

/// 机密集合（data/secrets.enc）——只在 Rust 侧内存出现，绝不跨 IPC。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Secrets {
    /// keyId -> 私钥口令。
    pub key_passphrases: HashMap<String, String>,
    /// GitHub PAT。
    pub github_pat: Option<String>,
}

/// 多端合并：身份按 `updated_at` 取较新；墓碑用于传播删除；仓库并集，但尊重删除墓碑。
pub fn merge_vault_data(local: VaultData, remote: VaultData) -> VaultData {
    let mut deleted = local.deleted_identities;
    for (id, ts) in remote.deleted_identities {
        match deleted.get(&id) {
            Some(old) if old >= &ts => {}
            _ => {
                deleted.insert(id, ts);
            }
        }
    }

    let mut deleted_repos = local.deleted_repos;
    for (id, ts) in remote.deleted_repos {
        match deleted_repos.get(&id) {
            Some(old) if old >= &ts => {}
            _ => {
                deleted_repos.insert(id, ts);
            }
        }
    }

    let mut identities: HashMap<String, Identity> = HashMap::new();
    for item in local.identities {
        identities.insert(item.id.clone(), item);
    }
    for item in remote.identities {
        match identities.get(&item.id) {
            Some(local_item) if timestamp_newer_or_eq(&local_item.updated_at, &item.updated_at) => {}
            _ => {
                identities.insert(item.id.clone(), item);
            }
        }
    }
    let identities: Vec<Identity> = identities
        .into_values()
        .filter(|item| match deleted.get(&item.id) {
            Some(ts) if timestamp_newer_or_eq(ts, &item.updated_at) => false,
            _ => true,
        })
        .collect();

    let mut keys: HashMap<String, KeyRecord> = HashMap::new();
    for item in local.keys {
        keys.insert(item.id.clone(), item);
    }
    for item in remote.keys {
        match keys.get(&item.id) {
            Some(local_item) if local_item.imported_at >= item.imported_at => {}
            _ => {
                keys.insert(item.id.clone(), item);
            }
        }
    }

    let mut repos = local.repos;
    repos.retain(|r| !deleted_repos.contains_key(&r.id));
    for repo in remote.repos {
        if deleted_repos.contains_key(&repo.id) {
            continue;
        }
        if !repos.iter().any(|r| r.id == repo.id) {
            repos.push(repo);
        }
    }

    let mut clone_history = local.clone_history;
    for (k, v) in remote.clone_history {
        clone_history.entry(k).or_insert(v);
    }

    VaultData {
        identities,
        keys: keys.into_values().collect(),
        clone_history,
        repos,
        deleted_identities: deleted,
        deleted_repos,
    }
}

fn timestamp_newer_or_eq(a: &str, b: &str) -> bool {
    match (parse_rfc3339(a), parse_rfc3339(b)) {
        (Some(ta), Some(tb)) => ta >= tb,
        _ => a >= b,
    }
}

fn parse_rfc3339(raw: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(id: &str, name: &str, updated_at: &str) -> Identity {
        Identity {
            id: id.into(),
            name: name.into(),
            platform: "github".into(),
            host_alias: format!("gh-{name}"),
            real_host: "github.com".into(),
            user: "git".into(),
            email: None,
            git_user_name: None,
            key_id: None,
            owners: vec![],
            strict_mode: false,
            updated_at: updated_at.into(),
        }
    }

    #[test]
    fn merge_prefers_newer_identity_and_honors_tombstone() {
        let local = VaultData {
            identities: vec![
                ident("a", "local-new", "2026-09-09T12:00:00Z"),
                ident("b", "local-old", "2026-09-01T00:00:00Z"),
            ],
            deleted_identities: HashMap::from([("c".into(), "2026-09-08T00:00:00Z".into())]),
            ..VaultData::default()
        };
        let remote = VaultData {
            identities: vec![
                ident("a", "remote-old", "2026-09-08T00:00:00Z"),
                ident("b", "remote-new", "2026-09-09T10:00:00Z"),
                ident("c", "should-drop", "2026-09-07T00:00:00Z"),
                ident("d", "remote-only", "2026-09-09T11:00:00Z"),
            ],
            ..VaultData::default()
        };
        let merged = merge_vault_data(local, remote);
        let names: HashMap<_, _> = merged
            .identities
            .iter()
            .map(|i| (i.id.as_str(), i.name.as_str()))
            .collect();
        assert_eq!(names.get("a"), Some(&"local-new"));
        assert_eq!(names.get("b"), Some(&"remote-new"));
        assert_eq!(names.get("d"), Some(&"remote-only"));
        assert!(!names.contains_key("c"));
    }

    fn repo(id: &str, path: &str) -> ManagedRepo {
        ManagedRepo {
            id: id.into(),
            path: path.into(),
            name: id.into(),
            remote_url: None,
            identity_id: None,
            added_at: "2026-09-01T00:00:00Z".into(),
            source: "scan".into(),
        }
    }

    #[test]
    fn merge_honors_repo_tombstone_and_keeps_new_remote() {
        let local = VaultData {
            repos: vec![repo("keep", "D:/keep")],
            deleted_repos: HashMap::from([("gone".into(), "2026-09-09T12:00:00Z".into())]),
            ..VaultData::default()
        };
        let remote = VaultData {
            repos: vec![
                repo("gone", "D:/gone"),
                repo("other", "D:/other"),
            ],
            ..VaultData::default()
        };
        let merged = merge_vault_data(local, remote);
        let ids: Vec<_> = merged.repos.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"keep"));
        assert!(ids.contains(&"other"));
        assert!(!ids.contains(&"gone"));
        assert!(merged.deleted_repos.contains_key("gone"));
    }
}
