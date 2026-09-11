//! 非机密的本机状态（工作空间路径、自动锁定时长等），存于 %APPDATA%。
//! 判断标准：换台电脑还需要的东西放工作空间；只对本机有意义的放这里。

use crate::error::Result;
use crate::vault::atomic_write;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_AUTO_SYNC_MINUTES: u32 = 30;
pub const MIN_AUTO_SYNC_MINUTES: u32 = 5;
pub const MAX_AUTO_SYNC_MINUTES: u32 = 24 * 60;
/// 内置默认更新源仓库（出厂值，用户可覆盖）。
pub const DEFAULT_UPDATE_REPO: &str = "ice-juice/git-keymaster-app";

fn default_auto_sync_minutes() -> u32 {
    DEFAULT_AUTO_SYNC_MINUTES
}

fn default_true() -> bool {
    true
}

fn default_reveal_grace_minutes() -> u32 {
    5
}

fn default_clipboard_clear_seconds() -> u32 {
    20
}

fn default_account_history_limit() -> u32 {
    10
}

pub fn clamp_reveal_grace_minutes(minutes: u32) -> u32 {
    minutes.min(30)
}

pub fn clamp_clipboard_clear_seconds(seconds: u32) -> u32 {
    if seconds == 0 {
        0
    } else {
        seconds.clamp(5, 120)
    }
}

pub fn clamp_account_history_limit(n: u32) -> u32 {
    n.clamp(1, 50)
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
    /// 更新源（None 表示使用内置默认 GitHub 仓库）。
    #[serde(default)]
    pub update_source: Option<UpdateSource>,
    /// 启动后自动检查更新（默认开启）。
    #[serde(default = "default_true")]
    pub auto_check_update: bool,
    /// 用户「跳过」的版本号，避免重复打扰。
    #[serde(default)]
    pub skipped_update_version: Option<String>,
    /// 最近一次检查时间（ISO）。
    #[serde(default)]
    pub last_update_check_at: Option<String>,
    /// 本机网络代理（访问 GitHub / GitLab / 更新源等）。None 表示未配置。
    #[serde(default)]
    pub network_proxy: Option<NetworkProxy>,
    /// 查看 OTP/密码的免密时效（分钟）。0 = 每次都验。
    #[serde(default = "default_reveal_grace_minutes")]
    pub reveal_grace_minutes: u32,
    /// 复制机密后清空剪贴板的秒数。0 = 不清空。
    #[serde(default = "default_clipboard_clear_seconds")]
    pub clipboard_clear_seconds: u32,
    /// 每个账号保留的密码历史条数。
    #[serde(default = "default_account_history_limit")]
    pub account_history_limit: u32,
    /// 是否开启本机指纹解锁。默认关。
    #[serde(default)]
    pub biometric_unlock_enabled: bool,
    /// 查看 OTP/账密是否允许指纹重认证。
    #[serde(default)]
    pub biometric_reveal_enabled: bool,
    /// 是否允许指纹替代密码取回 TOTP 原始密钥。默认关。
    #[serde(default)]
    pub biometric_reveal_secret: bool,
}

/// 本机 HTTP/HTTPS/SOCKS5 代理。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkProxy {
    /// 总开关。false 时其余字段保留但不生效。
    pub enabled: bool,
    /// "http" | "https" | "socks5"
    pub scheme: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// 本应用拉起的 git HTTPS 是否带上代理环境变量。
    #[serde(default = "default_true")]
    pub apply_to_git_https: bool,
    /// 本应用拉起的 ssh / GIT_SSH_COMMAND 是否注入 ProxyCommand。
    #[serde(default = "default_true")]
    pub apply_to_ssh: bool,
    /// 云同步 reqwest 是否走代理。
    #[serde(default = "default_true")]
    pub apply_to_cloud_sync: bool,
}

impl Default for NetworkProxy {
    fn default() -> Self {
        NetworkProxy {
            enabled: false,
            scheme: "http".into(),
            host: "127.0.0.1".into(),
            port: 7890,
            username: None,
            password: None,
            apply_to_git_https: true,
            apply_to_ssh: true,
            apply_to_cloud_sync: true,
        }
    }
}

/// 本机更新源偏好。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSource {
    /// "github" | "manifest"
    pub kind: String,
    /// github 模式：owner/repo；manifest 模式留空。
    pub repo: Option<String>,
    /// manifest 模式：清单 URL；github 模式留空。
    pub manifest_url: Option<String>,
    /// 是否接受 pre-release（github 模式）。
    #[serde(default)]
    pub include_prerelease: bool,
}

impl Default for UpdateSource {
    fn default() -> Self {
        UpdateSource {
            kind: "github".into(),
            repo: Some(DEFAULT_UPDATE_REPO.into()),
            manifest_url: None,
            include_prerelease: false,
        }
    }
}

impl UpdateSource {
    /// None 配置或空 github 仓库回填为出厂默认源。
    pub fn effective(src: Option<&UpdateSource>) -> Self {
        match src {
            Some(src) => {
                let mut out = src.clone();
                if out.kind.trim().is_empty() {
                    out.kind = "github".into();
                }
                if out.kind == "github"
                    && out
                        .repo
                        .as_deref()
                        .map(str::trim)
                        .unwrap_or("")
                        .is_empty()
                {
                    out.repo = Some(DEFAULT_UPDATE_REPO.into());
                }
                out
            }
            None => UpdateSource::default(),
        }
    }
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
            update_source: None,
            auto_check_update: true,
            skipped_update_version: None,
            last_update_check_at: None,
            network_proxy: None,
            reveal_grace_minutes: default_reveal_grace_minutes(),
            clipboard_clear_seconds: default_clipboard_clear_seconds(),
            account_history_limit: default_account_history_limit(),
            biometric_unlock_enabled: false,
            biometric_reveal_enabled: false,
            biometric_reveal_secret: false,
        }
    }
}

pub fn config_dir() -> PathBuf {
    crate::identity::app_config_dir()
}

fn config_file() -> PathBuf {
    config_dir().join("config.json")
}

impl AppConfig {
    pub fn load() -> Self {
        crate::identity::migrate_app_data();
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
        assert_eq!(cfg.update_source, None);
        assert!(cfg.auto_check_update, "旧配置缺少字段时应默认开启自动检查");
        assert_eq!(cfg.skipped_update_version, None);
        assert_eq!(cfg.last_update_check_at, None);
        assert_eq!(cfg.network_proxy, None);
        let filled = UpdateSource::effective(cfg.update_source.as_ref());
        assert_eq!(filled.kind, "github");
        assert_eq!(filled.repo.as_deref(), Some(DEFAULT_UPDATE_REPO));
        assert!(!filled.include_prerelease);
    }

    #[test]
    fn empty_github_repo_falls_back_to_default() {
        let src = UpdateSource {
            kind: "github".into(),
            repo: Some("  ".into()),
            manifest_url: None,
            include_prerelease: true,
        };
        let filled = UpdateSource::effective(Some(&src));
        assert_eq!(filled.repo.as_deref(), Some(DEFAULT_UPDATE_REPO));
        assert!(filled.include_prerelease);
    }

    #[test]
    fn custom_github_repo_is_preserved() {
        let src = UpdateSource {
            kind: "github".into(),
            repo: Some("acme/mirror".into()),
            manifest_url: None,
            include_prerelease: false,
        };
        let filled = UpdateSource::effective(Some(&src));
        assert_eq!(filled.repo.as_deref(), Some("acme/mirror"));
    }

    #[test]
    fn clamp_auto_sync_minutes_off_and_range() {
        assert_eq!(clamp_auto_sync_minutes(0), 0);
        assert_eq!(clamp_auto_sync_minutes(1), MIN_AUTO_SYNC_MINUTES);
        assert_eq!(clamp_auto_sync_minutes(30), 30);
        assert_eq!(clamp_auto_sync_minutes(9999), MAX_AUTO_SYNC_MINUTES);
    }
}
