//! Windows 平台实现：icacls 收紧私钥 ACL（落地 M0-4）。

use super::PlatformOps;
use crate::error::{AppError, Result};
use crate::sys;
use std::path::{Path, PathBuf};

pub struct Windows;

impl PlatformOps for Windows {
    fn secure_key_file(&self, path: &Path) -> Result<()> {
        let p = path.to_string_lossy().to_string();
        let user = std::env::var("USERNAME").unwrap_or_default();
        let grant = format!("{user}:F");
        // 关闭继承并只授予当前用户完全控制。
        let (_o, e, code) = sys::run(
            "icacls",
            &[&p, "/inheritance:r", "/grant:r", &grant],
        )?;
        if code != 0 {
            return Err(AppError::Io(format!("icacls 收紧权限失败：{e}")));
        }
        Ok(())
    }

    fn ssh_dir(&self) -> PathBuf {
        sys::ssh_dir()
    }
}
