//! 仓库地址解析：识别 HTTPS / SSH / 浏览器地址栏 / 裸 owner-repo / gh CLI /
//! 带端口 SSH / 自建三段路径等形态，提取 host + owner + repo。

use crate::error::{AppError, Result};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedRepo {
    /// 主机（裸 owner/repo 时为 None）。已小写化。
    pub host: Option<String>,
    pub port: Option<u16>,
    /// 完整仓库路径（去 .git），如 `owner/repo` 或 `grp/sub/repo`。
    pub repo_path: String,
    /// owner 前缀（repo_path 去掉最后一段），可能含多段。
    pub owner: String,
    /// 仓库名（最后一段）。
    pub repo: String,
    /// host 疑似 SSH 别名（无点、非 IP）。
    pub is_alias: bool,
}

/// 浏览器地址/网页路径中的“分隔标记”，其后的段不属于仓库路径。
const MARKERS: &[&str] = &[
    "tree", "blob", "commit", "commits", "pull", "pulls", "issues", "issue", "releases",
    "release", "wiki", "actions", "branches", "tags", "compare", "settings", "-",
];

/// 解析仓库地址。
pub fn parse_repo_url(input: &str) -> Result<ParsedRepo> {
    let mut s = input.trim().to_string();
    if s.is_empty() {
        return Err(AppError::Invalid("地址为空".into()));
    }
    // gh CLI: `gh repo clone owner/repo`
    if let Some(rest) = s.strip_prefix("gh repo clone ") {
        s = rest.trim().to_string();
    }

    let (host, port, raw_path) = if s.contains("://") {
        parse_scheme(&s)?
    } else if s.starts_with("git@") || (s.contains('@') && s.contains(':') && !s.contains('/')) {
        parse_scp_like(&s)?
    } else if looks_scp_like(&s) {
        parse_scp_like(&s)?
    } else {
        // 无 scheme：可能是 `host.com/owner/repo` 或裸 `owner/repo`。
        parse_bare(&s)?
    };

    let repo_path = clean_path(&raw_path);
    if repo_path.is_empty() {
        return Err(AppError::Invalid("无法从地址中提取仓库路径".into()));
    }
    let segments: Vec<&str> = repo_path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return Err(AppError::Invalid("地址缺少 owner/repo".into()));
    }
    let repo = segments.last().unwrap().to_string();
    let owner = segments[..segments.len() - 1].join("/");

    let host_lc = host.map(|h| h.to_lowercase());
    let is_alias = host_lc
        .as_deref()
        .map(|h| is_alias_host(h))
        .unwrap_or(false);

    Ok(ParsedRepo {
        host: host_lc,
        port,
        repo_path: segments.join("/"),
        owner,
        repo,
        is_alias,
    })
}

fn looks_scp_like(s: &str) -> bool {
    // host:path 且冒号后不是端口数字（区别于 host:port/path 需 scheme）。
    if let Some(colon) = s.find(':') {
        let after = &s[colon + 1..];
        return !after.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(true)
            && s[..colon].contains('.')
            && !s.contains("://");
    }
    false
}

/// `git@host:owner/repo.git` 或 `host:owner/repo.git`
fn parse_scp_like(s: &str) -> Result<(Option<String>, Option<u16>, String)> {
    let without_user = match s.split_once('@') {
        Some((_u, rest)) => rest,
        None => s,
    };
    let (host, path) = without_user
        .split_once(':')
        .ok_or_else(|| AppError::Invalid("SSH 地址缺少 ':'".into()))?;
    Ok((Some(host.to_string()), None, path.to_string()))
}

/// `scheme://[user@]host[:port]/path`
fn parse_scheme(s: &str) -> Result<(Option<String>, Option<u16>, String)> {
    let after = s.splitn(2, "://").nth(1).unwrap_or("");
    let (authority, path) = match after.split_once('/') {
        Some((a, p)) => (a, p.to_string()),
        None => (after, String::new()),
    };
    // 去掉 user@
    let hostport = authority.split('@').last().unwrap_or(authority);
    let (host, port) = match hostport.split_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().ok()),
        None => (hostport.to_string(), None),
    };
    Ok((Some(host), port, path))
}

/// 无 scheme：`host.com/owner/repo` 或裸 `owner/repo`
fn parse_bare(s: &str) -> Result<(Option<String>, Option<u16>, String)> {
    let segments: Vec<&str> = s.split('/').filter(|x| !x.is_empty()).collect();
    if segments.is_empty() {
        return Err(AppError::Invalid("无法解析地址".into()));
    }
    // 首段含 '.' 且总段数 >= 3 → 视为 host/owner/repo。
    if segments[0].contains('.') && segments.len() >= 3 {
        let host = segments[0].to_string();
        let path = segments[1..].join("/");
        Ok((Some(host), None, path))
    } else {
        // 裸 owner/repo（host 未知）。
        Ok((None, None, s.to_string()))
    }
}

/// 清洗路径：去前导斜杠、去 .git、在分隔标记处截断。
fn clean_path(raw: &str) -> String {
    let trimmed = raw.trim_start_matches('/').trim_end_matches('/');
    let mut segments: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
    // 在首个标记处截断。
    if let Some(pos) = segments.iter().position(|seg| MARKERS.contains(&seg.to_lowercase().as_str())) {
        segments.truncate(pos);
    }
    let joined = segments.join("/");
    joined
        .strip_suffix(".git")
        .map(|s| s.to_string())
        .unwrap_or(joined)
}

fn is_alias_host(host: &str) -> bool {
    if host == "localhost" {
        return false;
    }
    // IPv4/IPv6 粗判。
    if host.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ':') {
        return false;
    }
    !host.contains('.')
}

/// 改写为别名地址：`git@<alias>:<repo_path>.git`
pub fn rewrite_to_alias(alias: &str, repo_path: &str) -> String {
    format!("git@{}:{}.git", alias, repo_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_with_git_suffix() {
        let p = parse_repo_url("https://github.com/owner/repo.git").unwrap();
        assert_eq!(p.host.as_deref(), Some("github.com"));
        assert_eq!(p.owner, "owner");
        assert_eq!(p.repo, "repo");
        assert!(!p.is_alias);
    }

    #[test]
    fn browser_address_bar_tree_path() {
        let p = parse_repo_url("https://github.com/vortaq-trad/vq-officail/tree/main/src").unwrap();
        assert_eq!(p.owner, "vortaq-trad");
        assert_eq!(p.repo, "vq-officail");
        assert_eq!(p.repo_path, "vortaq-trad/vq-officail");
    }

    #[test]
    fn browser_pull_and_issues() {
        assert_eq!(parse_repo_url("https://github.com/o/r/pull/123").unwrap().repo, "r");
        assert_eq!(parse_repo_url("https://github.com/o/r/issues").unwrap().owner, "o");
    }

    #[test]
    fn ssh_scp_like() {
        let p = parse_repo_url("git@github.com:owner/repo.git").unwrap();
        assert_eq!(p.host.as_deref(), Some("github.com"));
        assert_eq!(p.repo_path, "owner/repo");
        assert!(!p.is_alias);
    }

    #[test]
    fn ssh_alias_form_detected() {
        let p = parse_repo_url("git@github-techn:owner/repo.git").unwrap();
        assert_eq!(p.host.as_deref(), Some("github-techn"));
        assert!(p.is_alias, "无点主机名视为别名");
    }

    #[test]
    fn bare_owner_repo() {
        let p = parse_repo_url("owner/repo").unwrap();
        assert_eq!(p.host, None);
        assert_eq!(p.owner, "owner");
        assert_eq!(p.repo, "repo");
    }

    #[test]
    fn gh_cli() {
        let p = parse_repo_url("gh repo clone octocat/Hello-World").unwrap();
        assert_eq!(p.owner, "octocat");
        assert_eq!(p.repo, "Hello-World");
    }

    #[test]
    fn ssh_with_port() {
        let p = parse_repo_url("ssh://git@host.example.com:2222/owner/repo.git").unwrap();
        assert_eq!(p.host.as_deref(), Some("host.example.com"));
        assert_eq!(p.port, Some(2222));
        assert_eq!(p.repo_path, "owner/repo");
    }

    #[test]
    fn self_hosted_three_segment_path() {
        let p = parse_repo_url(
            "git@ssh.boxexchanger.net:bx4/vchangr-com/exchanger-client-web.git",
        )
        .unwrap();
        assert_eq!(p.host.as_deref(), Some("ssh.boxexchanger.net"));
        assert_eq!(p.repo_path, "bx4/vchangr-com/exchanger-client-web");
        assert_eq!(p.owner, "bx4/vchangr-com");
        assert_eq!(p.repo, "exchanger-client-web");
    }

    #[test]
    fn gitlab_dash_marker() {
        // GitLab 嵌套 group 用 `/-/` 分隔视图路径。
        let p = parse_repo_url("https://gitlab.com/group/subgroup/proj/-/tree/main").unwrap();
        assert_eq!(p.repo_path, "group/subgroup/proj");
        assert_eq!(p.repo, "proj");
    }

    #[test]
    fn rewrite_alias_address() {
        assert_eq!(
            rewrite_to_alias("github-techn", "owner/repo"),
            "git@github-techn:owner/repo.git"
        );
    }

    #[test]
    fn host_slash_owner_repo_without_scheme() {
        let p = parse_repo_url("github.com/owner/repo").unwrap();
        assert_eq!(p.host.as_deref(), Some("github.com"));
        assert_eq!(p.repo_path, "owner/repo");
    }
}
