//! 本地仓库扫描、remote 解析、身份一致性体检与一键修复建议。

use crate::error::Result;
use crate::git::infer::{infer, Inference};
use crate::git::url::parse_repo_url;
use crate::model::Identity;
use crate::sys;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub path: String,
    pub remote_url: Option<String>,
    /// remote 解析出的别名（若地址已是别名形式）。
    pub current_alias: Option<String>,
    pub git_user_name: Option<String>,
    pub git_user_email: Option<String>,
    /// 推断出的应使用身份（用于体检）。
    pub inferred_identity: Option<String>,
    /// remote 用了真实主机而非别名，需要修复。
    pub needs_alias_fix: bool,
    /// 建议的修复命令。
    pub fix_command: Option<String>,
}

/// 从 `git remote get-url origin` 输出取 URL（去空白）。
pub fn parse_remote_url(output: &str) -> Option<String> {
    let s = output.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// 扫描根目录下的 Git 仓库（限深、跳过重目录、可中断由上层控制）。
pub fn scan(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, 0, max_depth, &mut found);
    found
}

fn walk(dir: &Path, depth: usize, max_depth: usize, found: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    if dir.join(".git").exists() {
        found.push(dir.to_path_buf());
        return; // 不递归进仓库内部
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if matches!(name, "node_modules" | "target" | ".venv" | ".git" | "dist" | "build") {
            continue;
        }
        walk(&p, depth + 1, max_depth, found);
    }
}

/// 读取单个仓库信息并做身份体检。
pub fn inspect(repo: &Path, identities: &[Identity], history: &HashMap<String, String>) -> RepoInfo {
    let path = repo.display().to_string();
    let remote_url = sys::run("git", &["-C", &path, "remote", "get-url", "origin"])
        .ok()
        .and_then(|(o, _, c)| if c == 0 { parse_remote_url(&o) } else { None });
    let git_user_name = git_config(&path, "user.name");
    let git_user_email = git_config(&path, "user.email");

    let mut info = RepoInfo {
        path: path.clone(),
        remote_url: remote_url.clone(),
        current_alias: None,
        git_user_name,
        git_user_email,
        inferred_identity: None,
        needs_alias_fix: false,
        fix_command: None,
    };

    if let Some(url) = &remote_url {
        if let Ok(parsed) = parse_repo_url(url) {
            if parsed.is_alias {
                info.current_alias = parsed.host.clone();
            }
            let inf: Inference = infer(&parsed, identities, history);
            if let Some(rec) = &inf.recommended {
                info.inferred_identity = Some(rec.identity_name.clone());
                // 若当前不是别名形式而推断出应使用别名，则建议修复。
                if !parsed.is_alias {
                    info.needs_alias_fix = true;
                    if let Some(new_url) = &inf.rewritten_url {
                        info.fix_command =
                            Some(format!("git -C \"{}\" remote set-url origin {}", path, new_url));
                    }
                }
            }
        }
    }
    info
}

fn git_config(path: &str, key: &str) -> Option<String> {
    sys::run("git", &["-C", path, "config", "--get", key])
        .ok()
        .and_then(|(o, _, c)| {
            if c == 0 && !o.trim().is_empty() {
                Some(o.trim().to_string())
            } else {
                None
            }
        })
}

/// 切换仓库身份：改 remote 为别名地址 + 设仓库级提交身份。
pub fn switch_identity(
    repo: &str,
    new_url: &str,
    user_name: Option<&str>,
    user_email: Option<&str>,
) -> Result<()> {
    sys::run("git", &["-C", repo, "remote", "set-url", "origin", new_url])?;
    if let Some(name) = user_name {
        sys::run("git", &["-C", repo, "config", "user.name", name])?;
    }
    if let Some(email) = user_email {
        sys::run("git", &["-C", repo, "config", "user.email", email])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str, alias: &str, host: &str, owners: &[&str]) -> Identity {
        Identity {
            id: format!("id-{name}"),
            name: name.into(),
            platform: "github".into(),
            host_alias: alias.into(),
            real_host: host.into(),
            user: "git".into(),
            email: None,
            git_user_name: None,
            key_id: None,
            owners: owners.iter().map(|s| s.to_string()).collect(),
            strict_mode: false,
        }
    }

    #[test]
    fn parse_remote_url_trims() {
        assert_eq!(
            parse_remote_url("git@github.com:o/r.git\n").as_deref(),
            Some("git@github.com:o/r.git")
        );
        assert_eq!(parse_remote_url("   "), None);
    }

    #[test]
    fn scan_finds_git_repos() {
        let root = std::env::temp_dir().join(format!("gam-scan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("proj-a").join(".git")).unwrap();
        std::fs::create_dir_all(root.join("sub").join("proj-b").join(".git")).unwrap();
        std::fs::create_dir_all(root.join("node_modules").join("pkg").join(".git")).unwrap();
        let found = scan(&root, 5);
        // 找到 proj-a 与 proj-b，跳过 node_modules。
        assert_eq!(found.len(), 2, "应跳过 node_modules 且找到两个仓库");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn checkup_flags_github_com_remote_needing_alias() {
        // 用推断逻辑验证 fix 建议（不依赖真实 git）。
        let ids = vec![id("techn4950", "github-techn", "github.com", &["vortaq-trad"])];
        let parsed = parse_repo_url("git@github.com:vortaq-trad/vq.git").unwrap();
        let inf = infer(&parsed, &ids, &HashMap::new());
        assert!(!parsed.is_alias);
        assert_eq!(
            inf.rewritten_url.as_deref(),
            Some("git@github-techn:vortaq-trad/vq.git")
        );
    }
}
