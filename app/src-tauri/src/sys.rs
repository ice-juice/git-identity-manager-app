//! 外部命令执行与路径解析辅助（平台相关操作的薄封装）。

use crate::error::{AppError, Result};
use std::path::PathBuf;
use std::process::Command;

/// 运行外部命令，返回 (stdout, stderr, exit_code)。stdout/stderr 按 UTF-8 有损解码。
pub fn run(exe: &str, args: &[&str]) -> Result<(String, String, i32)> {
    let output = Command::new(exe)
        .args(args)
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
    let mut child = Command::new(exe)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
