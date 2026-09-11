//! `~/.ssh/config` 托管区块的安全增删改。
//!
//! 只在 `# ===== BEGIN/END managed by git-keymaster =====` 之间改写，
//! 区块外的用户手写内容（含注释、空行）原样保留。读取时同时认旧标记。

use crate::identity;
use crate::ssh::config;
use serde::{Deserialize, Serialize};

pub const BEGIN_MARKER: &str = identity::SSH_BEGIN;
pub const END_MARKER: &str = identity::SSH_END;

/// 一个托管 Host 条目。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedEntry {
    pub alias: String,
    pub host_name: String,
    pub user: String,
    pub identity_file: String,
    pub identities_only: bool,
}

impl ManagedEntry {
    pub fn new(alias: &str, host_name: &str, identity_file: &str) -> Self {
        ManagedEntry {
            alias: alias.to_string(),
            host_name: host_name.to_string(),
            user: "git".to_string(),
            identity_file: identity_file.to_string(),
            identities_only: true,
        }
    }
}

fn render_entry(e: &ManagedEntry) -> String {
    let mut s = String::new();
    s.push_str(&format!("Host {}\n", e.alias));
    s.push_str(&format!("    HostName {}\n", e.host_name));
    s.push_str(&format!("    User {}\n", e.user));
    s.push_str(&format!("    IdentityFile {}\n", e.identity_file));
    if e.identities_only {
        s.push_str("    IdentitiesOnly yes\n");
    }
    s
}

fn render_region(entries: &[ManagedEntry]) -> String {
    let mut s = String::new();
    s.push_str(BEGIN_MARKER);
    s.push('\n');
    for (i, e) in entries.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        s.push_str(&render_entry(e));
    }
    s.push_str(END_MARKER);
    s.push('\n');
    s
}

/// 把托管区块内文本解析成条目列表。
fn parse_region(region_inner: &str) -> Vec<ManagedEntry> {
    config::parse(region_inner)
        .blocks
        .into_iter()
        .filter_map(|b| {
            let alias = b.patterns.first()?.clone();
            Some(ManagedEntry {
                alias,
                host_name: b.host_name().unwrap_or("").to_string(),
                user: b.user().unwrap_or("git").to_string(),
                identity_file: b.identity_file().unwrap_or("").to_string(),
                identities_only: b.identities_only(),
            })
        })
        .collect()
}

/// 拆分现有 config 为 (区块前, 托管条目, 区块后)。无托管区块时条目为空。
fn marker_span(lines: &[&str]) -> Option<(usize, usize)> {
    for (begin_m, end_m) in [
        (BEGIN_MARKER, END_MARKER),
        (identity::LEGACY_SSH_BEGIN, identity::LEGACY_SSH_END),
    ] {
        let begin = lines.iter().position(|l| l.trim() == begin_m);
        let end = lines.iter().position(|l| l.trim() == end_m);
        if let (Some(b), Some(e)) = (begin, end) {
            if b < e {
                return Some((b, e));
            }
        }
    }
    None
}

fn split(existing: &str) -> (String, Vec<ManagedEntry>, String) {
    let lines: Vec<&str> = existing.lines().collect();
    match marker_span(&lines) {
        Some((b, e)) => {
            let before = lines[..b].join("\n");
            let inner = lines[b + 1..e].join("\n");
            let after = if e + 1 < lines.len() {
                lines[e + 1..].join("\n")
            } else {
                String::new()
            };
            (before, parse_region(&inner), after)
        }
        None => (existing.to_string(), Vec::new(), String::new()),
    }
}

fn reassemble(before: &str, entries: &[ManagedEntry], after: &str) -> String {
    let region = render_region(entries);
    let mut out = String::new();
    let before_trimmed = before.trim_end_matches('\n');
    if !before_trimmed.is_empty() {
        out.push_str(before_trimmed);
        out.push_str("\n\n");
    }
    out.push_str(&region);
    let after_trimmed = after.trim_start_matches('\n');
    if !after_trimmed.is_empty() {
        out.push('\n');
        out.push_str(after_trimmed);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

fn alias_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn dedupe_entries(entries: Vec<ManagedEntry>) -> Vec<ManagedEntry> {
    let mut out: Vec<ManagedEntry> = Vec::new();
    for entry in entries {
        if let Some(slot) = out.iter_mut().find(|e| alias_eq(&e.alias, &entry.alias)) {
            *slot = entry;
        } else {
            out.push(entry);
        }
    }
    out
}

/// 从一段无标记文本里摘走指定 alias 的 Host 块，返回 (剩余文本, 摘出的托管条目)。
fn take_hosts_by_alias(section: &str, aliases: &std::collections::HashSet<String>) -> (String, Vec<ManagedEntry>) {
    if section.trim().is_empty() || aliases.is_empty() {
        return (section.to_string(), Vec::new());
    }
    let parsed = config::parse(section);
    let mut harvested = Vec::new();
    let mut drop_starts = std::collections::HashSet::new();
    for b in &parsed.blocks {
        let Some(alias) = b.patterns.first() else {
            continue;
        };
        if aliases.iter().any(|a| alias_eq(a, alias)) {
            drop_starts.insert(b.start_line);
            harvested.push(ManagedEntry {
                alias: alias.clone(),
                host_name: b.host_name().unwrap_or("").to_string(),
                user: b.user().unwrap_or("git").to_string(),
                identity_file: b.identity_file().unwrap_or("").to_string(),
                identities_only: b.identities_only(),
            });
        }
    }
    if harvested.is_empty() {
        return (section.to_string(), harvested);
    }
    (strip_host_blocks(section, &drop_starts), harvested)
}

fn strip_host_blocks(section: &str, drop_starts: &std::collections::HashSet<usize>) -> String {
    let mut kept = String::new();
    let mut skipping = false;
    for (idx, raw) in section.lines().enumerate() {
        let trimmed = raw.trim();
        let is_host = !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && trimmed.split_whitespace().next().is_some_and(|k| k.eq_ignore_ascii_case("Host"));
        if is_host {
            skipping = drop_starts.contains(&idx);
        }
        if skipping {
            continue;
        }
        kept.push_str(raw);
        kept.push('\n');
    }
    kept.trim_end_matches('\n').to_string()
}

/// 把全文里同 alias 的 Host 折进唯一托管区，消除云端恢复/补齐造成的重复。
/// 已是「唯一托管区、无重复」时原样返回，避免每次落盘改空白。
pub fn normalize_unique_hosts(existing: &str) -> String {
    let (before, mut entries, after) = split(existing);
    let before_len = entries.len();
    entries = dedupe_entries(entries);
    let mut dirty = entries.len() != before_len;
    let mut aliases: std::collections::HashSet<String> =
        entries.iter().map(|e| e.alias.to_ascii_lowercase()).collect();
    if aliases.is_empty() {
        // 无标记的云端快照：把带 IdentityFile 的 Host 收进托管区，避免随后 upsert 再写一套。
        for b in config::parse(existing).blocks {
            if b.identity_file().is_some() {
                if let Some(alias) = b.patterns.first() {
                    aliases.insert(alias.to_ascii_lowercase());
                }
            }
        }
    }
    let (before, from_before) = take_hosts_by_alias(&before, &aliases);
    let (after, from_after) = take_hosts_by_alias(&after, &aliases);
    if !from_before.is_empty() || !from_after.is_empty() {
        dirty = true;
    }
    for extra in from_before.into_iter().chain(from_after) {
        if let Some(slot) = entries.iter_mut().find(|e| alias_eq(&e.alias, &extra.alias)) {
            if slot.identity_file.trim().is_empty() && !extra.identity_file.trim().is_empty() {
                *slot = extra;
            }
        } else {
            entries.push(extra);
        }
    }
    entries = dedupe_entries(entries);
    if entries.is_empty() {
        return existing.to_string();
    }
    if !dirty {
        return existing.to_string();
    }
    reassemble(&before, &entries, &after)
}

/// 插入或更新一个托管条目（按 alias 去重），返回新 config 文本。
pub fn upsert(existing: &str, entry: ManagedEntry) -> String {
    let existing = normalize_unique_hosts(existing);
    let (before, mut entries, after) = split(&existing);
    if let Some(slot) = entries.iter_mut().find(|e| alias_eq(&e.alias, &entry.alias)) {
        *slot = entry;
    } else {
        entries.push(entry);
    }
    reassemble(&before, &dedupe_entries(entries), &after)
}

/// 移除一个托管条目，返回新 config 文本。
pub fn remove(existing: &str, alias: &str) -> String {
    let existing = normalize_unique_hosts(existing);
    let (before, mut entries, after) = split(&existing);
    entries.retain(|e| !alias_eq(&e.alias, alias));
    reassemble(&before, &entries, &after)
}

/// 列出当前托管条目。
pub fn list(existing: &str) -> Vec<ManagedEntry> {
    split(existing).1
}

/// 用快照里的托管区块覆盖本机 config 的托管区，保留用户手写内容。
pub fn apply_managed_from_snapshot(home: &str, snapshot: &str) -> String {
    let snapshot = normalize_unique_hosts(snapshot);
    let entries = list(&snapshot);
    let (before, _, after) = split(home);
    normalize_unique_hosts(&reassemble(&before, &entries, &after))
}

/// 多端合并托管 Host：同 alias 以本地为准，对端新增且仍有效的 alias 并入。
///
/// 必须在可移植路径空间里合并。本机重写后的绝对路径若参与合并，
/// 会被当成「本地正文」再上传，覆盖对端机器的路径。
pub fn merge_managed_prefer_local(
    local: &str,
    remote: &str,
    keep_remote_aliases: &std::collections::HashSet<String>,
) -> String {
    let local = normalize_unique_hosts(local);
    let remote = normalize_unique_hosts(remote);
    let (before, mut entries, after) = split(&local);
    let mut local_aliases: std::collections::HashSet<String> =
        entries.iter().map(|e| e.alias.to_ascii_lowercase()).collect();
    for entry in list(&remote) {
        if local_aliases.iter().any(|a| alias_eq(a, &entry.alias)) {
            continue;
        }
        if keep_remote_aliases.iter().any(|a| alias_eq(a, &entry.alias)) {
            local_aliases.insert(entry.alias.to_ascii_lowercase());
            entries.push(entry);
        }
    }
    reassemble(&before, &dedupe_entries(entries), &after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_into_empty_appends_region() {
        let out = upsert("", ManagedEntry::new("github-techn", "github.com", "C:\\k\\techn"));
        assert!(out.contains(BEGIN_MARKER));
        assert!(out.contains(END_MARKER));
        let cfg = config::parse(&out);
        let b = cfg.blocks.iter().find(|b| b.patterns[0] == "github-techn").unwrap();
        assert_eq!(b.host_name(), Some("github.com"));
        assert!(b.identities_only());
    }

    #[test]
    fn preserves_user_content_outside_markers() {
        let existing = "# 我的手写配置\nHost myserver\n    HostName 10.0.0.1\n    User root\n";
        let out = upsert(existing, ManagedEntry::new("github-juice", "github.com", "/k/juice"));
        // 用户块仍在。
        assert!(out.contains("Host myserver"));
        assert!(out.contains("HostName 10.0.0.1"));
        assert!(out.contains("# 我的手写配置"));
        // 托管块也在。
        assert!(out.contains("Host github-juice"));
    }

    #[test]
    fn upsert_same_alias_updates_not_duplicates() {
        let mut out = upsert("", ManagedEntry::new("gh", "github.com", "/k/old"));
        out = upsert(&out, ManagedEntry::new("gh", "github.com", "/k/new"));
        let cfg = config::parse(&out);
        let count = cfg.blocks.iter().filter(|b| b.patterns[0] == "gh").count();
        assert_eq!(count, 1, "同 alias 不应重复");
        assert_eq!(cfg.blocks[0].identity_file(), Some("/k/new"));
    }

    #[test]
    fn remove_deletes_only_target_and_keeps_rest() {
        let mut out = upsert("", ManagedEntry::new("a", "github.com", "/k/a"));
        out = upsert(&out, ManagedEntry::new("b", "github.com", "/k/b"));
        out = remove(&out, "a");
        let aliases: Vec<String> = list(&out).into_iter().map(|e| e.alias).collect();
        assert_eq!(aliases, vec!["b"]);
    }

    #[test]
    fn remove_keeps_user_content() {
        let existing = "Host keep\n    HostName x\n";
        let mut out = upsert(existing, ManagedEntry::new("gh", "github.com", "/k"));
        out = remove(&out, "gh");
        assert!(out.contains("Host keep"));
        assert!(list(&out).is_empty());
    }

    #[test]
    fn strict_mode_identity_file_points_to_pub() {
        // 严格模式下 identity_file 指向 .pub。
        let e = ManagedEntry::new("gh", "github.com", "/k/id.pub");
        let out = upsert("", e);
        assert!(out.contains("IdentityFile /k/id.pub"));
    }

    #[test]
    fn merge_in_portable_space_does_not_keep_machine_paths() {
        let local = upsert(
            "",
            ManagedEntry::new(
                "gh-a",
                "github.com",
                "D:/dataSpace/gitIdentityData/ssh-keys/id_ed25519_a",
            ),
        );
        let remote = upsert(
            "",
            ManagedEntry::new(
                "gh-a",
                "github.com",
                "D:/gitIdentifyData/ssh-keys/id_ed25519_a",
            ),
        );
        let local_p = crate::sys::canonical_ssh_for_sync(&local);
        let remote_p = crate::sys::canonical_ssh_for_sync(&remote);
        let keep = std::collections::HashSet::from(["gh-a".into()]);
        let out = merge_managed_prefer_local(&local_p, &remote_p, &keep);
        assert_eq!(
            list(&out)[0].identity_file,
            "%GAM_WORKSPACE%/ssh-keys/id_ed25519_a"
        );
        assert!(!out.contains("dataSpace"));
        assert!(!out.contains("gitIdentifyData/ssh-keys"));
    }

    #[test]
    fn merge_managed_keeps_local_and_adds_remote_only() {
        let local = upsert("", ManagedEntry::new("gh-a", "github.com", "/k/a"));
        let remote = {
            let mut t = upsert("", ManagedEntry::new("gh-a", "github.com", "/k/a-old"));
            t = upsert(&t, ManagedEntry::new("gh-b", "github.com", "/k/b"));
            upsert(&t, ManagedEntry::new("gh-gone", "github.com", "/k/x"))
        };
        let keep = std::collections::HashSet::from(["gh-b".into()]);
        let out = merge_managed_prefer_local(&local, &remote, &keep);
        let aliases: Vec<String> = list(&out).into_iter().map(|e| e.alias).collect();
        assert_eq!(aliases, vec!["gh-a".to_string(), "gh-b".to_string()]);
        assert_eq!(
            list(&out).iter().find(|e| e.alias == "gh-a").unwrap().identity_file,
            "/k/a"
        );
    }

    #[test]
    fn normalize_dedupes_unmarked_then_managed_copy() {
        let raw = "\
Host github-mgccp-afkf
    HostName github.com
    User git
    IdentityFile /k/a
    IdentitiesOnly yes

# ===== BEGIN managed by git-keymaster =====
Host github-mgccp-afkf
    HostName github.com
    User git
    IdentityFile /k/a
    IdentitiesOnly yes
Host github-ice-juice
    HostName github.com
    User git
    IdentityFile /k/b
    IdentitiesOnly yes
# ===== END managed by git-keymaster =====
";
        let out = normalize_unique_hosts(raw);
        let cfg = config::parse(&out);
        assert_eq!(
            cfg.blocks.iter().filter(|b| b.patterns[0] == "github-mgccp-afkf").count(),
            1
        );
        assert_eq!(
            cfg.blocks.iter().filter(|b| b.patterns[0] == "github-ice-juice").count(),
            1
        );
        assert!(out.contains(BEGIN_MARKER));
    }

    #[test]
    fn normalize_wraps_unmarked_snapshot_without_duplicating() {
        let raw = "\
Host github-a
    HostName github.com
    User git
    IdentityFile /k/a
    IdentitiesOnly yes
Host github-b
    HostName github.com
    User git
    IdentityFile /k/b
    IdentitiesOnly yes
";
        let once = normalize_unique_hosts(raw);
        let twice = upsert(&once, ManagedEntry::new("github-a", "github.com", "/k/a"));
        let cfg = config::parse(&twice);
        assert_eq!(cfg.blocks.len(), 2);
        assert_eq!(
            cfg.blocks.iter().filter(|b| b.patterns[0] == "github-a").count(),
            1
        );
        assert_eq!(normalize_unique_hosts(&once), once);
    }

    #[test]
    fn normalize_dedupes_duplicate_aliases_inside_managed_region() {
        let raw = "\
# ===== BEGIN managed by git-keymaster =====
Host github-a
    HostName github.com
    User git
    IdentityFile /k/a
    IdentitiesOnly yes
Host github-a
    HostName github.com
    User git
    IdentityFile /k/a2
    IdentitiesOnly yes
# ===== END managed by git-keymaster =====
";
        let out = normalize_unique_hosts(raw);
        let cfg = config::parse(&out);
        assert_eq!(cfg.blocks.len(), 1);
        assert_eq!(cfg.blocks[0].identity_file(), Some("/k/a2"));
    }

    #[test]
    fn apply_managed_from_snapshot_keeps_user_hosts() {
        let home = "Host keep\n    HostName x\n\n# ===== BEGIN managed by git-account-manager =====\nHost old\n    HostName github.com\n    User git\n    IdentityFile /old\n# ===== END managed by git-account-manager =====\n";
        let snap = upsert("", ManagedEntry::new("new", "github.com", "/k/new"));
        let out = apply_managed_from_snapshot(home, &snap);
        assert!(out.contains("Host keep"));
        assert!(out.contains("Host new"));
        assert!(!out.contains("Host old"));
        assert!(out.contains(BEGIN_MARKER));
        assert!(!out.contains(crate::identity::LEGACY_SSH_BEGIN));
    }
}
