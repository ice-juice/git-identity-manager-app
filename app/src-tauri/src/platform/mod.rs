//! 平台抽象：所有平台相关操作收在此，业务代码禁用 `cfg!(windows)`。
//! 当前实现 Windows；新增 macOS/Linux 只需补 impl。

use crate::error::Result;
use std::path::{Path, PathBuf};

pub trait PlatformOps {
    /// 收紧私钥文件 ACL 到仅当前用户（Windows: icacls；Unix: chmod 600）。
    fn secure_key_file(&self, path: &Path) -> Result<()>;
    fn ssh_dir(&self) -> PathBuf;
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub fn current() -> impl PlatformOps {
    windows::Windows
}

#[cfg(not(windows))]
mod unix;
#[cfg(not(windows))]
pub fn current() -> impl PlatformOps {
    unix::Unix
}
