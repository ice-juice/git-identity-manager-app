//! SSH 工具链探测（落地 M0-5）：区分 System32 与 Git for Windows 两套 ssh，
//! 并判断 git 实际调用哪一套。版本解析逻辑独立可测。

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SshBinary {
    pub path: String,
    /// 解析出的版本，如 `9.5p2`。
    pub version: Option<String>,
    /// 标识来源：system / git。
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Toolchain {
    pub system: Option<SshBinary>,
    pub git: Option<SshBinary>,
    /// git 实际调用的 ssh 路径（读取 `git config core.sshCommand` 或默认）。
    pub git_uses: Option<String>,
}

/// 从 `ssh -V` 输出解析版本号。示例：
/// `OpenSSH_for_Windows_9.5p2, LibreSSL 3.8.2`
/// `OpenSSH_10.5p1, OpenSSL 3.x`
pub fn parse_ssh_version(text: &str) -> Option<String> {
    let t = text.trim();
    let token = t.split_whitespace().next()?; // OpenSSH_for_Windows_9.5p2,
    let token = token.trim_end_matches(',');
    // 取最后一个下划线段作为版本。
    let ver = token.rsplit('_').next()?;
    // 基本校验：含数字。
    if ver.chars().any(|c| c.is_ascii_digit()) {
        Some(ver.to_string())
    } else {
        None
    }
}

/// 候选路径（Windows）。真实版本需运行 `ssh -V` 再填。
pub fn candidate_paths() -> Vec<(String, PathBuf)> {
    let mut v = Vec::new();
    if let Ok(win) = std::env::var("SystemRoot") {
        v.push((
            "system".into(),
            PathBuf::from(win).join(r"System32\OpenSSH\ssh.exe"),
        ));
    }
    for p in [
        r"C:\Program Files\Git\usr\bin\ssh.exe",
        r"C:\Program Files (x86)\Git\usr\bin\ssh.exe",
    ] {
        v.push(("git".into(), PathBuf::from(p)));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_windows_openssh_version() {
        assert_eq!(
            parse_ssh_version("OpenSSH_for_Windows_9.5p2, LibreSSL 3.8.2"),
            Some("9.5p2".to_string())
        );
    }

    #[test]
    fn parses_git_openssh_version() {
        assert_eq!(
            parse_ssh_version("OpenSSH_10.5p1, OpenSSL 3.5.0"),
            Some("10.5p1".to_string())
        );
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_ssh_version("not a version string"), None);
    }
}
