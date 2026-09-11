//! 本地仓库扫描、remote 解析、身份一致性体检与一键修复建议。

use crate::error::Result;
use crate::git::infer::{infer, Inference};
use crate::git::url::parse_repo_url;
use crate::model::{Identity, ManagedRepo};
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

/// 克隆/初始化目标目录的探测结果。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClonePlan {
    /// empty | missing | parent | existingProject | alreadyGit | alreadyGitHasRemote
    pub kind: String,
    /// clone | init | addRemote | blocked
    pub suggested_mode: String,
    pub dest: String,
    pub target_path: String,
    pub repo_name: String,
    pub message: String,
    pub can_proceed: bool,
}

const IGNORE_ENTRIES: &[&str] = &[".ds_store", "thumbs.db", "desktop.ini"];

const PROJECT_MARKERS: &[&str] = &[
    "package.json",
    "cargo.toml",
    "pom.xml",
    "go.mod",
    "pyproject.toml",
    "requirements.txt",
    "composer.json",
    "gemfile",
    "cmakelists.txt",
    "makefile",
    "src",
    "lib",
    "app",
];

fn is_ignored_name(name: &str) -> bool {
    IGNORE_ENTRIES.contains(&name.to_ascii_lowercase().as_str())
}

/// 目录是否不存在、或仅含系统垃圾文件。
pub fn is_effectively_empty(dir: &Path) -> bool {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return false,
    };
    rd.flatten().all(|e| is_ignored_name(&e.file_name().to_string_lossy()))
}

fn looks_like_project(dir: &Path) -> bool {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return false,
    };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let lower = name.to_ascii_lowercase();
        if PROJECT_MARKERS.contains(&lower.as_str()) {
            return true;
        }
        if lower.ends_with(".sln") || lower.ends_with(".csproj") || lower.ends_with(".code-workspace") {
            return true;
        }
    }
    false
}

fn dir_name(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// 根据用户选定的文件夹与仓库名，决定 clone / init / 加 remote / 拒绝。
///
/// - 空目录或不存在：在该路径执行 `git clone`（目录即仓库根）。
/// - 已有项目文件、无 .git：在该路径执行 `git init` 并绑定远程。
/// - 非空且不像项目（更像工作区父目录）：在其下新建 `repo_name` 再 clone。
/// - 已是 Git 仓库：无 origin 则补 remote，有 origin 则拒绝。
pub fn plan_clone_or_init(dest: &Path, repo_name: &str) -> ClonePlan {
    let dest_str = dest.display().to_string();
    let repo = repo_name.trim();
    let repo = if repo.is_empty() { "repo" } else { repo };

    if !dest.exists() {
        return ClonePlan {
            kind: "missing".into(),
            suggested_mode: "clone".into(),
            dest: dest_str.clone(),
            target_path: dest_str,
            repo_name: repo.into(),
            message: "目标目录尚不存在，将创建并执行 git clone（目录即仓库根）。".into(),
            can_proceed: true,
        };
    }

    if !dest.is_dir() {
        return ClonePlan {
            kind: "blocked".into(),
            suggested_mode: "blocked".into(),
            dest: dest_str.clone(),
            target_path: dest_str,
            repo_name: repo.into(),
            message: "选定路径不是文件夹，请另选目录。".into(),
            can_proceed: false,
        };
    }

    if dest.join(".git").exists() {
        let dest_s = dest_str.clone();
        let has_origin = sys::run("git", &["-C", &dest_s, "remote", "get-url", "origin"])
            .ok()
            .map(|(o, _, c)| c == 0 && !o.trim().is_empty())
            .unwrap_or(false);
        if has_origin {
            return ClonePlan {
                kind: "alreadyGitHasRemote".into(),
                suggested_mode: "blocked".into(),
                dest: dest_str.clone(),
                target_path: dest_str,
                repo_name: repo.into(),
                message: "该目录已是 Git 仓库且已配置 origin。请另选空文件夹克隆，或另选未初始化的项目目录。".into(),
                can_proceed: false,
            };
        }
        return ClonePlan {
            kind: "alreadyGit".into(),
            suggested_mode: "addRemote".into(),
            dest: dest_str.clone(),
            target_path: dest_str,
            repo_name: repo.into(),
            message: "该目录已是 Git 仓库但没有 origin，将绑定别名远程地址（不拉取、不覆盖文件）。".into(),
            can_proceed: true,
        };
    }

    if is_effectively_empty(dest) {
        return ClonePlan {
            kind: "empty".into(),
            suggested_mode: "clone".into(),
            dest: dest_str.clone(),
            target_path: dest_str,
            repo_name: repo.into(),
            message: "目标为空文件夹，将在此目录执行 git clone（目录即仓库根）。".into(),
            can_proceed: true,
        };
    }

    let name_matches_repo = dir_name(dest).eq_ignore_ascii_case(repo);
    if looks_like_project(dest) || name_matches_repo {
        return ClonePlan {
            kind: "existingProject".into(),
            suggested_mode: "init".into(),
            dest: dest_str.clone(),
            target_path: dest_str,
            repo_name: repo.into(),
            message: "该目录已有项目文件但不是 Git 仓库。将执行 git init 并绑定远程（不会覆盖现有文件，也不会自动拉取）。".into(),
            can_proceed: true,
        };
    }

    let child = dest.join(repo);
    if child.exists() {
        if child.join(".git").exists() {
            return ClonePlan {
                kind: "alreadyGitHasRemote".into(),
                suggested_mode: "blocked".into(),
                dest: dest_str,
                target_path: child.display().to_string(),
                repo_name: repo.into(),
                message: format!("子目录「{repo}」已存在且是 Git 仓库。请另选空文件夹，或直接打开该仓库。"),
                can_proceed: false,
            };
        }
        if !is_effectively_empty(&child) {
            return ClonePlan {
                kind: "existingProject".into(),
                suggested_mode: "init".into(),
                dest: dest_str,
                target_path: child.display().to_string(),
                repo_name: repo.into(),
                message: format!("子目录「{repo}」已有内容但不是 Git 仓库。将对其执行 git init 并绑定远程（不覆盖现有文件）。"),
                can_proceed: true,
            };
        }
    }

    ClonePlan {
        kind: "parent".into(),
        suggested_mode: "clone".into(),
        dest: dest_str,
        target_path: child.display().to_string(),
        repo_name: repo.into(),
        message: format!("所选目录非空，更像工作区。将在其下新建空目录「{repo}」并执行 git clone。"),
        can_proceed: true,
    }
}

/// 统一路径比较：反斜杠、尾斜杠、Windows 大小写。
pub fn normalize_repo_path(p: &str) -> String {
    let mut s = p.trim().replace('\\', "/");
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    #[cfg(windows)]
    {
        s.make_ascii_lowercase();
    }
    s
}

pub fn display_repo_name(path: &str) -> String {
    let n = path.replace('\\', "/");
    n.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

/// 按规范化路径插入或更新已登记仓库。返回 (是否新建, id)。
pub fn upsert_managed_repo(repos: &mut Vec<ManagedRepo>, incoming: ManagedRepo) -> (bool, String) {
    let key = normalize_repo_path(&incoming.path);
    if let Some(existing) = repos
        .iter_mut()
        .find(|r| normalize_repo_path(&r.path) == key)
    {
        existing.path = incoming.path;
        if !incoming.name.is_empty() {
            existing.name = incoming.name;
        }
        if incoming.remote_url.is_some() {
            existing.remote_url = incoming.remote_url;
        }
        if incoming.identity_id.is_some() {
            existing.identity_id = incoming.identity_id;
        }
        if !incoming.source.is_empty() {
            existing.source = incoming.source;
        }
        if existing.machine_id.trim().is_empty() && !incoming.machine_id.trim().is_empty() {
            existing.machine_id = incoming.machine_id;
        }
        (false, existing.id.clone())
    } else {
        let id = incoming.id.clone();
        repos.push(incoming);
        (true, id)
    }
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
            updated_at: String::new(),
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

    fn temp_dir(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("gam-clone-plan-{}-{}", label, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn plan_missing_dir_is_clone() {
        let p = std::env::temp_dir().join(format!("gam-missing-{}", uuid::Uuid::new_v4()));
        let plan = plan_clone_or_init(&p, "demo");
        assert_eq!(plan.suggested_mode, "clone");
        assert_eq!(plan.kind, "missing");
        assert!(plan.can_proceed);
        assert_eq!(plan.target_path, p.display().to_string());
    }

    #[test]
    fn plan_empty_dir_is_clone_into_itself() {
        let p = temp_dir("empty");
        let plan = plan_clone_or_init(&p, "demo");
        assert_eq!(plan.kind, "empty");
        assert_eq!(plan.suggested_mode, "clone");
        assert_eq!(plan.target_path, p.display().to_string());
        std::fs::remove_dir_all(&p).ok();
    }

    #[test]
    fn plan_existing_project_is_init() {
        let p = temp_dir("proj");
        std::fs::write(p.join("package.json"), "{}").unwrap();
        let plan = plan_clone_or_init(&p, "other-name");
        assert_eq!(plan.kind, "existingProject");
        assert_eq!(plan.suggested_mode, "init");
        assert_eq!(plan.target_path, p.display().to_string());
        std::fs::remove_dir_all(&p).ok();
    }

    #[test]
    fn plan_parent_workspace_clones_into_child() {
        let p = temp_dir("work");
        std::fs::create_dir_all(p.join("unrelated")).unwrap();
        let plan = plan_clone_or_init(&p, "demo-repo");
        assert_eq!(plan.kind, "parent");
        assert_eq!(plan.suggested_mode, "clone");
        assert_eq!(plan.target_path, p.join("demo-repo").display().to_string());
        std::fs::remove_dir_all(&p).ok();
    }

    #[test]
    fn plan_folder_named_as_repo_is_init() {
        let root = temp_dir("named");
        let p = root.join("my-app");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("readme.txt"), "hi").unwrap();
        let plan = plan_clone_or_init(&p, "my-app");
        assert_eq!(plan.suggested_mode, "init");
        assert_eq!(plan.kind, "existingProject");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn normalize_repo_path_unifies_slash_and_case() {
        let slashes = normalize_repo_path(r"D:\Work\Repo\");
        assert_eq!(slashes, normalize_repo_path("D:/Work/Repo"));
        assert_eq!(display_repo_name(r"D:\Work\demo-app"), "demo-app");
        #[cfg(windows)]
        {
            assert_eq!(slashes, normalize_repo_path("d:/work/repo"));
        }
        #[cfg(not(windows))]
        {
            assert_ne!(
                normalize_repo_path(r"D:\Work\Repo\"),
                normalize_repo_path("d:/work/repo"),
                "Unix 路径大小写敏感，不应折叠"
            );
        }
    }

    #[test]
    fn upsert_managed_repo_updates_same_path() {
        let mut repos = Vec::new();
        let first = ManagedRepo {
            id: "r1".into(),
            path: r"D:\Work\App".into(),
            name: "App".into(),
            remote_url: Some("git@github.com:o/r.git".into()),
            identity_id: None,
            added_at: "t1".into(),
            source: "scan".into(),
            machine_id: "m1".into(),
        };
        let (is_new, id) = upsert_managed_repo(&mut repos, first);
        assert!(is_new);
        assert_eq!(id, "r1");
        let (is_new, id) = upsert_managed_repo(
            &mut repos,
            ManagedRepo {
                id: "r2".into(),
                path: "D:/Work/App".into(),
                name: "App".into(),
                remote_url: Some("git@github-x:o/r.git".into()),
                identity_id: Some("i1".into()),
                added_at: "t2".into(),
                source: "clone".into(),
                machine_id: "m1".into(),
            },
        );
        assert!(!is_new);
        assert_eq!(id, "r1");
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].identity_id.as_deref(), Some("i1"));
        assert_eq!(repos[0].remote_url.as_deref(), Some("git@github-x:o/r.git"));
    }

    #[cfg(windows)]
    #[test]
    fn upsert_managed_repo_treats_windows_case_as_same() {
        let mut repos = Vec::new();
        let (is_new, _) = upsert_managed_repo(
            &mut repos,
            ManagedRepo {
                id: "r1".into(),
                path: r"D:\Work\App".into(),
                name: "App".into(),
                remote_url: None,
                identity_id: None,
                added_at: "t1".into(),
                source: "scan".into(),
                machine_id: "m1".into(),
            },
        );
        assert!(is_new);
        let (is_new, id) = upsert_managed_repo(
            &mut repos,
            ManagedRepo {
                id: "r2".into(),
                path: "d:/work/app".into(),
                name: "App".into(),
                remote_url: Some("git@github-x:o/r.git".into()),
                identity_id: Some("i1".into()),
                added_at: "t2".into(),
                source: "clone".into(),
                machine_id: "m1".into(),
            },
        );
        assert!(!is_new);
        assert_eq!(id, "r1");
        assert_eq!(repos.len(), 1);
    }
}
