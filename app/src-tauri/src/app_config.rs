//! 非机密的本机状态（工作空间路径、自动锁定时长等），存于 %APPDATA%。
//! 判断标准：换台电脑还需要的东西放工作空间；只对本机有意义的放这里。

use crate::error::Result;
use crate::vault::atomic_write;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_AUTO_SYNC_MINUTES: u32 = 30;
pub const MIN_AUTO_SYNC_MINUTES: u32 = 5;
pub const MAX_AUTO_SYNC_MINUTES: u32 = 24 * 60;

fn default_auto_sync_minutes() -> u32 {
    DEFAULT_AUTO_SYNC_MINUTES
}

/// 0 表示关闭；其余夹到 5–1440 分钟。
pub fn clamp_auto_sync_minutes(minutes: u32) -> u32 {
    if minutes == 0 {
        0
    } else {
        minutes.clamp(MIN_AUTO_SYNC_MINUTES, MAX_AUTO_SYNC_MINUTES)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    /// 当前工作空间路径（未初始化时为 None）。
    pub workspace_path: Option<String>,
    /// 无操作自动锁定时长（分钟），0 表示永不。
    pub auto_lock_minutes: u32,
    /// 系统休眠/锁屏时立即锁定。
    pub lock_on_sleep: bool,
    /// 登录 Windows 后自动启动本应用。
    #[serde(default)]
    pub launch_at_login: bool,
    /// 免验证天数：0 表示每次开机都要输入访问密码；1–30 为宽限期。
    #[serde(default)]
    pub grace_days: u32,
    /// 云端同步存储配置（v1.1）
    #[serde(default)]
    pub cloud_sync: Option<crate::sync::s3::S3Config>,
    /// 关闭主窗口时的记住选择：`tray` 最小化到托盘，`quit` 退出；`None` 每次询问。
    #[serde(default)]
    pub close_action: Option<String>,
    /// 自动同步间隔（分钟）。0 关闭；默认 30。打开程序时仍会先拉取一次。
    #[serde(default = "default_auto_sync_minutes")]
    pub auto_sync_minutes: u32,
    /// 最近一次自动/手动云同步时间（ISO）。
    #[serde(default)]
    pub last_auto_sync_at: Option<String>,
    /// 最近一次自动同步说明（成功或失败摘要）。
    #[serde(default)]
    pub last_auto_sync_message: Option<String>,
    /// 本机安装实例 ID。换机/重装会变，不进云端工作空间。
    #[serde(default)]
    pub machine_id: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            workspace_path: None,
            auto_lock_minutes: 15,
            lock_on_sleep: true,
            launch_at_login: false,
            grace_days: 0,
            cloud_sync: None,
            close_action: None,
            auto_sync_minutes: DEFAULT_AUTO_SYNC_MINUTES,
            last_auto_sync_at: None,
            last_auto_sync_message: None,
            machine_id: String::new(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("git-account-manager")
}

fn config_file() -> PathBuf {
    config_dir().join("config.json")
}

impl AppConfig {
    pub fn load() -> Self {
        let path = config_file();
        let mut cfg = match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => AppConfig::default(),
        };
        cfg.ensure_machine_id();
        cfg
    }

    pub fn ensure_machine_id(&mut self) -> &str {
        if self.machine_id.trim().is_empty() {
            self.machine_id = uuid::Uuid::new_v4().to_string();
            let _ = self.save();
        }
        &self.machine_id
    }

    pub fn current_machine_id() -> String {
        Self::load().machine_id
    }

    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        atomic_write(&config_file(), json.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_without_close_action_defaults_to_ask() {
        let raw = r#"{"workspacePath":null,"autoLockMinutes":15,"lockOnSleep":true}"#;
        let cfg: AppConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(cfg.close_action, None);
        assert_eq!(cfg.auto_lock_minutes, 15);
        assert_eq!(cfg.auto_sync_minutes, DEFAULT_AUTO_SYNC_MINUTES);
        assert!(cfg.machine_id.is_empty());
    }

    #[test]
    fn clamp_auto_sync_minutes_off_and_range() {
        assert_eq!(clamp_auto_sync_minutes(0), 0);
        assert_eq!(clamp_auto_sync_minutes(1), MIN_AUTO_SYNC_MINUTES);
        assert_eq!(clamp_auto_sync_minutes(30), 30);
        assert_eq!(clamp_auto_sync_minutes(9999), MAX_AUTO_SYNC_MINUTES);
    }
}
