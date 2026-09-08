//! 非机密的本机状态（工作空间路径、自动锁定时长等），存于 %APPDATA%。
//! 判断标准：换台电脑还需要的东西放工作空间；只对本机有意义的放这里。

use crate::error::Result;
use crate::vault::atomic_write;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    /// 当前工作空间路径（未初始化时为 None）。
    pub workspace_path: Option<String>,
    /// 无操作自动锁定时长（分钟），0 表示永不。
    pub auto_lock_minutes: u32,
    /// 系统休眠/锁屏时立即锁定。
    pub lock_on_sleep: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            workspace_path: None,
            auto_lock_minutes: 15,
            lock_on_sleep: true,
        }
    }
}

fn config_dir() -> PathBuf {
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
        match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => AppConfig::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        atomic_write(&config_file(), json.as_bytes())
    }
}
