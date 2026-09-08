//! `~/.ssh/config` 结构化解析与只读诊断（M2）。
//! 托管区块写入与原子落盘见 M3（`ssh::managed`）。

use serde::Serialize;

/// 一个 Host 块的结构化视图。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostBlock {
    /// `Host` 行可含多个模式（如 `Host github.com github-work`）。
    pub patterns: Vec<String>,
    /// 保留顺序的键值对（键规范化为规范大小写，如 HostName/IdentityFile）。
    pub options: Vec<(String, String)>,
    /// 该块在原文中的起始行号（0 基）。
    pub start_line: usize,
}

impl HostBlock {
    fn get(&self, key: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
    pub fn host_name(&self) -> Option<&str> {
        self.get("HostName")
    }
    pub fn identity_file(&self) -> Option<&str> {
        self.get("IdentityFile")
    }
    pub fn user(&self) -> Option<&str> {
        self.get("User")
    }
    pub fn port(&self) -> Option<&str> {
        self.get("Port")
    }
    pub fn identities_only(&self) -> bool {
        self.get("IdentitiesOnly")
            .map(|v| v.eq_ignore_ascii_case("yes"))
            .unwrap_or(false)
    }
    /// 该块的“真实主机”：优先 HostName，否则退回第一个非通配模式。
    pub fn effective_host(&self) -> Option<String> {
        if let Some(h) = self.host_name() {
            return Some(h.to_string());
        }
        self.patterns
            .iter()
            .find(|p| !p.contains('*') && !p.contains('?'))
            .cloned()
    }
}

/// 解析结果。
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SshConfig {
    pub blocks: Vec<HostBlock>,
}

/// 规范化选项键为 OpenSSH 常见拼写（不影响语义，仅用于展示）。
fn canonical_key(k: &str) -> String {
    const KNOWN: &[&str] = &[
        "HostName",
        "User",
        "Port",
        "IdentityFile",
        "IdentitiesOnly",
        "PreferredAuthentications",
        "AddKeysToAgent",
        "UserKnownHostsFile",
        "StrictHostKeyChecking",
        "ProxyCommand",
        "ForwardAgent",
    ];
    for &known in KNOWN {
        if known.eq_ignore_ascii_case(k) {
            return known.to_string();
        }
    }
    k.to_string()
}

/// 解析 config 文本为结构化块（保留未知选项，忽略注释与空行）。
pub fn parse(text: &str) -> SshConfig {
    let mut blocks: Vec<HostBlock> = Vec::new();
    let mut current: Option<HostBlock> = None;

    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // 允许 `Key value` 或 `Key=value`。
        let (key, value) = split_kv(line);
        if key.eq_ignore_ascii_case("Host") {
            if let Some(b) = current.take() {
                blocks.push(b);
            }
            let patterns = value.split_whitespace().map(|s| s.to_string()).collect();
            current = Some(HostBlock {
                patterns,
                options: Vec::new(),
                start_line: idx,
            });
        } else if key.eq_ignore_ascii_case("Match") {
            // Match 块暂不结构化，作为独立空块占位以免选项串入上一个 Host。
            if let Some(b) = current.take() {
                blocks.push(b);
            }
            current = None;
        } else if let Some(b) = current.as_mut() {
            b.options.push((canonical_key(&key), value.to_string()));
        }
        // Host 之前的全局选项（少见）在只读视图中忽略。
    }
    if let Some(b) = current.take() {
        blocks.push(b);
    }
    SshConfig { blocks }
}

fn split_kv(line: &str) -> (String, String) {
    // 先按 '=' 再按空白。
    if let Some(eq) = line.find('=') {
        let (k, v) = line.split_at(eq);
        // 仅当 '=' 前无空白时才视为 key=value 形式；否则退回空白分割。
        if !k.trim().contains(char::is_whitespace) {
            return (k.trim().to_string(), v[1..].trim().trim_matches('"').to_string());
        }
    }
    let mut it = line.splitn(2, char::is_whitespace);
    let k = it.next().unwrap_or("").to_string();
    let v = it.next().unwrap_or("").trim().trim_matches('"').to_string();
    (k, v)
}

// ---------------- 诊断 ----------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub severity: Severity,
    /// 稳定机器码，前端据此提供“一键修复”。
    pub code: String,
    pub message: String,
    /// 相关 Host 模式（如适用）。
    pub host: Option<String>,
}

/// 只读诊断。`file_exists` 用于判断 IdentityFile 是否为孤儿条目（便于测试注入）。
pub fn diagnose<F: Fn(&str) -> bool>(cfg: &SshConfig, file_exists: F) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    // Host 重名检测。
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for b in &cfg.blocks {
        for p in &b.patterns {
            *seen.entry(p.to_lowercase()).or_insert(0) += 1;
        }
    }
    for (pat, count) in &seen {
        if *count > 1 {
            out.push(Diagnostic {
                severity: Severity::Error,
                code: "DUPLICATE_HOST".into(),
                message: format!("Host 模式 `{}` 出现了 {} 次，后者会覆盖前者，易造成串号", pat, count),
                host: Some(pat.clone()),
            });
        }
    }

    for b in &cfg.blocks {
        let host_label = b.patterns.first().cloned();
        // 缺 IdentitiesOnly yes（有指定 IdentityFile 却未收紧）。
        if b.identity_file().is_some() && !b.identities_only() {
            out.push(Diagnostic {
                severity: Severity::Warn,
                code: "MISSING_IDENTITIES_ONLY".into(),
                message: format!(
                    "Host `{}` 指定了 IdentityFile 但缺少 `IdentitiesOnly yes`，agent 可能拿别的 key 去试，导致串号",
                    host_label.clone().unwrap_or_default()
                ),
                host: host_label.clone(),
            });
        }
        // 孤儿 IdentityFile。
        if let Some(idf) = b.identity_file() {
            // 严格模式允许指向 .pub；判断存在性时按原路径。
            if !file_exists(idf) {
                out.push(Diagnostic {
                    severity: Severity::Error,
                    code: "ORPHAN_IDENTITY_FILE".into(),
                    message: format!(
                        "Host `{}` 的 IdentityFile 指向不存在的文件：{}",
                        host_label.clone().unwrap_or_default(),
                        idf
                    ),
                    host: host_label.clone(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // 取自笔记的真实场景：第一段缺 IdentitiesOnly，后两段规范。
    const SAMPLE: &str = r#"
# 个人主号
Host github.com
    HostName github.com
    User git
    IdentityFile C:\Users\juice\.ssh\id_ed25519_juice

Host github-techn
    HostName github.com
    User git
    IdentityFile C:\Users\juice\.ssh\id_ed25519_techn
    IdentitiesOnly yes

Host ssh.boxexchanger.net
    HostName ssh.boxexchanger.net
    User git
    Port 22
    IdentityFile D:\codeProgrammer\vchangr\git-ssh\vchangr-box-git-ssh
    IdentitiesOnly yes
"#;

    #[test]
    fn parses_all_host_blocks() {
        let cfg = parse(SAMPLE);
        assert_eq!(cfg.blocks.len(), 3);
        assert_eq!(cfg.blocks[0].patterns, vec!["github.com"]);
        assert_eq!(cfg.blocks[1].host_name(), Some("github.com"));
        assert_eq!(cfg.blocks[1].effective_host().as_deref(), Some("github.com"));
        assert_eq!(cfg.blocks[2].port(), Some("22"));
    }

    #[test]
    fn detects_missing_identities_only_on_github() {
        let cfg = parse(SAMPLE);
        let diags = diagnose(&cfg, |_| true);
        let hit = diags
            .iter()
            .find(|d| d.code == "MISSING_IDENTITIES_ONLY")
            .expect("应发现缺 IdentitiesOnly");
        assert_eq!(hit.host.as_deref(), Some("github.com"));
        // 后两段规范，不应再报。
        assert_eq!(
            diags.iter().filter(|d| d.code == "MISSING_IDENTITIES_ONLY").count(),
            1
        );
    }

    #[test]
    fn detects_orphan_identity_file() {
        let cfg = parse(SAMPLE);
        // 只有 techn 那把“存在”，其余都是孤儿。
        let diags = diagnose(&cfg, |p| p.contains("id_ed25519_techn"));
        let orphans: Vec<_> = diags.iter().filter(|d| d.code == "ORPHAN_IDENTITY_FILE").collect();
        assert_eq!(orphans.len(), 2);
    }

    #[test]
    fn detects_duplicate_host() {
        let text = "Host github.com\n HostName github.com\nHost github.com\n HostName github.com\n";
        let cfg = parse(text);
        let diags = diagnose(&cfg, |_| true);
        assert!(diags.iter().any(|d| d.code == "DUPLICATE_HOST"));
    }

    #[test]
    fn supports_equals_and_multi_pattern() {
        let text = "Host a b\n  HostName=example.com\n  IdentitiesOnly=yes\n  IdentityFile=/k\n";
        let cfg = parse(text);
        assert_eq!(cfg.blocks[0].patterns, vec!["a", "b"]);
        assert_eq!(cfg.blocks[0].host_name(), Some("example.com"));
        assert!(cfg.blocks[0].identities_only());
    }

    #[test]
    fn preserves_user_comments_are_ignored_in_structure() {
        let cfg = parse(SAMPLE);
        // 注释不进入结构，但块数量正确。
        assert_eq!(cfg.blocks.len(), 3);
    }
}
