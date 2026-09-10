//! 安全自查清单：同步读取工作区路径与 AppConfig，无后台扫描、无评分。

use crate::app_config::AppConfig;
use serde::Serialize;
use time::format_description::well_known::Rfc3339;

#[cfg(not(windows))]
mod unix;

/// 查看 OTP/密码的免密窗口：默认 5 分钟，配置上限 30。
/// 超过 15 分钟视为偏长——设置页下一档是 15/30，已明显高于推荐的 5 分钟。
const REVEAL_GRACE_LONG_MINUTES: u32 = 15;

/// 空闲自动锁定：默认 15 分钟；0 表示永不锁定。
/// 超过 30 分钟（默认的两倍）视为窗口偏长。
const AUTO_LOCK_LONG_MINUTES: u32 = 30;

const LIMITATION_CLIPBOARD: &str =
    "无法枚举剪贴板监听者或截屏调用方。剪贴板排除是给系统历史、云剪贴板和守规矩的管理器看的协作式约定，恶意程序可无视；不能防木马，也不会让截图变黑。";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SecuritySeverity {
    Ok,
    Info,
    Warn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SecurityLevel {
    Safe,
    Caution,
    Risk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SecurityCategory {
    Storage,
    App,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityFinding {
    pub id: String,
    pub category: SecurityCategory,
    pub severity: SecuritySeverity,
    pub title: String,
    pub detail: String,
    pub advice: String,
    pub settings_anchor: Option<String>,
    pub limitation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityChecklist {
    pub checked_at: String,
    pub level: SecurityLevel,
    pub items: Vec<SecurityFinding>,
}

/// 有 warn → risk；否则有 info → caution；否则 safe。不计算 0–100 分。
pub fn level_from_items(items: &[SecurityFinding]) -> SecurityLevel {
    if items.iter().any(|i| i.severity == SecuritySeverity::Warn) {
        SecurityLevel::Risk
    } else if items.iter().any(|i| i.severity == SecuritySeverity::Info) {
        SecurityLevel::Caution
    } else {
        SecurityLevel::Safe
    }
}

fn now_iso8601() -> String {
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// 直接复用 `commands::vault::check_workspace_path` 的关键词表，不另起一套。
pub fn workspace_sync_warning(path: &str) -> Option<String> {
    crate::commands::vault::check_workspace_path(path.to_string()).warning
}

pub fn build_checklist(cfg: &AppConfig) -> SecurityChecklist {
    let items = vec![
        item_workspace(cfg.workspace_path.as_deref()),
        item_clipboard_clear(cfg.clipboard_clear_seconds),
        item_reveal_grace(cfg.reveal_grace_minutes),
        item_lock_on_sleep(cfg.lock_on_sleep),
        item_auto_lock(cfg.auto_lock_minutes),
        item_clipboard_exclude(),
    ];
    SecurityChecklist {
        checked_at: now_iso8601(),
        level: level_from_items(&items),
        items,
    }
}

fn item_workspace(path: Option<&str>) -> SecurityFinding {
    match path {
        None => SecurityFinding {
            id: "storage.sync-drive".into(),
            category: SecurityCategory::Storage,
            severity: SecuritySeverity::Ok,
            title: "尚未设置工作空间路径".into(),
            detail: "当前没有工作空间路径可检查，因此未命中同步盘关键词。".into(),
            advice: "初始化工作空间时请避开 OneDrive / Dropbox / 坚果云等同步盘目录。".into(),
            settings_anchor: Some("workspace-path".into()),
            limitation: None,
        },
        Some(p) => match workspace_sync_warning(p) {
            Some(warning) => SecurityFinding {
                id: "storage.sync-drive".into(),
                category: SecurityCategory::Storage,
                severity: SecuritySeverity::Warn,
                title: "工作空间位于同步盘".into(),
                detail: warning,
                advice: "请把工作空间移到非同步盘目录。端到端备份应走应用内的云同步，不要让第三方同步盘接管整个工作空间。".into(),
                settings_anchor: Some("workspace-path".into()),
                limitation: None,
            },
            None => SecurityFinding {
                id: "storage.sync-drive".into(),
                category: SecurityCategory::Storage,
                severity: SecuritySeverity::Ok,
                title: "工作空间未落在常见同步盘".into(),
                detail: format!("当前路径未命中 OneDrive / Dropbox / Google Drive / iCloud / 坚果云 / 百度网盘 等关键词：{p}"),
                advice: "继续避免把整个工作空间交给第三方同步盘。".into(),
                settings_anchor: Some("workspace-path".into()),
                limitation: None,
            },
        },
    }
}

fn item_clipboard_clear(seconds: u32) -> SecurityFinding {
    if seconds == 0 {
        SecurityFinding {
            id: "app.clipboard-clear".into(),
            category: SecurityCategory::App,
            severity: SecuritySeverity::Warn,
            title: "复制后不会自动清空剪贴板".into(),
            detail: "clipboard_clear_seconds 为 0，验证码和密码会一直留在剪贴板。".into(),
            advice: "建议设为 10–20 秒，降低机密在剪贴板停留的时间。".into(),
            settings_anchor: Some("clipboard-clear".into()),
            limitation: Some(LIMITATION_CLIPBOARD.into()),
        }
    } else {
        SecurityFinding {
            id: "app.clipboard-clear".into(),
            category: SecurityCategory::App,
            severity: SecuritySeverity::Ok,
            title: "复制后会限时清空剪贴板".into(),
            detail: format!("当前 {seconds} 秒后尝试清空（仅当剪贴板仍是本次写入时）。"),
            advice: "保持开启即可。清空无法抹掉已被历史或监听者取走的副本。".into(),
            settings_anchor: Some("clipboard-clear".into()),
            limitation: Some(LIMITATION_CLIPBOARD.into()),
        }
    }
}

fn item_reveal_grace(minutes: u32) -> SecurityFinding {
    if minutes > REVEAL_GRACE_LONG_MINUTES {
        SecurityFinding {
            id: "app.reveal-grace".into(),
            category: SecurityCategory::App,
            severity: SecuritySeverity::Info,
            title: "查看机密的免密窗口较长".into(),
            detail: format!("当前免密查看时效为 {minutes} 分钟（阈值：超过 {REVEAL_GRACE_LONG_MINUTES} 分钟视为偏长）。"),
            advice: "建议不超过 5 分钟。取回 TOTP 原始密钥仍每次都要密码。".into(),
            settings_anchor: Some("reveal-grace".into()),
            limitation: None,
        }
    } else {
        SecurityFinding {
            id: "app.reveal-grace".into(),
            category: SecurityCategory::App,
            severity: SecuritySeverity::Ok,
            title: "查看机密的免密窗口在建议范围内".into(),
            detail: if minutes == 0 {
                "当前每次查看 OTP / 密码都要验证访问密码。".into()
            } else {
                format!("当前免密查看时效为 {minutes} 分钟。")
            },
            advice: "推荐 5 分钟；锁定或退出后立即失效。".into(),
            settings_anchor: Some("reveal-grace".into()),
            limitation: None,
        }
    }
}

fn item_lock_on_sleep(enabled: bool) -> SecurityFinding {
    if !enabled {
        SecurityFinding {
            id: "app.lock-on-sleep".into(),
            category: SecurityCategory::App,
            severity: SecuritySeverity::Info,
            title: "休眠或锁屏时不会自动锁定".into(),
            detail: "lock_on_sleep 为关闭，系统休眠/锁屏后保险库仍可能保持解锁。".into(),
            advice: "建议开启休眠/锁屏自动锁定，避免离开座位后机密仍可查看。".into(),
            settings_anchor: Some("lock-on-sleep".into()),
            limitation: None,
        }
    } else {
        SecurityFinding {
            id: "app.lock-on-sleep".into(),
            category: SecurityCategory::App,
            severity: SecuritySeverity::Ok,
            title: "休眠或锁屏时会自动锁定".into(),
            detail: "lock_on_sleep 已开启。".into(),
            advice: "保持开启即可。".into(),
            settings_anchor: Some("lock-on-sleep".into()),
            limitation: None,
        }
    }
}

fn item_auto_lock(minutes: u32) -> SecurityFinding {
    if minutes == 0 {
        SecurityFinding {
            id: "app.auto-lock".into(),
            category: SecurityCategory::App,
            severity: SecuritySeverity::Info,
            title: "已关闭空闲自动锁定".into(),
            detail: "auto_lock_minutes 为 0，空闲时不会自动锁定保险库。".into(),
            advice: "建议设为 15 分钟左右，避免长时间离开后仍保持解锁。".into(),
            settings_anchor: Some("auto-lock".into()),
            limitation: None,
        }
    } else if minutes > AUTO_LOCK_LONG_MINUTES {
        SecurityFinding {
            id: "app.auto-lock".into(),
            category: SecurityCategory::App,
            severity: SecuritySeverity::Info,
            title: "空闲自动锁定窗口较长".into(),
            detail: format!("当前 {minutes} 分钟无操作才锁定（阈值：0 或超过 {AUTO_LOCK_LONG_MINUTES} 分钟视为偏长）。"),
            advice: "建议不超过 30 分钟。".into(),
            settings_anchor: Some("auto-lock".into()),
            limitation: None,
        }
    } else {
        SecurityFinding {
            id: "app.auto-lock".into(),
            category: SecurityCategory::App,
            severity: SecuritySeverity::Ok,
            title: "空闲自动锁定已开启".into(),
            detail: format!("当前 {minutes} 分钟无操作后锁定。"),
            advice: "默认 15 分钟。设为 0 会关闭此项。".into(),
            settings_anchor: Some("auto-lock".into()),
            limitation: None,
        }
    }
}

fn item_clipboard_exclude() -> SecurityFinding {
    if crate::clipboard::exclusion_supported() {
        SecurityFinding {
            id: "app.clipboard-exclude".into(),
            category: SecurityCategory::App,
            severity: SecuritySeverity::Ok,
            title: "剪贴板排除标记可用".into(),
            detail: "本机为 Windows，复制机密时可请求系统历史与云剪贴板不要记录。".into(),
            advice: "排除标记只约束守规矩的监听者，不能阻止刻意无视标记的程序。".into(),
            settings_anchor: None,
            limitation: Some(LIMITATION_CLIPBOARD.into()),
        }
    } else {
        #[cfg(not(windows))]
        let detail = unix::clipboard_exclude_detail();
        #[cfg(not(windows))]
        let advice = unix::clipboard_exclude_advice();
        #[cfg(windows)]
        let detail = String::new();
        #[cfg(windows)]
        let advice = String::new();
        SecurityFinding {
            id: "app.clipboard-exclude".into(),
            category: SecurityCategory::App,
            severity: SecuritySeverity::Info,
            title: "当前平台无法打剪贴板排除标记".into(),
            detail,
            advice,
            settings_anchor: None,
            limitation: Some(LIMITATION_CLIPBOARD.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(severity: SecuritySeverity) -> SecurityFinding {
        SecurityFinding {
            id: "t".into(),
            category: SecurityCategory::App,
            severity,
            title: String::new(),
            detail: String::new(),
            advice: String::new(),
            settings_anchor: None,
            limitation: None,
        }
    }

    #[test]
    fn level_warn_is_risk() {
        let items = vec![
            finding(SecuritySeverity::Ok),
            finding(SecuritySeverity::Info),
            finding(SecuritySeverity::Warn),
        ];
        assert_eq!(level_from_items(&items), SecurityLevel::Risk);
    }

    #[test]
    fn level_info_without_warn_is_caution() {
        let items = vec![finding(SecuritySeverity::Ok), finding(SecuritySeverity::Info)];
        assert_eq!(level_from_items(&items), SecurityLevel::Caution);
    }

    #[test]
    fn level_all_ok_is_safe() {
        let items = vec![finding(SecuritySeverity::Ok), finding(SecuritySeverity::Ok)];
        assert_eq!(level_from_items(&items), SecurityLevel::Safe);
    }

    #[test]
    fn level_empty_is_safe() {
        assert_eq!(level_from_items(&[]), SecurityLevel::Safe);
    }

    #[test]
    fn sync_drive_keywords_reuse_vault_checker() {
        let hits = [
            r"C:\Users\me\OneDrive\vault",
            r"C:\Users\me\Dropbox\vault",
            r"C:\Users\me\Google Drive\work",
            r"C:\Users\me\GoogleDrive\work",
            r"/Users/me/Library/Mobile Documents/iCloud",
            r"D:\坚果云\vault",
            r"D:\Nutstore\Cloud",
            r"D:\百度网盘\vault",
            r"E:\BaiduNetdisk\data",
        ];
        for path in hits {
            assert!(
                workspace_sync_warning(path).is_some(),
                "应命中同步盘关键词：{path}"
            );
        }
        assert!(
            workspace_sync_warning(r"D:\Documents\git-vault").is_none(),
            "普通路径不应报警"
        );
    }

    #[test]
    fn onedrive_workspace_makes_checklist_risk() {
        let mut cfg = AppConfig::default();
        cfg.workspace_path = Some(r"C:\Users\me\OneDrive\git-vault".into());
        let list = build_checklist(&cfg);
        let item = list
            .items
            .iter()
            .find(|i| i.id == "storage.sync-drive")
            .expect("storage 项");
        assert_eq!(item.severity, SecuritySeverity::Warn);
        assert_eq!(item.category, SecurityCategory::Storage);
        assert_eq!(item.settings_anchor.as_deref(), Some("workspace-path"));
        assert_eq!(list.level, SecurityLevel::Risk);
        assert!(serde_json::to_value(&list).unwrap().get("score").is_none());
    }

    #[test]
    fn clipboard_clear_zero_is_warn_and_risk() {
        let mut cfg = AppConfig::default();
        cfg.clipboard_clear_seconds = 0;
        let list = build_checklist(&cfg);
        let item = list
            .items
            .iter()
            .find(|i| i.id == "app.clipboard-clear")
            .unwrap();
        assert_eq!(item.severity, SecuritySeverity::Warn);
        assert_eq!(item.settings_anchor.as_deref(), Some("clipboard-clear"));
        assert_eq!(list.level, SecurityLevel::Risk);
    }

    #[test]
    fn reveal_grace_and_auto_lock_thresholds() {
        let mut cfg = AppConfig::default();
        cfg.reveal_grace_minutes = 30;
        cfg.auto_lock_minutes = 60;
        cfg.lock_on_sleep = false;
        let list = build_checklist(&cfg);
        assert_eq!(
            list.items
                .iter()
                .find(|i| i.id == "app.reveal-grace")
                .unwrap()
                .severity,
            SecuritySeverity::Info
        );
        assert_eq!(
            list.items
                .iter()
                .find(|i| i.id == "app.auto-lock")
                .unwrap()
                .severity,
            SecuritySeverity::Info
        );
        assert_eq!(
            list.items
                .iter()
                .find(|i| i.id == "app.lock-on-sleep")
                .unwrap()
                .severity,
            SecuritySeverity::Info
        );
        assert_eq!(list.level, SecurityLevel::Caution);
    }

    #[test]
    fn auto_lock_zero_is_info() {
        let mut cfg = AppConfig::default();
        cfg.auto_lock_minutes = 0;
        let item = build_checklist(&cfg)
            .items
            .into_iter()
            .find(|i| i.id == "app.auto-lock")
            .unwrap();
        assert_eq!(item.severity, SecuritySeverity::Info);
        assert_eq!(item.settings_anchor.as_deref(), Some("auto-lock"));
    }

    #[test]
    fn default_app_findings_are_ok_except_platform_exclude() {
        let cfg = AppConfig::default();
        let list = build_checklist(&cfg);
        for id in [
            "app.clipboard-clear",
            "app.reveal-grace",
            "app.lock-on-sleep",
            "app.auto-lock",
        ] {
            assert_eq!(
                list.items.iter().find(|i| i.id == id).unwrap().severity,
                SecuritySeverity::Ok,
                "{id}"
            );
        }
        let exclude = list
            .items
            .iter()
            .find(|i| i.id == "app.clipboard-exclude")
            .unwrap();
        assert_eq!(exclude.settings_anchor, None);
        if crate::clipboard::exclusion_supported() {
            assert_eq!(exclude.severity, SecuritySeverity::Ok);
            assert_eq!(list.level, SecurityLevel::Safe);
        } else {
            assert_eq!(exclude.severity, SecuritySeverity::Info);
            assert_eq!(list.level, SecurityLevel::Caution);
        }
    }
}
