//! 产品本机标识。只在这里改名字，其它模块引用这些常量。
//!
//! 旧名 `git-account-manager` 只读兼容：首次启动把旧目录/文件拷到新位置，
//! 旧副本保留作备份，不覆盖已经存在的新路径。

use std::path::{Path, PathBuf};

/// 本机状态目录名（`%APPDATA%` / 临时目录下）。
pub const APP_DIR: &str = "git-keymaster";
pub const LEGACY_APP_DIR: &str = "git-account-manager";

/// `~/.ssh` 下的配置镜像文件名。
pub const SSH_INCLUDE: &str = "git-keymaster.config";
pub const LEGACY_SSH_INCLUDE: &str = "git-account-manager.config";

/// Git ssh-agent 固定套接字与 pid 文件名。
pub const AGENT_SOCK: &str = "git-keymaster";
pub const AGENT_PID: &str = "git-keymaster.pid";
pub const LEGACY_AGENT_SOCK: &str = "git-account-manager";
pub const LEGACY_AGENT_PID: &str = "git-account-manager.pid";

/// `~/.ssh/config` 托管区块标记。写入只用新标记；读取同时认旧标记。
pub const SSH_BEGIN: &str = "# ===== BEGIN managed by git-keymaster =====";
pub const SSH_END: &str = "# ===== END managed by git-keymaster =====";
pub const LEGACY_SSH_BEGIN: &str = "# ===== BEGIN managed by git-account-manager =====";
pub const LEGACY_SSH_END: &str = "# ===== END managed by git-account-manager =====";

/// PowerShell / Bash 启动脚本里的 agent 环境块。
pub const AGENT_BEGIN: &str = "# >>> git-keymaster agent >>>";
pub const AGENT_END: &str = "# <<< git-keymaster agent <<<";
pub const LEGACY_AGENT_BEGIN: &str = "# >>> git-account-manager agent >>>";
pub const LEGACY_AGENT_END: &str = "# <<< git-account-manager agent <<<";

pub const ENV_PS1: &str = "git-keymaster-agent.env.ps1";
pub const ENV_SH: &str = "git-keymaster-agent.env.sh";
pub const LEGACY_ENV_PS1: &str = "git-account-manager-agent.env.ps1";
pub const LEGACY_ENV_SH: &str = "git-account-manager-agent.env.sh";

/// 导出的 S3/R2 配置文件 `kind`。
pub const S3_KIND: &str = "git-keymaster-s3";
pub const LEGACY_S3_KIND: &str = "git-account-manager-s3";

/// 单实例锁文件名。
pub const INSTANCE_LOCK: &str = "com.jeck.gitkeymaster.instance.lock";
pub const LEGACY_INSTANCE_LOCK: &str = "com.jeck.gitaccountmanager.instance.lock";

/// HTTP User-Agent。
pub const USER_AGENT: &str = "git-keymaster";

/// 本机配置根目录（Windows 为 `%APPDATA%`，其它平台回退临时目录）。
pub fn config_base_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
}

pub fn app_config_dir() -> PathBuf {
    config_base_dir().join(APP_DIR)
}

pub fn legacy_app_config_dir() -> PathBuf {
    config_base_dir().join(LEGACY_APP_DIR)
}

/// 若新目录还没有 `config.json`、旧目录存在，则整目录拷贝到新位置。旧目录不删。
pub fn migrate_app_data() {
    let new_dir = app_config_dir();
    let old_dir = legacy_app_config_dir();
    if new_dir.join("config.json").is_file() {
        return;
    }
    if !old_dir.is_dir() {
        return;
    }
    match copy_dir_recursive(&old_dir, &new_dir) {
        Ok(()) => log::info!(
            "已从旧本机目录拷贝配置：{} → {}（旧目录保留作备份）",
            old_dir.display(),
            new_dir.display()
        ),
        Err(e) => log::warn!(
            "拷贝旧本机目录失败：{} → {}：{e}",
            old_dir.display(),
            new_dir.display()
        ),
    }
}

pub fn copy_file_if_missing(src: &Path, dest: &Path) -> bool {
    if dest.exists() || !src.is_file() {
        return false;
    }
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::copy(src, dest) {
        Ok(_) => {
            log::info!("已拷贝 {} → {}（旧文件保留作备份）", src.display(), dest.display());
            true
        }
        Err(e) => {
            log::warn!("拷贝 {} → {} 失败：{e}", src.display(), dest.display());
            false
        }
    }
}

pub fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else if !to.exists() {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

pub fn accepted_s3_kind(kind: &str) -> bool {
    kind.is_empty() || kind == S3_KIND || kind == LEGACY_S3_KIND
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_dir_keeps_old_and_skips_existing() {
        let root = std::env::temp_dir().join(format!(
            "gam-ident-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let old = root.join("old");
        let new = root.join("new");
        std::fs::create_dir_all(old.join("sub")).unwrap();
        std::fs::write(old.join("config.json"), b"old-cfg").unwrap();
        std::fs::write(old.join("sub").join("a.txt"), b"a").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("keep.json"), b"keep").unwrap();
        copy_dir_recursive(&old, &new).unwrap();
        assert_eq!(std::fs::read_to_string(new.join("config.json")).unwrap(), "old-cfg");
        assert_eq!(std::fs::read_to_string(new.join("sub").join("a.txt")).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(new.join("keep.json")).unwrap(), "keep");
        assert!(old.join("config.json").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn s3_kind_accepts_legacy_and_new() {
        assert!(accepted_s3_kind(""));
        assert!(accepted_s3_kind(S3_KIND));
        assert!(accepted_s3_kind(LEGACY_S3_KIND));
        assert!(!accepted_s3_kind("other-app"));
    }
}
