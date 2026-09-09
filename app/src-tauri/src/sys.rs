//! 外部命令执行与路径解析辅助（平台相关操作的薄封装）。

use crate::error::{AppError, Result};
use std::path::PathBuf;
use std::process::Command;

/// GUI 进程里隐藏子进程控制台窗口，避免 `ssh-add`/`sc` 闪黑框。
pub fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

/// 运行外部命令，返回 (stdout, stderr, exit_code)。stdout/stderr 按 UTF-8 有损解码。
pub fn run(exe: &str, args: &[&str]) -> Result<(String, String, i32)> {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    hide_console(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| AppError::Io(format!("执行 {exe} 失败：{e}")))?;
    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    ))
}

/// 运行命令并把内容写到 stdin。
pub fn run_with_stdin(exe: &str, args: &[&str], stdin_data: &[u8]) -> Result<(String, String, i32)> {
    use std::io::Write;
    use std::process::Stdio;
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Io(format!("执行 {exe} 失败：{e}")))?;
    if let Some(mut si) = child.stdin.take() {
        si.write_all(stdin_data)
            .map_err(|e| AppError::Io(e.to_string()))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| AppError::Io(e.to_string()))?;
    Ok((
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    ))
}

/// 用户主目录。
pub fn home_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// `~/.ssh` 目录。
pub fn ssh_dir() -> PathBuf {
    home_dir().join(".ssh")
}

/// 本机 OpenSSH 入口：`~/.ssh/config`（迁入工作空间后只保留 Include）。
pub fn home_ssh_config() -> PathBuf {
    ssh_dir().join("config")
}

/// `~/.ssh` 下的配置镜像文件名。相对 Include 对 Windows OpenSSH 和 Git/MSYS ssh 都有效。
pub const HOME_INCLUDED_CONFIG_NAME: &str = "git-account-manager.config";

/// 本机 SSH 实际 Include 的镜像（与工作空间正本同步）。
pub fn home_included_config() -> PathBuf {
    ssh_dir().join(HOME_INCLUDED_CONFIG_NAME)
}

/// 工作空间内的 SSH config 正本，随身份数据一起同步。
pub fn workspace_ssh_config(workspace: &std::path::Path) -> PathBuf {
    workspace.join("ssh").join("config")
}

fn ssh_path_for_include(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalize_include_target(raw: &str) -> String {
    raw.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn include_line_targets_workspace(line: &str, workspace_config: &std::path::Path) -> bool {
    let t = line.trim();
    let Some(rest) = t
        .strip_prefix("Include ")
        .or_else(|| t.strip_prefix("include "))
    else {
        return false;
    };
    let target = normalize_include_target(rest);
    target == HOME_INCLUDED_CONFIG_NAME
        || target == normalize_include_target(&ssh_path_for_include(&home_included_config()))
        || target == normalize_include_target(&ssh_path_for_include(workspace_config))
}

/// 文本是否只有注释和 Include（正本被写成入口 stub 时也用这个判断）。
pub fn text_is_include_only(text: &str) -> bool {
    let mut saw_include = false;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.to_ascii_lowercase().starts_with("include ") {
            saw_include = true;
            continue;
        }
        return false;
    }
    saw_include
}

pub fn text_has_host_blocks(text: &str) -> bool {
    text.lines().any(|line| {
        let t = line.trim();
        t.len() >= 5 && t[..5].eq_ignore_ascii_case("host ")
    })
}

/// `~/.ssh/config` 是否已经只是指向工作空间正本的 Include 入口。
pub fn is_system_include_stub(text: &str, workspace_config: &std::path::Path) -> bool {
    let mut saw = false;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if include_line_targets_workspace(t, workspace_config) {
            saw = true;
            continue;
        }
        return false;
    }
    saw
}

fn home_config_body_to_adopt(home_text: &str, workspace_config: &std::path::Path) -> String {
    if is_system_include_stub(home_text, workspace_config) {
        return String::new();
    }
    let mut out = String::new();
    for line in home_text.lines() {
        if include_line_targets_workspace(line, workspace_config) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn file_newer(a: &std::path::Path, b: &std::path::Path) -> bool {
    let ta = a.metadata().and_then(|m| m.modified()).ok();
    let tb = b.metadata().and_then(|m| m.modified()).ok();
    match (ta, tb) {
        (Some(x), Some(y)) => x > y,
        (Some(_), None) => true,
        _ => false,
    }
}

fn home_include_stub() -> String {
    format!(
        "# git-account-manager：真实 SSH 配置在工作空间，请勿在此文件编写 Host。\nInclude {HOME_INCLUDED_CONFIG_NAME}\n"
    )
}

fn write_home_include_mirror(text: &str) -> Result<()> {
    let dest = home_included_config();
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::vault::atomic_write(&dest, text.as_bytes())?;
    let _ = tighten_user_acl(&dest);
    Ok(())
}

fn write_system_include_stub(workspace_config: &std::path::Path) -> Result<()> {
    let home = home_ssh_config();
    if let Some(parent) = home.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let stub = home_include_stub();
    let current = std::fs::read_to_string(&home).unwrap_or_default();
    if current == stub {
        return Ok(());
    }
    // 旧版绝对路径 Include 也是我们的入口，直接覆盖，避免把 stub 再备份一份。
    if !current.trim().is_empty() && !is_system_include_stub(&current, workspace_config) {
        let _ = crate::util::backup_file(&home);
    }
    crate::vault::atomic_write(&home, stub.as_bytes())
}

/// 把工作空间正本镜像到 `~/.ssh/git-account-manager.config`，并改写系统入口为相对 Include。
pub fn ensure_home_ssh_bridge(workspace: &std::path::Path) -> Result<()> {
    let dest = workspace_ssh_config(workspace);
    let text = std::fs::read_to_string(&dest).unwrap_or_default();
    write_home_include_mirror(&text)?;
    write_system_include_stub(&dest)?;
    let _ = tighten_user_acl(&dest);
    Ok(())
}

/// 把本机 `~/.ssh/config` 迁入工作空间，并把系统入口改写成 Include。
pub fn adopt_ssh_config(workspace: &std::path::Path) -> Result<std::path::PathBuf> {
    let dest = workspace_ssh_config(workspace);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let home = home_ssh_config();
    let home_text = std::fs::read_to_string(&home).unwrap_or_default();
    let dest_text = std::fs::read_to_string(&dest).unwrap_or_default();
    let home_body = home_config_body_to_adopt(&home_text, &dest);

    // 正本里已有 Host 时绝不拿系统入口覆盖，避免 Include stub 把身份 Host 冲掉。
    let should_copy_home = !text_has_host_blocks(&dest_text)
        && !home_body.trim().is_empty()
        && (dest_text.trim().is_empty()
            || text_is_include_only(&dest_text)
            || dest_text == home_body
            || (!is_system_include_stub(&home_text, &dest) && file_newer(&home, &dest)));

    if should_copy_home {
        crate::vault::atomic_write(&dest, home_body.as_bytes())?;
    } else if !dest.exists() {
        crate::vault::atomic_write(
            &dest,
            "# git-account-manager SSH config\n# Host blocks are written when you create an identity.\n".as_bytes(),
        )?;
    }

    write_system_include_stub(&dest)?;
    let dest_text = std::fs::read_to_string(&dest).unwrap_or_default();
    let _ = write_home_include_mirror(&dest_text);
    let _ = tighten_user_acl(&dest);
    Ok(dest)
}

/// 写入工作空间正本，并确保 `~/.ssh/config` 以 Include 指向它。
pub fn persist_ssh_config(workspace: Option<&std::path::Path>, text: &str) -> Result<()> {
    let Some(ws) = workspace else {
        let home = home_ssh_config();
        if let Some(parent) = home.parent() {
            std::fs::create_dir_all(parent)?;
        }
        return crate::vault::atomic_write(&home, text.as_bytes());
    };
    let dest = workspace_ssh_config(ws);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::vault::atomic_write(&dest, text.as_bytes())?;
    let _ = write_home_include_mirror(text);
    write_system_include_stub(&dest)?;
    let _ = tighten_user_acl(&dest);
    Ok(())
}

/// Windows OpenSSH 要求被 Include 的 config 不能被其他用户读取。
fn tighten_user_acl(path: &std::path::Path) -> Result<()> {
    #[cfg(windows)]
    {
        let p = path.to_string_lossy().to_string();
        let user = std::env::var("USERNAME").unwrap_or_default();
        if user.is_empty() {
            return Ok(());
        }
        let grant = format!("{user}:F");
        let (_o, e, code) = run("icacls", &[&p, "/inheritance:r", "/grant:r", &grant])?;
        if code != 0 {
            return Err(AppError::Io(format!("收紧 SSH config 权限失败：{e}")));
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
    Ok(())
}

/// 只读工作空间正本，不跑迁移、不改系统入口。
pub fn read_workspace_ssh_config(workspace: &std::path::Path) -> (std::path::PathBuf, String) {
    let dest = workspace_ssh_config(workspace);
    let text = std::fs::read_to_string(&dest).unwrap_or_default();
    (dest, text)
}

/// 读取工作空间正本。无工作空间时读本机文件。
pub fn read_canonical_ssh_config(workspace: Option<&std::path::Path>) -> (std::path::PathBuf, String) {
    if let Some(ws) = workspace {
        return read_workspace_ssh_config(ws);
    }
    let home = home_ssh_config();
    let text = std::fs::read_to_string(&home).unwrap_or_default();
    (home, text)
}

/// 工作空间内供 OpenSSH 使用的密钥目录（与 vault 的 `keys/*.enc` 加密副本分开）。
pub fn workspace_ssh_keys_dir(workspace: &std::path::Path) -> PathBuf {
    workspace.join("ssh-keys")
}

/// 把备注名收成安全文件名主干：`id_ed25519_<name>`。
pub fn key_file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim();
    if cleaned.is_empty() {
        "id_ed25519".into()
    } else if cleaned.starts_with("id_ed25519") {
        cleaned.to_string()
    } else {
        format!("id_ed25519_{cleaned}")
    }
}

/// SSH config 的 IdentityFile 值：绝对路径、正斜杠；含空格时加引号。
pub fn identity_file_for_ssh(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.chars().any(|c| c.is_whitespace()) {
        format!("\"{s}\"")
    } else {
        s
    }
}

/// 解析 config 里的路径：展开开头的 `~`。
pub fn expand_path(p: &str) -> PathBuf {
    let trimmed = p.trim().trim_matches('"');
    if let Some(rest) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\")) {
        return home_dir().join(rest);
    }
    if trimmed == "~" {
        return home_dir();
    }
    PathBuf::from(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn key_stem_sanitizes_and_prefixes() {
        assert_eq!(key_file_stem("mgcp-afk"), "id_ed25519_mgcp-afk");
        assert_eq!(key_file_stem("id_ed25519_work"), "id_ed25519_work");
        assert_eq!(key_file_stem("a/b:c"), "id_ed25519_a_b_c");
    }

    #[test]
    fn identity_file_uses_slashes_and_quotes() {
        assert_eq!(
            identity_file_for_ssh(Path::new(r"D:\gitIdentifyData\ssh-keys\id_ed25519_a")),
            "D:/gitIdentifyData/ssh-keys/id_ed25519_a"
        );
        assert_eq!(
            identity_file_for_ssh(Path::new(r"D:\my keys\id_ed25519_a")),
            "\"D:/my keys/id_ed25519_a\""
        );
    }

    #[test]
    fn include_stub_detects_workspace_pointer() {
        let dest = Path::new(r"D:\gitIdentifyData\ssh\config");
        let stub = "Include D:/gitIdentifyData/ssh/config\n";
        assert!(is_system_include_stub(stub, dest));
        assert!(is_system_include_stub(
            "# comment\nInclude \"D:\\gitIdentifyData\\ssh\\config\"\n",
            dest
        ));
        assert!(!is_system_include_stub(
            "Include D:/gitIdentifyData/ssh/config\nHost x\n    HostName github.com\n",
            dest
        ));
        let body = home_config_body_to_adopt(
            "Include D:/gitIdentifyData/ssh/config\nHost keep\n    HostName x\n",
            dest,
        );
        assert!(body.contains("Host keep"));
        assert!(!body.to_ascii_lowercase().contains("include "));
        assert!(text_is_include_only(
            "# git-account-manager: real SSH config lives in the workspace.\nInclude D:/gitIdentifyData/ssh/config\n"
        ));
        assert!(text_has_host_blocks("Host github-a\n    HostName github.com\n"));
        assert!(!text_has_host_blocks("Include D:/x\n"));
        assert!(is_system_include_stub(
            "# c\nInclude git-account-manager.config\n",
            dest
        ));
    }
}
