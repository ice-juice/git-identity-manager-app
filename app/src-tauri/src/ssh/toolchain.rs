//! SSH 工具链探测（落地 M0-5）：区分 System32 与 Git for Windows 两套 ssh，
//! Unix 上使用本机 / Homebrew OpenSSH。版本解析逻辑独立可测。

use serde::Serialize;
use std::path::{Path, PathBuf};

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

/// 当前运行的操作系统键：`windows` / `macos` / `linux`。
pub fn host_os() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        "linux"
    }
}

/// Windows 必须用 Git 自带 ssh（与 System32 OpenSSH 的 agent 不兼容）；
/// macOS / Linux 用本机或 Homebrew OpenSSH。
pub fn preferred_ssh_source() -> &'static str {
    #[cfg(windows)]
    {
        "git"
    }
    #[cfg(not(windows))]
    {
        "system"
    }
}

/// 候选路径。真实版本需运行 `ssh -V` 再填。
pub fn candidate_paths() -> Vec<(String, PathBuf)> {
    let mut v = Vec::new();
    #[cfg(windows)]
    {
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
    }
    #[cfg(not(windows))]
    {
        for p in [
            "/usr/bin/ssh",
            "/usr/local/bin/ssh",
            "/opt/homebrew/bin/ssh",
            "/opt/homebrew/opt/openssh/bin/ssh",
            "/usr/local/opt/openssh/bin/ssh",
            "/opt/local/bin/ssh",
        ] {
            v.push(("system".into(), PathBuf::from(p)));
        }
    }
    v
}

fn exe_name(name: &str) -> String {
    #[cfg(windows)]
    {
        if name.to_ascii_lowercase().ends_with(".exe") {
            name.to_string()
        } else {
            format!("{name}.exe")
        }
    }
    #[cfg(not(windows))]
    {
        name.to_string()
    }
}

fn sibling_in_source(source: &str, file: &str) -> Option<PathBuf> {
    for (src, ssh) in candidate_paths() {
        if src == source && ssh.exists() {
            let p = ssh.with_file_name(file);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

/// Windows 上只有 Git for Windows 目录里的 ssh 才可与本程序 agent 混用。
pub fn is_preferred_ssh_path(path: &Path) -> bool {
    #[cfg(windows)]
    {
        let s = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
        s.contains("/git/")
            && (s.contains("/usr/bin/") || s.contains("/mingw64/bin/") || s.contains("/mingw32/bin/"))
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        true
    }
}

/// 按当前操作系统查找配套的 `ssh` / `ssh-add` / `ssh-agent`。
pub fn find_ssh_tool(name: &str) -> Option<PathBuf> {
    let file = exe_name(name);
    if let Some(p) = sibling_in_source(preferred_ssh_source(), &file) {
        return Some(p);
    }
    let found = crate::sys::resolve_bin(&file)?;
    if is_preferred_ssh_path(&found) {
        Some(found)
    } else {
        None
    }
}

/// 界面上「用哪套 ssh」的短标签。
pub fn ssh_tool_label() -> &'static str {
    match host_os() {
        "windows" => "Git 自带 ssh",
        _ => "本机 OpenSSH",
    }
}

/// 未找到 ssh / ssh-add / ssh-agent 时的说明 + 按操作系统给出的解决方法。
pub fn missing_ssh_tool(tool: &str) -> String {
    match host_os() {
        "windows" => format!(
            "未找到 Git 自带的 {tool}。本程序在 Windows 上必须使用 Git for Windows 内置的 OpenSSH，不能用系统自带的 OpenSSH（两套 ssh-agent 互不兼容）。\n\n\
             解决方法：\n\
             1. 打开 https://git-scm.com/download/win 下载并安装 Git for Windows。\n\
             2. 安装时保持默认组件即可，会带上 usr\\bin\\ssh.exe / ssh-add.exe / ssh-agent.exe。\n\
             3. 装完后完全退出本程序再打开（只关窗口不够，若托盘里还在请退出）。\n\
             4. 可在资源管理器确认存在：C:\\Program Files\\Git\\usr\\bin\\ssh.exe"
        ),
        "macos" => format!(
            "未找到本机 {tool}（OpenSSH）。macOS 通常自带 ssh / ssh-add / ssh-agent，不需要安装 Git for Windows，也不需要 git-gui。\n\n\
             解决方法：\n\
             1. 打开「终端」执行：xcode-select --install\n\
             然后按提示装完「命令行工具」（会提供可用的 git 与配套工具）。\n\
             2. 若已安装 Homebrew，可再执行：brew install git\n\
             需要更新 OpenSSH 时再执行：brew install openssh\n\
             3. git-gui 只是图形界面，本程序不会调用它；装了 git-gui 也不等于本程序能找到 ssh。\n\
             4. 从访达或程序坞启动时 PATH 不含 Homebrew。本程序会自动查找 /opt/homebrew/bin 与 /usr/local/bin；装完后请完全退出再打开。\n\
             5. 在终端执行 which ssh && ssh -V，应能看到 /usr/bin/ssh 或 Homebrew 的 ssh。"
        ),
        _ => format!(
            "未找到本机 {tool}（OpenSSH）。Linux 需要 openssh-client 与 git 命令行。\n\n\
             解决方法：\n\
             Debian/Ubuntu：sudo apt update && sudo apt install -y git openssh-client\n\
             Fedora：sudo dnf install -y git openssh-clients\n\
             Arch：sudo pacman -S --needed git openssh\n\
             装完后完全退出并重新打开本程序。"
        ),
    }
}

/// 未找到 `git` 命令行时的说明 + 解决方法（不是 git-gui）。
pub fn missing_git_cli() -> String {
    match host_os() {
        "windows" => "未找到 git 命令行。\n\n\
             解决方法：\n\
             1. 打开 https://git-scm.com/download/win 下载并安装 Git for Windows。\n\
             2. 装完后完全退出本程序再打开。\n\
             3. 可确认 C:\\Program Files\\Git\\cmd\\git.exe 存在。"
            .into(),
        "macos" => "未找到 git 命令行。本程序需要终端里的 git，不是 git-gui。\n\n\
             解决方法：\n\
             1. 打开「终端」执行：xcode-select --install\n\
             2. 或用 Homebrew：brew install git\n\
             3. 不要只装 git-gui；它不会提供本程序要调用的 git / ssh。\n\
             4. 从访达启动时 PATH 可能没有 /opt/homebrew/bin。装完后完全退出本程序再打开。"
            .into(),
        _ => "未找到 git 命令行。\n\n\
             解决方法：\n\
             Debian/Ubuntu：sudo apt install -y git\n\
             Fedora：sudo dnf install -y git\n\
             Arch：sudo pacman -S --needed git\n\
             装完后完全退出并重新打开本程序。"
            .into(),
    }
}

/// 已启动 agent 但 ssh-add 连不上。
pub fn agent_add_mismatch() -> String {
    match host_os() {
        "windows" => {
            "已启动 Git ssh-agent，但配套 ssh-add 仍连不上。请确认已安装 Git for Windows，并完全退出本程序后再试。\n\
             解决方法：安装 https://git-scm.com/download/win ，确认 C:\\Program Files\\Git\\usr\\bin\\ssh-add.exe 存在。"
                .into()
        }
        "macos" => {
            "已启动 ssh-agent，但配套 ssh-add 仍连不上。请确认本机 OpenSSH 完整，然后完全退出本程序再试。\n\
             解决方法：终端执行 xcode-select --install；或 brew install openssh。不需要 Git for Windows / git-gui。"
                .into()
        }
        _ => {
            "已启动 ssh-agent，但配套 ssh-add 仍连不上。请安装 openssh-client 后完全退出本程序再试。\n\
             Debian/Ubuntu：sudo apt install -y openssh-client"
                .into()
        }
    }
}

/// SSH 代理助手缺失（connect / ncat / nc）。
pub fn missing_ssh_proxy_helper() -> String {
    match host_os() {
        "windows" => {
            "未找到 SSH 代理助手（Git for Windows 的 connect，或 ncat）。HTTPS 仍可用，SSH 克隆/体检不会走代理。\n\
             解决方法：安装 Git for Windows（自带 connect.exe），或安装 Nmap 以获得 ncat。"
                .into()
        }
        "macos" => {
            "未找到 SSH 代理助手 ncat。HTTPS 仍可用，SSH 克隆/体检不会走代理。\n\
             解决方法：brew install nmap   （提供 ncat）。不需要 Git for Windows。"
                .into()
        }
        _ => {
            "未找到 SSH 代理助手（ncat 或 nc）。HTTPS 仍可用，SSH 克隆/体检不会走代理。\n\
             解决方法：sudo apt install -y nmap  或  sudo dnf install -y nmap-ncat"
                .into()
        }
    }
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

    #[test]
    fn host_os_matches_target() {
        let os = host_os();
        assert!(os == "windows" || os == "macos" || os == "linux");
        #[cfg(target_os = "windows")]
        assert_eq!(os, "windows");
        #[cfg(target_os = "macos")]
        assert_eq!(os, "macos");
    }

    #[test]
    fn preferred_source_is_git_only_on_windows() {
        #[cfg(windows)]
        assert_eq!(preferred_ssh_source(), "git");
        #[cfg(not(windows))]
        assert_eq!(preferred_ssh_source(), "system");
    }

    #[test]
    fn missing_ssh_tool_is_os_specific_and_actionable() {
        let msg = missing_ssh_tool("ssh");
        assert!(msg.contains("解决方法"));
        #[cfg(windows)]
        {
            assert!(msg.contains("Git for Windows"));
            assert!(msg.contains("git-scm.com/download/win"));
        }
        #[cfg(target_os = "macos")]
        {
            assert!(msg.contains("xcode-select --install"));
            assert!(msg.contains("git-gui"));
            assert!(!msg.contains("请先安装 Git for Windows。"));
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert!(msg.contains("apt") || msg.contains("dnf") || msg.contains("pacman"));
        }
    }

    #[test]
    fn missing_git_cli_mentions_cli_not_gui() {
        let msg = missing_git_cli();
        assert!(msg.contains("解决方法"));
        #[cfg(target_os = "macos")]
        {
            assert!(msg.contains("git-gui"));
            assert!(msg.contains("brew install git") || msg.contains("xcode-select"));
        }
    }

    #[test]
    fn windows_only_accepts_git_bundled_ssh() {
        #[cfg(windows)]
        {
            assert!(is_preferred_ssh_path(Path::new(
                r"C:\Program Files\Git\usr\bin\ssh.exe"
            )));
            assert!(!is_preferred_ssh_path(Path::new(
                r"C:\Windows\System32\OpenSSH\ssh.exe"
            )));
        }
        #[cfg(not(windows))]
        {
            assert!(is_preferred_ssh_path(Path::new("/usr/bin/ssh")));
            assert!(is_preferred_ssh_path(Path::new("/opt/homebrew/bin/ssh")));
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_candidates_are_system_not_git_for_windows() {
        for (src, path) in candidate_paths() {
            assert_eq!(src, "system");
            assert!(!path.to_string_lossy().contains("Program Files"));
        }
    }
}
