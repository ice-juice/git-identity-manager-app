//! 通用工具：变更预览 diff、安全备份、脱敏审计日志。

use crate::error::{AppError, Result};
use std::path::{Path, PathBuf};

/// 生成统一 diff（供落盘前预览）。
pub fn unified_diff(old: &str, new: &str) -> String {
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_lines(old, new);
    let mut out = String::new();
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        out.push_str(sign);
        out.push_str(change.value());
        if !change.value().ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

fn timestamp() -> String {
    use time::format_description::FormatItem;
    use time::macros::format_description;
    const FMT: &[FormatItem] =
        format_description!("[year][month][day]-[hour][minute][second]");
    time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .format(FMT)
        .unwrap_or_else(|_| "backup".to_string())
}

/// 备份文件到同目录 `<name>.bak.<时间戳>`；文件不存在则返回 None。
pub fn backup_file(path: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| AppError::Invalid("无效文件名".into()))?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let backup = dir.join(format!("{name}.bak.{}", timestamp()));
    std::fs::copy(path, &backup)?;
    Ok(Some(backup))
}

/// UTC RFC3339 时间戳。
pub fn now_rfc3339() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// 追加一条脱敏审计日志。
pub fn audit(workspace_root: &Path, action: &str) {
    use std::io::Write;
    let path = workspace_root.join("audit.log");
    let line = format!("{}  {}\n", iso_now(), redact(action));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn iso_now() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

/// 简单脱敏：屏蔽疑似私钥/口令/token 片段。
pub fn redact(s: &str) -> String {
    let mut out = s.to_string();
    for marker in ["ghp_", "github_pat_", "BEGIN OPENSSH PRIVATE KEY"] {
        if let Some(idx) = out.find(marker) {
            out.replace_range(idx.., "***redacted***");
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_marks_insert_and_delete() {
        let old = "a\nb\nc\n";
        let new = "a\nB\nc\n";
        let d = unified_diff(old, new);
        assert!(d.contains("-b"));
        assert!(d.contains("+B"));
        assert!(d.contains(" a"));
    }

    #[test]
    fn backup_copies_existing_file() {
        let dir = std::env::temp_dir().join(format!("gam-bak-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("config");
        std::fs::write(&f, "hello").unwrap();
        let b = backup_file(&f).unwrap().unwrap();
        assert!(b.exists());
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "hello");
        // 不存在的文件返回 None。
        assert!(backup_file(&dir.join("nope")).unwrap().is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn redact_hides_tokens() {
        assert!(redact("uploaded ghp_abcdef123").contains("***redacted***"));
        assert!(!redact("uploaded ghp_abcdef123").contains("abcdef123"));
    }
}
