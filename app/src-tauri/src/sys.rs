//! 外部命令执行与路径解析辅助（平台相关操作的薄封装）。

use crate::error::{AppError, Result};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

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
    run_timeout(exe, args, Duration::from_secs(30))
}

/// 带超时的外部命令。超时后杀掉进程树，避免 ssh-agent / ssh-add 挂死调用方。
pub fn run_timeout(exe: &str, args: &[&str], timeout: Duration) -> Result<(String, String, i32)> {
    run_timeout_with_env(exe, args, timeout, &[])
}

/// 与 [`run_timeout`] 相同，额外注入环境变量（如 `ssh-agent -k` 需要 PID）。
pub fn run_timeout_with_env(
    exe: &str,
    args: &[&str],
    timeout: Duration,
    extra_env: &[(&str, &str)],
) -> Result<(String, String, i32)> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    hide_console(&mut cmd);
    let child = cmd
        .spawn()
        .map_err(|e| AppError::Io(format!("执行 {exe} 失败：{e}")))?;
    wait_output_timeout(child, timeout, exe)
}

/// 按 Windows/本机 PID 杀掉进程树。MSYS 的 `SSH_AGENT_PID` 不能直接拿来用。
pub fn kill_pid(pid: u32) {
    kill_process_tree(pid);
}

/// 等待已启动子进程结束；超时则杀掉进程树。
pub fn wait_output_timeout(
    child: Child,
    timeout: Duration,
    label: &str,
) -> Result<(String, String, i32)> {
    let pid = child.id();
    let finished = Arc::new(AtomicBool::new(false));
    let flag = finished.clone();
    std::thread::spawn(move || {
        std::thread::sleep(timeout);
        if !flag.load(Ordering::SeqCst) {
            kill_process_tree(pid);
        }
    });
    let output = child
        .wait_with_output()
        .map_err(|e| AppError::Io(format!("等待 {label} 失败：{e}")))?;
    finished.store(true, Ordering::SeqCst);
    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    ))
}

fn kill_process_tree(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(windows)]
    {
        let mut cmd = Command::new("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
        hide_console(&mut cmd);
        let _ = cmd.output();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
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

/// 从访达 / 开始菜单启动时 PATH 往往不含 Homebrew、Git for Windows。
/// 把常见可执行目录补进当前进程，子进程（git / ssh）才能找到。
pub fn augment_search_path() {
    let extras: Vec<PathBuf> = common_bin_dirs().into_iter().filter(|p| p.is_dir()).collect();
    if extras.is_empty() {
        return;
    }
    let mut parts: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for dir in extras.into_iter().rev() {
        if !parts.iter().any(|p| paths_equal_for_path(p, &dir)) {
            parts.insert(0, dir);
        }
    }
    if let Ok(joined) = std::env::join_paths(parts) {
        std::env::set_var("PATH", joined);
    }
}

fn paths_equal_for_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

/// 各平台常见的 git / ssh 安装目录（不一定已存在）。
pub fn common_bin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        for root in [
            r"C:\Program Files\Git",
            r"C:\Program Files (x86)\Git",
        ] {
            dirs.push(PathBuf::from(root).join("cmd"));
            dirs.push(PathBuf::from(root).join("bin"));
            dirs.push(PathBuf::from(root).join(r"usr\bin"));
            dirs.push(PathBuf::from(root).join(r"mingw64\bin"));
        }
    }
    #[cfg(not(windows))]
    {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/opt/homebrew/sbin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/opt/local/bin"));
        dirs.push(PathBuf::from("/home/linuxbrew/.linuxbrew/bin"));
        dirs.push(home_dir().join(".local/bin"));
    }
    dirs
}

/// 在 PATH 与常见安装目录中查找可执行文件。
pub fn resolve_bin(name: &str) -> Option<PathBuf> {
    let as_path = std::path::Path::new(name);
    if as_path.is_absolute() && as_path.is_file() {
        return Some(as_path.to_path_buf());
    }
    let mut names = vec![name.to_string()];
    #[cfg(windows)]
    {
        if !name.to_ascii_lowercase().ends_with(".exe") {
            names.insert(0, format!("{name}.exe"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for n in &names {
                let p = dir.join(n);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    for dir in common_bin_dirs() {
        for n in &names {
            let p = dir.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// 本机 `git` 可执行文件。GUI 启动时 PATH 可能没有 Homebrew / Git for Windows。
pub fn git_exe() -> Result<String> {
    resolve_bin("git")
        .map(|p| p.display().to_string())
        .ok_or_else(|| AppError::Other(crate::ssh::toolchain::missing_git_cli()))
}

/// 用解析到的 git 跑命令，避免只写 `"git"` 时从访达启动找不到。
pub fn run_git(args: &[&str]) -> Result<(String, String, i32)> {
    run(&git_exe()?, args)
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
pub const HOME_INCLUDED_CONFIG_NAME: &str = crate::identity::SSH_INCLUDE;

/// 本机 SSH 实际 Include 的镜像（与工作空间正本同步）。
pub fn home_included_config() -> PathBuf {
    ssh_dir().join(HOME_INCLUDED_CONFIG_NAME)
}

fn legacy_home_included_config() -> PathBuf {
    ssh_dir().join(crate::identity::LEGACY_SSH_INCLUDE)
}

/// 把旧镜像拷到新文件名，并把只含 Include 的系统入口改写成新 stub。旧文件不删。
pub fn migrate_legacy_ssh_names() {
    let old = legacy_home_included_config();
    let new = home_included_config();
    crate::identity::copy_file_if_missing(&old, &new);

    let home = home_ssh_config();
    let Ok(current) = std::fs::read_to_string(&home) else {
        return;
    };
    if !is_system_include_stub(&current, std::path::Path::new("")) {
        return;
    }
    let stub = home_include_stub();
    if current == stub {
        return;
    }
    if let Err(e) = crate::vault::atomic_write(&home, stub.as_bytes()) {
        log::warn!("改写 ~/.ssh/config Include 入口失败：{e}");
    } else {
        log::info!("已将 ~/.ssh/config 的 Include 入口改为 {}", HOME_INCLUDED_CONFIG_NAME);
    }
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
        || target == crate::identity::LEGACY_SSH_INCLUDE
        || target == normalize_include_target(&ssh_path_for_include(&home_included_config()))
        || target
            == normalize_include_target(&ssh_path_for_include(&legacy_home_included_config()))
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
        "# git-keymaster：真实 SSH 配置在工作空间，请勿在此文件编写 Host。\nInclude {HOME_INCLUDED_CONFIG_NAME}\n"
    )
}

fn write_home_include_mirror(text: &str) -> Result<()> {
    let dest = home_included_config();
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::vault::atomic_write(&dest, text.as_bytes())?;
    // 只有被 ~/.ssh/config Include 的镜像需要收紧 ACL；工作空间正本不要动。
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

fn newest_sibling_backup(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let name = path.file_name()?.to_str()?;
    let dir = path.parent()?;
    let prefix = format!("{name}.bak.");
    let mut best: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    let entries = std::fs::read_dir(dir).ok()?;
    for ent in entries.flatten() {
        let fname = ent.file_name();
        let s = fname.to_string_lossy();
        if !s.starts_with(&prefix) {
            continue;
        }
        let t = ent.metadata().ok()?.modified().ok()?;
        if best.as_ref().map(|(bt, _)| t > *bt).unwrap_or(true) {
            best = Some((t, ent.path()));
        }
    }
    best.map(|(_, p)| p)
}

/// 撤销本程序对 `~/.ssh` 的改写：去掉 Include 入口与镜像，尽量恢复备份。
pub fn revert_home_ssh_bridge() -> Result<Vec<String>> {
    let mut steps = Vec::new();
    for included in [home_included_config(), legacy_home_included_config()] {
        if included.exists() {
            let _ = std::fs::remove_file(&included);
            steps.push(format!("已删除 {}", included.display()));
        }
    }
    let home = home_ssh_config();
    let current = std::fs::read_to_string(&home).unwrap_or_default();
    let dummy = std::path::Path::new("");
    let ours = is_system_include_stub(&current, dummy)
        || text_is_include_only(&current)
        || current.contains(HOME_INCLUDED_CONFIG_NAME)
        || current.contains(crate::identity::LEGACY_SSH_INCLUDE);
    if ours {
        if let Some(bak) = newest_sibling_backup(&home) {
            std::fs::copy(&bak, &home)?;
            steps.push(format!("已从备份恢复 {} ← {}", home.display(), bak.display()));
        } else {
            crate::vault::atomic_write(&home, b"# SSH config\n")?;
            steps.push("未找到 ~/.ssh/config 备份，已清空本程序写入的 Include 入口".into());
        }
    } else if !current.trim().is_empty() {
        let body = home_config_body_to_adopt(&current, dummy);
        if body != current {
            crate::vault::atomic_write(&home, body.as_bytes())?;
            steps.push("已从 ~/.ssh/config 去掉本程序的 Include".into());
        }
    }
    Ok(steps)
}

/// 把工作空间正本镜像到 `~/.ssh/git-keymaster.config`，并改写系统入口为相对 Include。
pub fn ensure_home_ssh_bridge(workspace: &std::path::Path) -> Result<()> {
    let dest = workspace_ssh_config(workspace);
    let text = std::fs::read_to_string(&dest).unwrap_or_default();
    let text = localize_ssh_config(&text, workspace);
    if let Err(e) = write_home_include_mirror(&text) {
        log::warn!("写入 ~/.ssh 镜像失败：{e}");
    }
    if let Err(e) = write_system_include_stub(&dest) {
        log::warn!("写入 ~/.ssh/config Include 入口失败：{e}");
    }
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
            "# git-keymaster SSH config\n# Host blocks are written when you create an identity.\n".as_bytes(),
        )?;
    }

    if let Err(e) = write_system_include_stub(&dest) {
        log::warn!("写入 ~/.ssh/config Include 入口失败：{e}");
    }
    let dest_text = std::fs::read_to_string(&dest).unwrap_or_default();
    let localized = localize_ssh_config(&dest_text, workspace);
    if localized != dest_text {
        crate::vault::atomic_write(&dest, localized.as_bytes())?;
    }
    let _ = write_home_include_mirror(&localized);
    Ok(dest)
}

/// 写入工作空间正本（本机展开绝对路径），并确保 `~/.ssh/config` 以 Include 指向它。
/// 这里写出的文本只给本机 OpenSSH 用，上传前必须再收成 `canonical_ssh_for_sync`。
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
    let text = localize_ssh_config(text, ws);
    // 换机后若上次 icacls 把正本收成不可写，先把当前用户加回去，避免整段同步失败。
    ensure_current_user_can_write(&dest);
    crate::vault::atomic_write(&dest, text.as_bytes())?;
    // ~/.ssh 受 Controlled Folder Access / 只读/占用时经常 Error 5。
    // 正本已在工作空间，系统入口失败不得打断云端拉取或按身份重建 Host。
    if let Err(e) = write_home_include_mirror(&text) {
        log::warn!("写入 ~/.ssh 镜像失败（工作空间正本已保存）：{e}");
    }
    if let Err(e) = write_system_include_stub(&dest) {
        log::warn!("写入 ~/.ssh/config Include 入口失败（工作空间正本已保存）：{e}");
    }
    Ok(())
}

/// 只追加当前用户写权限，不关闭继承。用于修复被误收紧的工作空间文件。
fn ensure_current_user_can_write(path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    #[cfg(windows)]
    {
        let p = path.to_string_lossy().to_string();
        let user = std::env::var("USERNAME").unwrap_or_default();
        if user.is_empty() {
            return;
        }
        let grant = format!("{user}:F");
        let _ = run("icacls", &[&p, "/grant:r", &grant]);
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
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
/// 文件不存在视为空；权限不足等真实读失败会带上路径，避免界面误显示成「空配置」。
pub fn read_workspace_ssh_config(workspace: &std::path::Path) -> Result<(std::path::PathBuf, String)> {
    let dest = workspace_ssh_config(workspace);
    match std::fs::read_to_string(&dest) {
        Ok(text) => Ok((dest, text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((dest, String::new())),
        Err(_) => {
            ensure_current_user_can_write(&dest);
            match std::fs::read_to_string(&dest) {
                Ok(text) => Ok((dest, text)),
                Err(e2) => Err(AppError::Io(format!(
                    "读取 {} 失败：{e2}。若刚从云端恢复，请确认当前用户对该文件有读写权限。",
                    dest.display()
                ))),
            }
        }
    }
}

/// 读取工作空间正本。无工作空间时读本机文件。
pub fn read_canonical_ssh_config(workspace: Option<&std::path::Path>) -> (std::path::PathBuf, String) {
    if let Some(ws) = workspace {
        return read_workspace_ssh_config(ws)
            .unwrap_or_else(|_| (workspace_ssh_config(ws), String::new()));
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

/// 云端/库内工作空间路径占位符。本机落盘给 OpenSSH 时再替换成当前工作空间绝对路径。
pub const WORKSPACE_PATH_TOKEN: &str = "%GAM_WORKSPACE%";

fn normalize_slashes(raw: &str) -> String {
    raw.trim().trim_matches('"').replace('\\', "/")
}

/// 若路径指向工作空间 `ssh-keys/` 下的文件，返回文件名（可含 `.pub`）。
pub fn workspace_key_filename(raw: &str) -> Option<String> {
    let mut t = normalize_slashes(raw);
    if let Some(rest) = t.strip_prefix(WORKSPACE_PATH_TOKEN) {
        t = rest.trim_start_matches('/').to_string();
    }
    let lower = t.to_ascii_lowercase();
    let after = if let Some(idx) = lower.rfind("/ssh-keys/") {
        &t[idx + "/ssh-keys/".len()..]
    } else if lower.starts_with("ssh-keys/") {
        &t["ssh-keys/".len()..]
    } else {
        return None;
    };
    if after.is_empty() || after.contains('/') || after.contains("..") {
        return None;
    }
    Some(after.to_string())
}

pub fn portable_deployed_path(raw: &str) -> String {
    if let Some(name) = workspace_key_filename(raw) {
        format!("{WORKSPACE_PATH_TOKEN}/ssh-keys/{name}")
    } else {
        raw.trim().to_string()
    }
}

/// 把工作空间密钥路径解析成当前机器上的绝对 IdentityFile。
pub fn resolve_identity_file(raw: &str, workspace: &std::path::Path) -> String {
    if let Some(name) = workspace_key_filename(raw) {
        identity_file_for_ssh(&workspace_ssh_keys_dir(workspace).join(name))
    } else {
        let t = normalize_slashes(raw);
        if t.chars().any(|c| c.is_whitespace()) && !t.starts_with('"') {
            format!("\"{t}\"")
        } else {
            t
        }
    }
}

fn map_ssh_identity_files(text: &str, mut f: impl FnMut(&str) -> String) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let is_idf = trimmed.len() >= 12 && trimmed[..12].eq_ignore_ascii_case("IdentityFile");
        if is_idf {
            let rest = trimmed[12..].trim_start();
            let rest = rest.strip_prefix('=').map(str::trim_start).unwrap_or(rest);
            let indent = line.len() - trimmed.len();
            out.push_str(&line[..indent]);
            out.push_str("IdentityFile ");
            out.push_str(&f(rest));
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !text.is_empty() && !text.ends_with('\n') {
        out.pop();
    }
    out
}

/// 本机 OpenSSH 需要绝对路径：把占位符和任意机器上的 `ssh-keys/` 路径展开到当前工作空间。
/// 只用于本机落盘，结果不得作为云同步 / 快照正文。
pub fn localize_ssh_config(text: &str, workspace: &std::path::Path) -> String {
    map_ssh_identity_files(text, |raw| resolve_identity_file(raw, workspace))
}

/// 云端、历史快照、同步比对只认这个可移植形态。
/// 换机后重写的本机绝对路径若原样上传，会覆盖对端机器上的路径。
pub fn canonical_ssh_for_sync(text: &str) -> String {
    portable_ssh_config(text)
}

/// 身份里的 `deployed_path` 同样不能带着某台机器的盘符进云端。
pub fn portableize_vault_data(data: &mut crate::model::VaultData) {
    for key in data.keys.iter_mut() {
        if let Some(path) = key.deployed_path.as_deref() {
            key.deployed_path = Some(portable_deployed_path(path));
        }
    }
}

pub fn vault_data_for_sync(data: &crate::model::VaultData) -> crate::model::VaultData {
    let mut cloned = data.clone();
    portableize_vault_data(&mut cloned);
    cloned
}

/// 把工作空间密钥路径改写成占位符，便于跨机器同步。
pub fn portable_ssh_config(text: &str) -> String {
    map_ssh_identity_files(text, |raw| {
        if workspace_key_filename(raw).is_some() {
            portable_deployed_path(raw)
        } else {
            normalize_slashes(raw)
        }
    })
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
        assert!(is_system_include_stub(
            "# c\nInclude git-keymaster.config\n",
            dest
        ));
    }

    #[test]
    fn cloud_form_ignores_rewritten_machine_paths() {
        let portable = "Host gh\n    HostName github.com\n    User git\n    IdentityFile %GAM_WORKSPACE%/ssh-keys/id_ed25519\n    IdentitiesOnly yes\n";
        let old = Path::new("D:/gitIdentifyData");
        let neu = Path::new("D:/dataSpace/gitIdentityData");
        let on_old = localize_ssh_config(portable, old);
        let on_new = localize_ssh_config(portable, neu);
        assert!(on_old.contains("D:/gitIdentifyData/ssh-keys/id_ed25519"));
        assert!(on_new.contains("D:/dataSpace/gitIdentityData/ssh-keys/id_ed25519"));
        assert_ne!(on_old, on_new);
        assert_eq!(canonical_ssh_for_sync(&on_old), portable);
        assert_eq!(canonical_ssh_for_sync(&on_new), portable);
        assert_eq!(canonical_ssh_for_sync(&on_old), canonical_ssh_for_sync(&on_new));
    }

    #[test]
    fn vault_data_for_sync_strips_machine_deployed_paths() {
        let mut data = crate::model::VaultData::default();
        data.keys.push(crate::model::KeyRecord {
            id: "k1".into(),
            name: "id_ed25519_a".into(),
            algorithm: "ed25519".into(),
            fingerprint: "SHA256:x".into(),
            public_openssh: "ssh-ed25519 AAAA".into(),
            bits: Some(256),
            has_passphrase: true,
            weak: false,
            source_path: None,
            deployed_path: Some("D:/dataSpace/gitIdentityData/ssh-keys/id_ed25519_a".into()),
            imported_at: "t".into(),
        });
        let sync = vault_data_for_sync(&data);
        assert_eq!(
            sync.keys[0].deployed_path.as_deref(),
            Some("%GAM_WORKSPACE%/ssh-keys/id_ed25519_a")
        );
        assert_ne!(sync.keys[0].deployed_path, data.keys[0].deployed_path);
    }

    #[test]
    fn ssh_identity_paths_relocate_across_workspaces() {
        let old = "Host github-a\n    IdentityFile D:/gitIdentifyData/ssh-keys/id_ed25519_a.pub\n    IdentitiesOnly yes\n";
        let ws = Path::new(r"D:\dataSpace\gitIdentityData");
        let local = localize_ssh_config(old, ws);
        assert!(local.contains("D:/dataSpace/gitIdentityData/ssh-keys/id_ed25519_a.pub"));
        assert!(!local.contains("gitIdentifyData/ssh-keys"));

        let portable = portable_ssh_config(&local);
        assert!(portable.contains("%GAM_WORKSPACE%/ssh-keys/id_ed25519_a.pub"));
        assert_eq!(localize_ssh_config(&portable, ws), local);

        let again = localize_ssh_config(&local, ws);
        assert_eq!(again, local);

        let external = "Host lab\n    IdentityFile C:/Users/me/.ssh/id_ed25519\n";
        assert!(localize_ssh_config(external, ws).contains("C:/Users/me/.ssh/id_ed25519"));
        assert_eq!(
            portable_deployed_path(r"D:\gitIdentifyData\ssh-keys\id_ed25519_a"),
            "%GAM_WORKSPACE%/ssh-keys/id_ed25519_a"
        );
    }

    #[test]
    fn common_bin_dirs_cover_current_os() {
        let dirs = common_bin_dirs();
        assert!(!dirs.is_empty());
        #[cfg(windows)]
        assert!(dirs.iter().any(|p| p.to_string_lossy().contains("Git")));
        #[cfg(target_os = "macos")]
        assert!(dirs.iter().any(|p| p.to_string_lossy().contains("homebrew") || p.to_string_lossy().contains("/usr/local")));
    }

    #[test]
    fn resolve_bin_rejects_missing_name() {
        assert!(resolve_bin("gam-definitely-not-installed-bin-xyz").is_none());
    }
}
