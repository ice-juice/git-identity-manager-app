//! 领域模型：身份、密钥记录、机密集合、以及加密落盘的数据容器。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod serde_maps {
    use serde::ser::{SerializeMap, Serializer};
    use serde::Serialize;
    use std::collections::HashMap;

    /// HashMap 的 JSON 键序不稳定，会导致云端清单哈希每次推送后对不上。
    pub fn ordered_string_map<S: Serializer>(
        map: &HashMap<String, String>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        ordered_map(map, serializer)
    }

    pub fn ordered_account_secret_map<S: Serializer>(
        map: &HashMap<String, super::AccountSecret>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        ordered_map(map, serializer)
    }

    fn ordered_map<S: Serializer, V: Serialize>(
        map: &HashMap<String, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut items: Vec<_> = map.iter().collect();
        items.sort_by(|a, b| a.0.cmp(b.0));
        let mut ser = serializer.serialize_map(Some(items.len()))?;
        for (k, v) in items {
            ser.serialize_entry(k, v)?;
        }
        ser.end()
    }
}

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
    /// 工作空间 `ssh-keys/` 下的密钥路径。库内与云端存 `%GAM_WORKSPACE%/ssh-keys/...`，
    /// 本机 OpenSSH 落盘时再展开成当前工作空间绝对路径。
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
    /// 登记该仓库的本机安装实例。空值表示升级前的旧记录。
    #[serde(default)]
    pub machine_id: String,
}

/// 加密落盘的元数据容器（data/identities.enc）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VaultData {
    pub identities: Vec<Identity>,
    pub keys: Vec<KeyRecord>,
    /// 自动学习的克隆历史：owner(小写) → identity_id。
    #[serde(default, serialize_with = "serde_maps::ordered_string_map")]
    pub clone_history: HashMap<String, String>,
    /// 已登记的本地仓库。
    #[serde(default)]
    pub repos: Vec<ManagedRepo>,
    /// 已删除身份：id → 删除时间。多端拉取时用于真正去掉对端已删的身份。
    #[serde(default, serialize_with = "serde_maps::ordered_string_map")]
    pub deleted_identities: HashMap<String, String>,
    /// 已删除仓库：id → 删除时间。避免云端旧快照把本机刚移除的登记合并回来。
    #[serde(default, serialize_with = "serde_maps::ordered_string_map")]
    pub deleted_repos: HashMap<String, String>,
}

/// 机密集合（data/secrets.enc）——只在 Rust 侧内存出现，绝不跨 IPC。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Secrets {
    /// keyId -> 私钥口令。
    #[serde(serialize_with = "serde_maps::ordered_string_map")]
    pub key_passphrases: HashMap<String, String>,
    /// GitHub PAT。
    pub github_pat: Option<String>,
    /// totpId -> Base32 种子。
    #[serde(default, serialize_with = "serde_maps::ordered_string_map")]
    pub totp_seeds: HashMap<String, String>,
    /// accountId -> 密码与历史。
    #[serde(default, serialize_with = "serde_maps::ordered_account_secret_map")]
    pub account_secrets: HashMap<String, AccountSecret>,
}

/// 一个 TOTP 条目（元数据；种子单独存 Secrets）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TotpEntry {
    pub id: String,
    pub issuer: String,
    pub account: String,
    pub note: Option<String>,
    pub url: Option<String>,
    pub group: Option<String>,
    pub algorithm: String,
    pub digits: u8,
    pub period: u32,
    pub icon: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

/// 一个隐私账号（元数据；密码单独存 Secrets）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountEntry {
    pub id: String,
    pub platform: String,
    pub username: String,
    pub display_name: Option<String>,
    pub url: Option<String>,
    pub note: Option<String>,
    pub group: Option<String>,
    pub tags: Vec<String>,
    pub icon: Option<String>,
    pub pinned: bool,
    pub sort_order: i32,
    pub totp_ref: Option<String>,
    pub last_used_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 分组元数据。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GroupMeta {
    pub name: String,
    pub color: Option<String>,
    pub sort_order: i32,
}

/// data/totp.enc
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TotpData {
    pub entries: Vec<TotpEntry>,
    pub groups: Vec<GroupMeta>,
    #[serde(default, serialize_with = "serde_maps::ordered_string_map")]
    pub deleted_entries: HashMap<String, String>,
}

/// data/accounts.enc
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountData {
    pub entries: Vec<AccountEntry>,
    pub groups: Vec<GroupMeta>,
    #[serde(default, serialize_with = "serde_maps::ordered_string_map")]
    pub deleted_entries: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountSecret {
    pub password: String,
    #[serde(default)]
    pub extra_fields: HashMap<String, String>,
    #[serde(default)]
    pub history: Vec<PasswordHistoryItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PasswordHistoryItem {
    pub password: String,
    pub replaced_at: String,
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

/// 把本地尚未打戳、且目录仍在的旧仓库记到当前机器。
/// 换机拉下来的失效路径保持无归属，随后会被 `keep_repos_for_machine` 丢掉且不打墓碑。
pub fn claim_unowned_repos(data: &mut VaultData, machine_id: &str) -> bool {
    let mut changed = false;
    for repo in &mut data.repos {
        if repo.machine_id.trim().is_empty() && std::path::Path::new(&repo.path).is_dir() {
            repo.machine_id = machine_id.to_string();
            changed = true;
        }
    }
    changed
}

/// 本机库只保留当前机器的仓库，外机路径不落本地。
pub fn keep_repos_for_machine(data: &mut VaultData, machine_id: &str) -> bool {
    let before = data.repos.len();
    data.repos.retain(|repo| repo.machine_id == machine_id);
    data.repos.len() != before
}

/// 恢复向导勾选「恢复仓库」时：把云端仓库改盖成本机归属。
pub fn adopt_repos_as_machine(data: &mut VaultData, remote: &VaultData, machine_id: &str) {
    for mut repo in remote.repos.clone() {
        if data.deleted_repos.contains_key(&repo.id) {
            continue;
        }
        repo.machine_id = machine_id.to_string();
        if let Some(existing) = data.repos.iter_mut().find(|item| item.id == repo.id) {
            *existing = repo;
        } else {
            data.repos.push(repo);
        }
    }
}

/// 推送时把本机仓库嵌回云端全集，保留其他机器的登记，避免互相覆盖。
pub fn compose_cloud_repos(local: &VaultData, remote: &VaultData, machine_id: &str) -> (Vec<ManagedRepo>, HashMap<String, String>) {
    let mut deleted = local.deleted_repos.clone();
    for (id, ts) in &remote.deleted_repos {
        match deleted.get(id) {
            Some(old) if old >= ts => {}
            _ => {
                deleted.insert(id.clone(), ts.clone());
            }
        }
    }
    let local_ids: std::collections::HashSet<&str> =
        local.repos.iter().map(|repo| repo.id.as_str()).collect();
    let mut repos: Vec<ManagedRepo> = remote
        .repos
        .iter()
        .filter(|repo| {
            if deleted.contains_key(&repo.id) {
                return false;
            }
            if repo.machine_id == machine_id {
                return false;
            }
            if repo.machine_id.trim().is_empty() && local_ids.contains(repo.id.as_str()) {
                return false;
            }
            true
        })
        .cloned()
        .collect();
    for repo in &local.repos {
        if repo.machine_id == machine_id && !deleted.contains_key(&repo.id) {
            repos.retain(|item| item.id != repo.id);
            repos.push(repo.clone());
        }
    }
    (repos, deleted)
}

fn merge_tombstones(mut a: HashMap<String, String>, b: HashMap<String, String>) -> HashMap<String, String> {
    for (id, ts) in b {
        match a.get(&id) {
            Some(old) if old >= &ts => {}
            _ => {
                a.insert(id, ts);
            }
        }
    }
    a
}

fn merge_groups(mut local: Vec<GroupMeta>, remote: Vec<GroupMeta>) -> Vec<GroupMeta> {
    for g in remote {
        if !local.iter().any(|x| x.name == g.name) {
            local.push(g);
        }
    }
    local
}

/// TOTP 条目按 updated_at 取新，墓碑传播删除；分组按名称去重后并集。
pub fn merge_totp_data(local: TotpData, remote: TotpData) -> TotpData {
    let deleted = merge_tombstones(local.deleted_entries, remote.deleted_entries);
    let mut entries: HashMap<String, TotpEntry> = HashMap::new();
    for item in local.entries {
        entries.insert(item.id.clone(), item);
    }
    for item in remote.entries {
        match entries.get(&item.id) {
            Some(local_item) if timestamp_newer_or_eq(&local_item.updated_at, &item.updated_at) => {}
            _ => {
                entries.insert(item.id.clone(), item);
            }
        }
    }
    let entries: Vec<TotpEntry> = entries
        .into_values()
        .filter(|item| match deleted.get(&item.id) {
            Some(ts) if timestamp_newer_or_eq(ts, &item.updated_at) => false,
            _ => true,
        })
        .collect();
    TotpData {
        entries,
        groups: merge_groups(local.groups, remote.groups),
        deleted_entries: deleted,
    }
}

/// 账号条目按 updated_at 取新，墓碑传播删除。
pub fn merge_account_data(local: AccountData, remote: AccountData) -> AccountData {
    let deleted = merge_tombstones(local.deleted_entries, remote.deleted_entries);
    let mut entries: HashMap<String, AccountEntry> = HashMap::new();
    for item in local.entries {
        entries.insert(item.id.clone(), item);
    }
    for item in remote.entries {
        match entries.get(&item.id) {
            Some(local_item) if timestamp_newer_or_eq(&local_item.updated_at, &item.updated_at) => {}
            _ => {
                entries.insert(item.id.clone(), item);
            }
        }
    }
    let entries: Vec<AccountEntry> = entries
        .into_values()
        .filter(|item| match deleted.get(&item.id) {
            Some(ts) if timestamp_newer_or_eq(ts, &item.updated_at) => false,
            _ => true,
        })
        .collect();
    AccountData {
        entries,
        groups: merge_groups(local.groups, remote.groups),
        deleted_entries: deleted,
    }
}

/// 机密并集：密钥口令/PAT 仍是本地优先补缺；TOTP/账号机密跟条目 `updated_at` 对齐。
pub fn merge_secrets(local: Secrets, remote: &Secrets) -> Secrets {
    merge_secrets_with_meta(local, remote, None, None, None, None)
}

pub fn merge_secrets_with_meta(
    mut local: Secrets,
    remote: &Secrets,
    local_totp: Option<&TotpData>,
    remote_totp: Option<&TotpData>,
    local_accounts: Option<&AccountData>,
    remote_accounts: Option<&AccountData>,
) -> Secrets {
    for (k, v) in remote.key_passphrases.clone() {
        local.key_passphrases.entry(k).or_insert(v);
    }
    if local.github_pat.is_none() {
        local.github_pat = remote.github_pat.clone();
    }
    for (k, v) in remote.totp_seeds.clone() {
        if take_remote_secret(
            totp_updated_at(local_totp, &k),
            totp_updated_at(remote_totp, &k),
            local.totp_seeds.contains_key(&k),
        ) {
            local.totp_seeds.insert(k, v);
        }
    }
    for (k, v) in remote.account_secrets.clone() {
        if take_remote_secret(
            account_updated_at(local_accounts, &k),
            account_updated_at(remote_accounts, &k),
            local.account_secrets.contains_key(&k),
        ) {
            local.account_secrets.insert(k, v);
        }
    }
    local
}

fn totp_updated_at<'a>(data: Option<&'a TotpData>, id: &str) -> Option<&'a str> {
    data.and_then(|d| d.entries.iter().find(|e| e.id == id).map(|e| e.updated_at.as_str()))
}

fn account_updated_at<'a>(data: Option<&'a AccountData>, id: &str) -> Option<&'a str> {
    data.and_then(|d| d.entries.iter().find(|e| e.id == id).map(|e| e.updated_at.as_str()))
}

/// 远程条目更新，或本地还没有这份机密时，采用远程。
fn take_remote_secret(local_ts: Option<&str>, remote_ts: Option<&str>, local_has: bool) -> bool {
    if !local_has {
        return true;
    }
    match (remote_ts, local_ts) {
        (Some(rt), Some(lt)) => !timestamp_newer_or_eq(lt, rt),
        (Some(_), None) => true,
        _ => false,
    }
}

pub fn timestamp_newer_or_eq(a: &str, b: &str) -> bool {
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
            machine_id: String::new(),
        }
    }

    fn repo_on(id: &str, path: &str, machine: &str) -> ManagedRepo {
        let mut item = repo(id, path);
        item.machine_id = machine.into();
        item
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

    #[test]
    fn claim_and_keep_and_compose_repos_by_machine() {
        let mut local = VaultData {
            repos: vec![
                repo_on("old", "D:/local", "pc-a"),
                repo_on("mine", "D:/mine", "pc-a"),
                repo_on("theirs", "E:/theirs", "pc-b"),
            ],
            ..VaultData::default()
        };
        assert!(!claim_unowned_repos(&mut local, "pc-a"));
        assert!(keep_repos_for_machine(&mut local, "pc-a"));
        assert_eq!(local.repos.len(), 2);
        assert!(local.repos.iter().all(|r| r.machine_id == "pc-a"));

        let existing = std::env::temp_dir();
        let mut orphan = VaultData {
            repos: vec![
                repo("ghost", "Z:/definitely-missing-gam-repo"),
                {
                    let mut item = repo("here", existing.to_string_lossy().as_ref());
                    item.machine_id.clear();
                    item
                },
            ],
            ..VaultData::default()
        };
        assert!(claim_unowned_repos(&mut orphan, "pc-a"));
        assert!(orphan.repos.iter().any(|r| r.id == "ghost" && r.machine_id.is_empty()));
        assert_eq!(
            orphan.repos.iter().find(|r| r.id == "here").unwrap().machine_id,
            "pc-a"
        );
        keep_repos_for_machine(&mut orphan, "pc-a");
        assert!(orphan.repos.iter().all(|r| r.id == "here"));

        let remote = VaultData {
            repos: vec![
                repo_on("mine", "D:/stale", "pc-a"),
                repo_on("theirs", "E:/theirs", "pc-b"),
                repo("legacy", "C:/legacy"),
            ],
            ..VaultData::default()
        };
        let (cloud, _) = compose_cloud_repos(&local, &remote, "pc-a");
        let ids: Vec<_> = cloud.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"old"));
        assert!(ids.contains(&"mine"));
        assert!(ids.contains(&"theirs"));
        assert!(ids.contains(&"legacy"));
        assert_eq!(
            cloud.iter().find(|r| r.id == "mine").unwrap().path,
            "D:/mine"
        );
    }

    #[test]
    fn adopt_remote_repos_stamps_current_machine() {
        let mut local = VaultData::default();
        let remote = VaultData {
            repos: vec![repo_on("r1", "D:/old-machine", "pc-old")],
            ..VaultData::default()
        };
        adopt_repos_as_machine(&mut local, &remote, "pc-new");
        keep_repos_for_machine(&mut local, "pc-new");
        assert_eq!(local.repos.len(), 1);
        assert_eq!(local.repos[0].machine_id, "pc-new");
    }

    #[test]
    fn vault_data_json_is_deterministic_with_maps() {
        let mut data = VaultData::default();
        data.clone_history.insert("zeta".into(), "id-z".into());
        data.clone_history.insert("alpha".into(), "id-a".into());
        data.deleted_identities.insert("b".into(), "t2".into());
        data.deleted_identities.insert("a".into(), "t1".into());
        let a = serde_json::to_vec(&data).unwrap();
        let b = serde_json::to_vec(&data).unwrap();
        assert_eq!(a, b);
        let text = String::from_utf8(a).unwrap();
        assert!(
            text.find("\"alpha\"").unwrap() < text.find("\"zeta\"").unwrap(),
            "clone_history 应按键名排序序列化"
        );
    }

    fn totp(id: &str, issuer: &str, updated_at: &str) -> TotpEntry {
        TotpEntry {
            id: id.into(),
            issuer: issuer.into(),
            account: "a".into(),
            note: None,
            url: None,
            group: None,
            algorithm: "SHA1".into(),
            digits: 6,
            period: 30,
            icon: None,
            sort_order: 0,
            created_at: updated_at.into(),
            updated_at: updated_at.into(),
        }
    }

    #[test]
    fn merge_totp_prefers_newer_and_tombstone() {
        let local = TotpData {
            entries: vec![totp("a", "local-new", "2026-09-09T12:00:00Z")],
            deleted_entries: HashMap::from([("c".into(), "2026-09-08T00:00:00Z".into())]),
            ..TotpData::default()
        };
        let remote = TotpData {
            entries: vec![
                totp("a", "remote-old", "2026-09-08T00:00:00Z"),
                totp("c", "should-drop", "2026-09-07T00:00:00Z"),
                totp("d", "remote-only", "2026-09-09T11:00:00Z"),
            ],
            ..TotpData::default()
        };
        let merged = merge_totp_data(local, remote);
        let names: HashMap<_, _> = merged.entries.iter().map(|e| (e.id.as_str(), e.issuer.as_str())).collect();
        assert_eq!(names.get("a"), Some(&"local-new"));
        assert_eq!(names.get("d"), Some(&"remote-only"));
        assert!(!names.contains_key("c"));
    }

    #[test]
    fn merge_totp_secret_follows_newer_entry() {
        let local_data = TotpData {
            entries: vec![totp("a", "local-old", "2026-09-08T00:00:00Z")],
            ..TotpData::default()
        };
        let remote_data = TotpData {
            entries: vec![totp("a", "remote-new", "2026-09-09T12:00:00Z")],
            ..TotpData::default()
        };
        let mut local = Secrets::default();
        local.totp_seeds.insert("a".into(), "LOCALSEED".into());
        let mut remote = Secrets::default();
        remote.totp_seeds.insert("a".into(), "REMOTESEED".into());
        remote.totp_seeds.insert("b".into(), "ONLYREMOTE".into());
        let merged = merge_secrets_with_meta(
            local,
            &remote,
            Some(&local_data),
            Some(&remote_data),
            None,
            None,
        );
        assert_eq!(merged.totp_seeds.get("a").map(String::as_str), Some("REMOTESEED"));
        assert_eq!(merged.totp_seeds.get("b").map(String::as_str), Some("ONLYREMOTE"));
    }
}
