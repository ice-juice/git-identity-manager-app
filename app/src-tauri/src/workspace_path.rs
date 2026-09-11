//! 工作空间目录规则：非法字符拒绝；空格 / 非 ASCII / 同步盘只警告。

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PathCheck {
    /// 可继续，但建议改路径。
    pub warning: Option<String>,
    /// 不可继续。
    pub error: Option<String>,
}

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

const SYNC_HITS: &[(&str, &str)] = &[
    ("onedrive", "OneDrive"),
    ("dropbox", "Dropbox"),
    ("google drive", "Google Drive"),
    ("googledrive", "Google Drive"),
    ("icloud", "iCloud"),
    ("坚果云", "坚果云"),
    ("nutstore", "坚果云"),
    ("百度网盘", "百度网盘"),
    ("baidunetdisk", "百度网盘"),
];

pub fn inspect(path: &str) -> PathCheck {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return PathCheck {
            warning: None,
            error: Some("请选择或填写工作空间目录".into()),
        };
    }
    if let Some(error) = illegal_reason(trimmed) {
        return PathCheck {
            warning: None,
            error: Some(error),
        };
    }
    let mut warnings = Vec::new();
    if let Some(w) = sync_drive_warning(trimmed) {
        warnings.push(w);
    }
    if let Some(w) = awkward_path_warning(trimmed) {
        warnings.push(w);
    }
    PathCheck {
        warning: if warnings.is_empty() {
            None
        } else {
            Some(warnings.join(" "))
        },
        error: None,
    }
}

pub fn reject_if_invalid(path: &str) -> crate::error::Result<()> {
    if let Some(error) = inspect(path).error {
        return Err(crate::error::AppError::Invalid(error));
    }
    Ok(())
}

pub fn sync_drive_warning(path: &str) -> Option<String> {
    let lower = path.to_lowercase();
    for (needle, label) in SYNC_HITS {
        if lower.contains(needle) {
            return Some(format!(
                "该路径疑似位于「{label}」同步盘内。端到端加密备份应走应用内的云同步通道，请勿让第三方同步盘接管整个工作空间目录。"
            ));
        }
    }
    None
}

fn awkward_path_warning(path: &str) -> Option<String> {
    let has_space = path.chars().any(|c| c.is_whitespace());
    let has_non_ascii = path.chars().any(|c| !c.is_ascii());
    if !has_space && !has_non_ascii {
        return None;
    }
    let mut why = Vec::new();
    if has_non_ascii {
        why.push("非英文字符");
    }
    if has_space {
        why.push("空格");
    }
    Some(format!(
        "该路径含有{}。Git / OpenSSH 在这种路径上容易出错，建议改用只含英文、数字、连字符的本地目录，例如 D:\\git-keymaster-ws。",
        why.join("和")
    ))
}

fn illegal_reason(path: &str) -> Option<String> {
    if path.chars().any(|c| c.is_control()) {
        return Some("路径含有不可见控制字符，无法用作工作空间。".into());
    }
    for (i, ch) in path.char_indices() {
        if matches!(ch, '<' | '>' | '"' | '|' | '?' | '*') {
            return Some(format!(
                "路径含有非法字符「{ch}」。请去掉 < > \" | ? * 这类符号。"
            ));
        }
        if ch == ':' && !is_windows_drive_colon(path, i) {
            return Some("路径中的冒号只能用在 Windows 盘符（例如 D:\\），其它位置不合法。".into());
        }
    }
    for component in path_components(path) {
        if component.is_empty() || component == "." || component == ".." {
            continue;
        }
        if component.ends_with(' ') || component.ends_with('.') {
            return Some("路径中的文件夹名不能以空格或小数点结尾。".into());
        }
        if is_reserved_component(component) {
            return Some(format!(
                "「{component}」是 Windows 保留名，不能作为工作空间目录的一部分。"
            ));
        }
    }
    None
}

fn is_windows_drive_colon(path: &str, colon_index: usize) -> bool {
    colon_index == 1
        && path
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
}

fn path_components(path: &str) -> impl Iterator<Item = &str> {
    let rest = if is_windows_drive_colon(path, 1) && path.len() >= 2 {
        &path[2..]
    } else {
        path
    };
    rest.split(['/', '\\'])
}

fn is_reserved_component(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_error() {
        assert!(inspect("").error.is_some());
        assert!(inspect("   ").error.is_some());
    }

    #[test]
    fn ascii_local_path_is_ok() {
        let c = inspect(r"D:\git-keymaster-ws");
        assert_eq!(c, PathCheck { warning: None, error: None });
    }

    #[test]
    fn illegal_chars_are_rejected() {
        assert!(inspect(r"D:\git|keymaster").error.unwrap().contains('|'));
        assert!(inspect(r"D:\foo:bar").error.unwrap().contains("冒号"));
        assert!(inspect(r"D:\CON\vault").error.unwrap().contains("CON"));
        assert!(inspect(r"D:\folder\NUL").error.unwrap().contains("NUL"));
        assert!(inspect(r"D:\ends-with. ").error.is_some());
    }

    #[test]
    fn drive_colon_is_allowed() {
        assert!(inspect(r"C:\git-keymaster-ws").error.is_none());
    }

    #[test]
    fn space_and_non_ascii_warn_but_allow() {
        let space = inspect(r"D:\git keymaster");
        assert!(space.error.is_none());
        assert!(space.warning.as_deref().unwrap().contains("空格"));

        let cjk = inspect(r"D:\我的密钥库");
        assert!(cjk.error.is_none());
        assert!(cjk.warning.as_deref().unwrap().contains("非英文字符"));
    }

    #[test]
    fn sync_drive_still_warns() {
        let c = inspect(r"C:\Users\me\OneDrive\git-keymaster-ws");
        assert!(c.error.is_none());
        assert!(c.warning.as_deref().unwrap().contains("OneDrive"));
    }

    #[test]
    fn sync_and_space_combine() {
        let c = inspect(r"C:\Users\me\OneDrive\git vault");
        let w = c.warning.unwrap();
        assert!(w.contains("OneDrive"));
        assert!(w.contains("空格"));
    }
}
