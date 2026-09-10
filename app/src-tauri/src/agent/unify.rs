//! 把当前用户环境统一到 Git 自带 ssh / ssh-agent。
//!
//! Windows 上系统 OpenSSH 与 Git/MSYS 的 agent 不能混用。本模块写入：
//! - `git config --global core.sshCommand`
//! - 用户环境变量 `GIT_SSH` / `SSH_AUTH_SOCK` / `SSH_AGENT_PID`
//! - PowerShell / Git Bash 启动脚本（会话 PATH 前置 Git `usr\bin`）
//!
//! 用户 PATH 不会改注册表（排在系统 PATH 之后也压不住 System32\OpenSSH）。
//! 交互终端靠 profile；`git` 靠 `GIT_SSH` 与 `core.sshCommand`。

use super::AgentEnv;
use crate::error::{AppError, Result};
use crate::sys;
use serde::Serialize;
use std::path::{Path, PathBuf};

const BEGIN: &str = "# >>> git-account-manager agent >>>";
const END: &str = "# <<< git-account-manager agent <<<";
const ENV_PS1: &str = "git-account-manager-agent.env.ps1";
const ENV_SH: &str = "git-account-manager-agent.env.sh";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvCheck {
    pub key: String,
    pub label: String,
    pub ok: bool,
    pub current: Option<String>,
    pub expected: Option<String>,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUnifyStatus {
    pub os: String,
    pub git_installed: bool,
    pub agent_running: bool,
    pub git_ssh: Option<String>,
    pub ssh_add: Option<String>,
    pub auth_sock: Option<String>,
    pub agent_pid: Option<String>,
    pub git_config_ok: bool,
    pub user_git_ssh_ok: bool,
    pub user_sock_ok: bool,
    pub powershell_profile_ok: bool,
    pub bash_profile_ok: bool,
    pub aligned: bool,
    pub git_config_value: Option<String>,
    pub user_git_ssh: Option<String>,
    pub user_auth_sock: Option<String>,
    pub checks: Vec<EnvCheck>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUnifyReport {
    pub git_ssh: String,
    pub ssh_add: String,
    pub auth_sock: String,
    pub agent_pid: Option<String>,
    pub git_config: bool,
    pub user_env: bool,
    pub powershell_profile: bool,
    pub bash_profile: bool,
    pub steps: Vec<String>,
    pub hint: String,
}

fn env_script_ps1() -> PathBuf {
    sys::ssh_dir().join(ENV_PS1)
}

fn env_script_sh() -> PathBuf {
    sys::ssh_dir().join(ENV_SH)
}

pub fn ssh_command_value(ssh: &str) -> String {
    let unix = ssh.replace('\\', "/");
    if unix.contains(' ') {
        format!("\"{unix}\"")
    } else {
        unix
    }
}

pub fn normalize_ssh_path(raw: &str) -> String {
    raw.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .replace('\\', "/")
        .to_ascii_lowercase()
}

pub fn upsert_marked_block(existing: &str, inner: &str) -> String {
    let block = format!("{BEGIN}\n{inner}\n{END}\n");
    if let Some(start) = existing.find(BEGIN) {
        if let Some(rel_end) = existing[start..].find(END) {
            let mut end = start + rel_end + END.len();
            let rest = &existing[end..];
            if rest.starts_with("\r\n") {
                end += 2;
            } else if rest.starts_with('\n') {
                end += 1;
            }
            let mut out = String::new();
            out.push_str(&existing[..start]);
            out.push_str(&block);
            out.push_str(&existing[end..]);
            return out;
        }
    }
    let mut s = existing.to_string();
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    if !s.is_empty() && !s.ends_with("\n\n") {
        s.push('\n');
    }
    s.push_str(&block);
    s
}

/// 去掉本程序写入的标记块，用于一键还原。
pub fn remove_marked_block(existing: &str) -> String {
    if let Some(start) = existing.find(BEGIN) {
        if let Some(rel_end) = existing[start..].find(END) {
            let mut end = start + rel_end + END.len();
            let rest = &existing[end..];
            if rest.starts_with("\r\n") {
                end += 2;
            } else if rest.starts_with('\n') {
                end += 1;
            }
            let mut out = String::new();
            out.push_str(&existing[..start]);
            out.push_str(&existing[end..]);
            return out.trim_start_matches(['\r', '\n']).to_string();
        }
    }
    existing.to_string()
}

fn profile_has_marker(text: &str) -> bool {
    text.contains(BEGIN) && text.contains(END)
}

fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    std::fs::write(path, text).map_err(|e| AppError::Io(format!("写入 {} 失败：{e}", path.display())))
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn documents_dirs() -> Vec<PathBuf> {
    let home = sys::home_dir();
    let mut dirs = vec![
        home.join("Documents"),
        home.join("OneDrive").join("Documents"),
        home.join("OneDrive").join("文档"),
        home.join("文档"),
    ];
    dirs.sort();
    dirs.dedup();
    dirs.into_iter().filter(|p| p.is_dir()).collect()
}

fn powershell_profile_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for docs in documents_dirs() {
        out.push(
            docs.join("WindowsPowerShell")
                .join("Microsoft.PowerShell_profile.ps1"),
        );
        out.push(docs.join("PowerShell").join("Microsoft.PowerShell_profile.ps1"));
    }
    if out.is_empty() {
        let docs = sys::home_dir().join("Documents");
        out.push(
            docs.join("WindowsPowerShell")
                .join("Microsoft.PowerShell_profile.ps1"),
        );
        out.push(docs.join("PowerShell").join("Microsoft.PowerShell_profile.ps1"));
    }
    out
}

fn bash_profile_paths() -> Vec<PathBuf> {
    let home = sys::home_dir();
    #[cfg(target_os = "macos")]
    {
        vec![
            home.join(".zshrc"),
            home.join(".zprofile"),
            home.join(".bashrc"),
            home.join(".bash_profile"),
        ]
    }
    #[cfg(not(target_os = "macos"))]
    {
        vec![home.join(".bashrc"), home.join(".bash_profile")]
    }
}

fn git_bin_dir(ssh: &str) -> Option<PathBuf> {
    Path::new(ssh).parent().map(|p| p.to_path_buf())
}

fn current_core_ssh_command() -> Option<String> {
    let (out, _, code) = sys::run_git(&["config", "--global", "--get", "core.sshCommand"]).ok()?;
    if code == 0 && !out.trim().is_empty() {
        Some(out.trim().to_string())
    } else {
        None
    }
}

fn set_core_ssh_command(ssh: &str) -> Result<()> {
    let value = ssh_command_value(ssh);
    let (out, err, code) = sys::run_git(&["config", "--global", "core.sshCommand", &value])?;
    if code != 0 {
        let detail = if !err.trim().is_empty() { err } else { out };
        return Err(AppError::Other(format!(
            "写入 git config --global core.sshCommand 失败：{}",
            detail.trim()
        )));
    }
    Ok(())
}

#[cfg(windows)]
fn user_env(name: &str) -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey("Environment").ok()?;
    key.get_value::<String, _>(name).ok().filter(|s| !s.trim().is_empty())
}

#[cfg(not(windows))]
fn user_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}

#[cfg(windows)]
fn set_user_env(name: &str, value: &str) -> Result<()> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey_with_flags("Environment", KEY_SET_VALUE)
        .or_else(|_| hkcu.create_subkey("Environment").map(|(k, _)| k))
        .map_err(|e| AppError::Io(format!("无法写入用户环境变量：{e}")))?;
    key.set_value(name, &value.to_string())
        .map_err(|e| AppError::Io(format!("写入用户环境变量 {name} 失败：{e}")))?;
    Ok(())
}

#[cfg(not(windows))]
fn set_user_env(_name: &str, _value: &str) -> Result<()> {
    Ok(())
}

#[cfg(windows)]
fn broadcast_env_change() {
    let script = "[Environment]::SetEnvironmentVariable('GIT_SSH',[Environment]::GetEnvironmentVariable('GIT_SSH','User'),'User')";
    let _ = sys::run(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    );
}

#[cfg(not(windows))]
fn broadcast_env_change() {}

fn write_env_scripts(env: &AgentEnv) -> Result<(PathBuf, PathBuf)> {
    let ssh = env
        .ssh
        .clone()
        .ok_or_else(|| AppError::Other("Git ssh 路径未知".into()))?;
    let sock = env
        .auth_sock
        .clone()
        .ok_or_else(|| AppError::Other("Git ssh-agent 套接字未知".into()))?;
    let git_bin = git_bin_dir(&ssh)
        .ok_or_else(|| AppError::Other("无法解析 ssh 所在目录".into()))?;
    let git_bin_s = git_bin.display().to_string();
    let git_bin_msys = crate::agent::to_msys_sock_path(&git_bin);
    let pid_line_ps = env
        .agent_pid
        .as_deref()
        .map(|p| format!("$env:SSH_AGENT_PID = '{p}'\n"))
        .unwrap_or_default();
    let pid_line_sh = env
        .agent_pid
        .as_deref()
        .map(|p| format!("export SSH_AGENT_PID='{p}'\n"))
        .unwrap_or_default();

    let ssh_ps = ssh.replace('\'', "''");
    let sock_ps = sock.replace('\'', "''");
    let bin_ps = git_bin_s.replace('\'', "''");
    let ps1 = format!(
        "$env:GIT_SSH = '{ssh_ps}'\n$env:SSH_AUTH_SOCK = '{sock_ps}'\n{pid_line_ps}$gamGitBin = '{bin_ps}'\nif (Test-Path -LiteralPath $gamGitBin) {{\n  $parts = @($env:PATH -split ';' | Where-Object {{ $_ }})\n  if ($parts -notcontains $gamGitBin) {{ $env:PATH = $gamGitBin + ';' + $env:PATH }}\n}}\n"
    );

    let ssh_sh = ssh.replace('\\', "/").replace('\'', "'\\''");
    let sock_sh = sock.replace('\'', "'\\''");
    let sh = format!(
        "export GIT_SSH='{ssh_sh}'\nexport SSH_AUTH_SOCK='{sock_sh}'\n{pid_line_sh}gam_git_bin='{git_bin_msys}'\ncase \":$PATH:\" in\n  *\":$gam_git_bin:\"*) ;;\n  *) export PATH=\"$gam_git_bin:$PATH\" ;;\nesac\n"
    );

    let ps_path = env_script_ps1();
    let sh_path = env_script_sh();
    write_text(&ps_path, &ps1)?;
    write_text(&sh_path, &sh)?;
    Ok((ps_path, sh_path))
}

fn hook_powershell_profiles(script: &Path) -> Result<bool> {
    let script_s = script.display().to_string().replace('\'', "''");
    let inner = format!(
        "if (Test-Path -LiteralPath '{script_s}') {{ . '{script_s}' }}"
    );
    let mut wrote = false;
    for path in powershell_profile_paths() {
        let next = upsert_marked_block(&read_text(&path), &inner);
        write_text(&path, &next)?;
        wrote = true;
    }
    Ok(wrote)
}

fn hook_bash_profiles(script: &Path) -> Result<bool> {
    let script_msys = crate::agent::to_msys_sock_path(script);
    let inner = format!("if [ -f '{script_msys}' ]; then . '{script_msys}'; fi");
    let paths = bash_profile_paths();
    let any_exist = paths.iter().any(|p| p.is_file());
    let targets: Vec<PathBuf> = if any_exist {
        paths.into_iter().filter(|p| p.is_file()).collect()
    } else if crate::ssh::toolchain::host_os() == "macos" {
        vec![sys::home_dir().join(".zshrc")]
    } else {
        vec![sys::home_dir().join(".bashrc")]
    };
    for path in &targets {
        let next = upsert_marked_block(&read_text(path), &inner);
        write_text(path, &next)?;
    }
    Ok(!targets.is_empty())
}

fn profiles_ok() -> (bool, bool) {
    let ps_ok = env_script_ps1().is_file()
        && powershell_profile_paths()
            .iter()
            .any(|p| p.is_file() && profile_has_marker(&read_text(p)));
    let sh_ok = env_script_sh().is_file()
        && bash_profile_paths()
            .iter()
            .any(|p| p.is_file() && profile_has_marker(&read_text(p)));
    (ps_ok, sh_ok)
}

fn paths_match(a: &str, b: &str) -> bool {
    normalize_ssh_path(a) == normalize_ssh_path(b)
}

fn env_check(
    key: &str,
    label: &str,
    ok: bool,
    current: Option<String>,
    expected: Option<String>,
    hint: Option<String>,
) -> EnvCheck {
    EnvCheck {
        key: key.into(),
        label: label.into(),
        ok,
        current,
        expected,
        hint: if ok { None } else { hint },
    }
}

/// 检查当前用户环境是否已对齐到这套 Git agent。
pub fn inspect(env: &AgentEnv) -> AgentUnifyStatus {
    let git_ssh = env.ssh.clone().or_else(|| {
        crate::agent::git_ssh_bin().map(|p| p.display().to_string())
    });
    let ssh_add = env.ssh_add.clone().or_else(|| {
        crate::agent::git_ssh_add_bin().map(|p| p.display().to_string())
    });
    let want_ssh = git_ssh.clone();
    let want_sock = env.auth_sock.clone();
    let git_config_value = current_core_ssh_command();
    let user_git_ssh = user_env("GIT_SSH");
    let user_auth_sock = user_env("SSH_AUTH_SOCK");
    let git_config_ok = match (&want_ssh, &git_config_value) {
        (Some(ssh), Some(cfg)) => paths_match(ssh, cfg),
        _ => false,
    };
    let user_git_ssh_ok = match (&want_ssh, &user_git_ssh) {
        (Some(ssh), Some(v)) => paths_match(ssh, v),
        _ => false,
    };
    let user_sock_ok = match (&want_sock, &user_auth_sock) {
        (Some(sock), Some(v)) => v.trim() == sock.trim(),
        _ => false,
    };
    let (powershell_profile_ok, bash_profile_ok) = profiles_ok();
    let git_installed = git_ssh.is_some();
    let agent_running = env.auth_sock.is_some() && crate::agent::is_ready(env);
    let os = crate::ssh::toolchain::host_os();
    let aligned = if os == "windows" {
        git_installed
            && agent_running
            && git_config_ok
            && user_git_ssh_ok
            && user_sock_ok
            && (powershell_profile_ok || bash_profile_ok)
    } else {
        git_installed && agent_running && bash_profile_ok
    };

    let ssh_label = crate::ssh::toolchain::ssh_tool_label();
    let shell_label = if os == "macos" {
        "终端启动脚本（zsh / bash）"
    } else if os == "windows" {
        "Git Bash 启动脚本"
    } else {
        "终端启动脚本（bash）"
    };

    let mut checks = vec![
        env_check(
            "gitSsh",
            ssh_label,
            git_installed,
            git_ssh.clone(),
            None,
            Some(crate::ssh::toolchain::missing_ssh_tool("ssh")),
        ),
        env_check(
            "agent",
            if os == "windows" { "Git ssh-agent" } else { "本机 ssh-agent" },
            agent_running,
            env.auth_sock.clone(),
            want_sock.clone(),
            Some("请点「确保运行」。若刚装完 OpenSSH / git，请完全退出本程序再打开。".into()),
        ),
    ];
    if os == "windows" {
        checks.push(env_check(
            "gitConfig",
            "git core.sshCommand",
            git_config_ok,
            git_config_value.clone(),
            want_ssh.as_ref().map(|s| ssh_command_value(s)),
            Some("点「应用到本机环境」写入 git config。若提示找不到 git，请先安装 Git for Windows。".into()),
        ));
        checks.push(env_check(
            "userGitSsh",
            "用户环境 GIT_SSH",
            user_git_ssh_ok,
            user_git_ssh.clone(),
            want_ssh.clone(),
            Some("点「应用到本机环境」写入当前用户环境变量 GIT_SSH。".into()),
        ));
        checks.push(env_check(
            "userSock",
            "用户环境 SSH_AUTH_SOCK",
            user_sock_ok,
            user_auth_sock.clone(),
            want_sock.clone(),
            Some("点「应用到本机环境」写入当前用户环境变量 SSH_AUTH_SOCK。".into()),
        ));
        checks.push(env_check(
            "powershell",
            "PowerShell 启动脚本",
            powershell_profile_ok,
            Some(if powershell_profile_ok {
                "已挂钩 Git ssh-agent".into()
            } else {
                "未挂钩".into()
            }),
            Some("新开终端自动使用 Git ssh / agent".into()),
            Some("点「应用到本机环境」挂钩 PowerShell $PROFILE。已打开的终端需新开窗口。".into()),
        ));
    }
    checks.push(env_check(
        "bash",
        shell_label,
        bash_profile_ok,
        Some(if bash_profile_ok {
            "已挂钩 ssh-agent".into()
        } else {
            "未挂钩".into()
        }),
        Some("新开终端自动使用同一套 ssh / agent".into()),
        Some(if os == "macos" {
            "点「应用到本机环境」挂钩 ~/.zshrc。已打开的终端需新开窗口。".into()
        } else {
            "点「应用到本机环境」挂钩 ~/.bashrc。已打开的终端需新开窗口。".into()
        }),
    ));

    AgentUnifyStatus {
        os: os.into(),
        git_installed,
        agent_running,
        git_ssh,
        ssh_add,
        auth_sock: env.auth_sock.clone(),
        agent_pid: env.agent_pid.clone(),
        git_config_ok,
        user_git_ssh_ok,
        user_sock_ok,
        powershell_profile_ok,
        bash_profile_ok,
        aligned,
        git_config_value,
        user_git_ssh,
        user_auth_sock,
        checks,
    }
}

/// 把 Git ssh 与当前 agent 套接字写入本机用户环境（可重复执行）。
/// 调用方必须先取得用户二次确认；IPC 入口在 `confirmed != true` 时会拒绝。
pub fn apply(env: &AgentEnv) -> Result<AgentUnifyReport> {
    let ssh = env
        .ssh
        .clone()
        .or_else(|| crate::agent::git_ssh_bin().map(|p| p.display().to_string()))
        .ok_or_else(|| AppError::Other(crate::ssh::toolchain::missing_ssh_tool("ssh")))?;
    let ssh_add = env
        .ssh_add
        .clone()
        .or_else(|| crate::agent::git_ssh_add_bin().map(|p| p.display().to_string()))
        .unwrap_or_else(|| "ssh-add".into());
    let sock = env
        .auth_sock
        .clone()
        .ok_or_else(|| AppError::Other("Git ssh-agent 尚未运行，请先点「确保运行」。".into()))?;

    let mut steps = Vec::new();
    let mut working = env.clone();
    working.ssh = Some(ssh.clone());
    working.ssh_add = Some(ssh_add.clone());

    set_core_ssh_command(&ssh)?;
    steps.push(format!("git config --global core.sshCommand {}", ssh_command_value(&ssh)));

    let mut user_env_ok = true;
    if let Err(e) = set_user_env("GIT_SSH", &ssh) {
        user_env_ok = false;
        steps.push(format!("用户环境 GIT_SSH 未写入：{e}"));
    } else {
        steps.push(format!("用户环境 GIT_SSH={ssh}"));
    }
    if let Err(e) = set_user_env("SSH_AUTH_SOCK", &sock) {
        user_env_ok = false;
        steps.push(format!("用户环境 SSH_AUTH_SOCK 未写入：{e}"));
    } else {
        steps.push(format!("用户环境 SSH_AUTH_SOCK={sock}"));
    }
    if let Some(pid) = &working.agent_pid {
        match set_user_env("SSH_AGENT_PID", pid) {
            Ok(()) => steps.push(format!("用户环境 SSH_AGENT_PID={pid}")),
            Err(e) => {
                user_env_ok = false;
                steps.push(format!("用户环境 SSH_AGENT_PID 未写入：{e}"));
            }
        }
    }
    if user_env_ok {
        broadcast_env_change();
    }

    let (ps_script, sh_script) = write_env_scripts(&working)?;
    steps.push(format!("已更新 {}", ps_script.display()));
    steps.push(format!("已更新 {}", sh_script.display()));

    let powershell_profile = hook_powershell_profiles(&ps_script)?;
    if powershell_profile {
        steps.push("已挂钩 PowerShell $PROFILE".into());
    }
    let bash_profile = hook_bash_profiles(&sh_script)?;
    if bash_profile {
        steps.push(if crate::ssh::toolchain::host_os() == "macos" {
            "已挂钩 ~/.zshrc".into()
        } else {
            "已挂钩 ~/.bashrc".into()
        });
    }

    let hint = match crate::ssh::toolchain::host_os() {
        "windows" => "新开的 PowerShell / Git Bash 会自动带上这套 Git ssh-agent。已经打开的 Cursor 窗口请新开终端；只改了用户环境变量的程序需要重启后才会读到。",
        "macos" => "新开的终端（zsh）会自动带上这套 ssh-agent。已经打开的终端或 Cursor 窗口请新开一页；从访达启动的本程序已能直接使用本机 OpenSSH。",
        _ => "新开的终端会自动带上这套 ssh-agent。已经打开的终端请新开窗口。",
    };

    Ok(AgentUnifyReport {
        git_ssh: ssh,
        ssh_add,
        auth_sock: sock,
        agent_pid: working.agent_pid,
        git_config: true,
        user_env: user_env_ok,
        powershell_profile,
        bash_profile,
        steps,
        hint: hint.into(),
    })
}

#[cfg(windows)]
fn delete_user_env(name: &str) -> Result<()> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags("Environment", KEY_SET_VALUE) {
        let _ = key.delete_value(name);
    }
    Ok(())
}

#[cfg(not(windows))]
fn delete_user_env(_name: &str) -> Result<()> {
    Ok(())
}

fn unset_core_ssh_command() -> Result<()> {
    let (_o, _e, code) = sys::run_git(&["config", "--global", "--unset", "core.sshCommand"])?;
    if code != 0 && code != 5 {
        // 5 = 该项不存在
        log::warn!("unset core.sshCommand 返回 {code}");
    }
    Ok(())
}

/// 撤销 `apply` 对用户环境、git config 和启动脚本的改动。
pub fn revert() -> Result<Vec<String>> {
    let mut steps = Vec::new();
    let _ = unset_core_ssh_command();
    steps.push("已清除 git config --global core.sshCommand".into());
    for name in ["GIT_SSH", "SSH_AUTH_SOCK", "SSH_AGENT_PID"] {
        let _ = delete_user_env(name);
        steps.push(format!("已删除用户环境变量 {name}"));
    }
    broadcast_env_change();
    for path in powershell_profile_paths() {
        if path.is_file() {
            let next = remove_marked_block(&read_text(&path));
            write_text(&path, &next)?;
            steps.push(format!("已还原 {}", path.display()));
        }
    }
    for path in bash_profile_paths() {
        if path.is_file() {
            let next = remove_marked_block(&read_text(&path));
            write_text(&path, &next)?;
            steps.push(format!("已还原 {}", path.display()));
        }
    }
    for script in [env_script_ps1(), env_script_sh()] {
        if script.is_file() {
            let _ = std::fs::remove_file(&script);
            steps.push(format!("已删除 {}", script.display()));
        }
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_ssh_path_with_spaces() {
        assert_eq!(
            ssh_command_value(r"C:\Program Files\Git\usr\bin\ssh.exe"),
            "\"C:/Program Files/Git/usr/bin/ssh.exe\""
        );
        assert_eq!(ssh_command_value("/usr/bin/ssh"), "/usr/bin/ssh");
    }

    #[test]
    fn normalize_ignores_slash_and_quotes() {
        assert_eq!(
            normalize_ssh_path(r#""C:\Program Files\Git\usr\bin\ssh.exe""#),
            "c:/program files/git/usr/bin/ssh.exe"
        );
        assert!(paths_match(
            r"C:\Program Files\Git\usr\bin\ssh.exe",
            "\"C:/Program Files/Git/usr/bin/ssh.exe\""
        ));
    }

    #[test]
    fn upsert_inserts_and_replaces_block() {
        let first = upsert_marked_block("echo hi\n", "export A=1");
        assert!(first.contains(BEGIN));
        assert!(first.contains("export A=1"));
        let second = upsert_marked_block(&first, "export A=2");
        assert!(second.contains("export A=2"));
        assert!(!second.contains("export A=1"));
        assert_eq!(second.matches(BEGIN).count(), 1);
    }

    #[test]
    fn remove_marked_block_restores_user_text() {
        let with = upsert_marked_block("echo hi\n", "export A=1");
        let out = remove_marked_block(&with);
        assert!(out.contains("echo hi"));
        assert!(!out.contains(BEGIN));
        assert!(!out.contains("export A=1"));
    }
}
