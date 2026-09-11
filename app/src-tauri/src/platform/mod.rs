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

#[cfg(mobile)]
mod mobile;
#[cfg(mobile)]
pub fn current() -> impl PlatformOps {
    mobile::Mobile
}

#[cfg(all(not(windows), not(mobile)))]
mod unix;
#[cfg(all(not(windows), not(mobile)))]
pub fn current() -> impl PlatformOps {
    unix::Unix
}

/// Tauri updater 清单里的 `{target}`，如 windows / darwin / linux。
/// 移动端不走自更新（交给应用商店），这里只为日志与诊断保留可读值。
pub fn updater_target() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "macos")]
    {
        "darwin"
    }
    #[cfg(target_os = "android")]
    {
        "android"
    }
    #[cfg(target_os = "ios")]
    {
        "ios"
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "android",
        target_os = "ios"
    )))]
    {
        "linux"
    }
}

/// Tauri updater 清单里的 `{arch}`。
pub fn updater_arch() -> &'static str {
    #[cfg(target_arch = "x86_64")]
    {
        "x86_64"
    }
    #[cfg(target_arch = "aarch64")]
    {
        "aarch64"
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        std::env::consts::ARCH
    }
}

/// 当前运行平台键，如 `windows-x86_64`。
pub fn updater_platform_key() -> String {
    format!("{}-{}", updater_target(), updater_arch())
}

/// 是否支持应用内整包自更新。Linux 仅 AppImage 可用；移动端一律交给应用商店。
pub fn self_update_supported() -> bool {
    #[cfg(mobile)]
    {
        false
    }
    #[cfg(all(not(mobile), target_os = "linux"))]
    {
        std::env::var_os("APPIMAGE").is_some()
    }
    #[cfg(all(not(mobile), not(target_os = "linux")))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_self_update_is_supported() {
        #[cfg(windows)]
        assert!(self_update_supported());
        #[cfg(all(not(mobile), target_os = "linux"))]
        assert_eq!(self_update_supported(), std::env::var_os("APPIMAGE").is_some());
        #[cfg(target_os = "macos")]
        assert!(self_update_supported());
        // 移动端必须关掉应用内自更新，否则会给出无效的更新入口。
        #[cfg(mobile)]
        assert!(!self_update_supported());
    }

    #[test]
    fn updater_platform_key_matches_target_arch() {
        let key = updater_platform_key();
        assert_eq!(key, format!("{}-{}", updater_target(), updater_arch()));
        assert!(
            key.starts_with("windows-")
                || key.starts_with("darwin-")
                || key.starts_with("linux-")
                || key.starts_with("android-")
                || key.starts_with("ios-"),
            "unexpected platform key {key}"
        );
    }
}
