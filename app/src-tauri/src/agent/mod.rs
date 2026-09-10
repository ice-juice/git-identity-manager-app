//! ssh-agent 管理（M4）。
//!
//! 加载走 M0 验证的路径：在 Rust 侧解密私钥 → 经 stdin 喂给 `ssh-add -`，
//! 私钥明文全程不落盘。列表按指纹反查身份（原生命令只给指纹）。
//! 本机统一使用 Git 自带 ssh-agent（与 Windows OpenSSH 服务隔离）。

use crate::error::{AppError, Result};
use crate::model::{Identity, KeyRecord};
use crate::ssh::key;
use crate::store;
use crate::sys;
use crate::vault::Vault;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub mod unify;

/// agent 中的一把 key（来自 `ssh-add -l`）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentKey {
    pub bits: Option<u32>,
    pub fingerprint: String,
    pub comment: String,
    pub algo: String,
}

/// 反查后的展示项。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentKeyResolved {
    pub agent: AgentKey,
    pub identity_name: Option<String>,
    pub key_name: Option<String>,
}

/// 解析 `ssh-add -l` 输出。
pub fn parse_agent_list(output: &str) -> Vec<AgentKey> {
    let mut out = Vec::new();
    for line in output.lines() {
        let l = line.trim();
        if l.is_empty() || l.to_lowercase().contains("no identities") {
            continue;
        }
        // 形如: `256 SHA256:abc... some comment (ED25519)`
        let tokens: Vec<&str> = l.split_whitespace().collect();
        if tokens.len() < 3 {
            continue;
        }
        let bits = tokens[0].parse::<u32>().ok();
        let fingerprint = tokens[1].to_string();
        // 仅接受形似指纹的第二列。
        if !(fingerprint.starts_with("SHA256:") || fingerprint.starts_with("MD5:")) {
            continue;
        }
        let last = tokens[tokens.len() - 1];
        let algo = last.trim_start_matches('(').trim_end_matches(')').to_string();
        let comment = tokens[2..tokens.len() - 1].join(" ");
        out.push(AgentKey {
            bits,
            fingerprint,
            comment,
            algo,
        });
    }
    out
}

/// 按指纹把 agent key 反查到 vault 的密钥记录与身份。
pub fn reverse_lookup(
    agent_keys: &[AgentKey],
    keys: &[KeyRecord],
    identities: &[Identity],
) -> Vec<AgentKeyResolved> {
    agent_keys
        .iter()
        .map(|ak| {
            let rec = keys.iter().find(|k| k.fingerprint == ak.fingerprint);
            let identity_name = rec.and_then(|r| {
                identities
                    .iter()
                    .find(|i| i.key_id.as_deref() == Some(r.id.as_str()))
                    .map(|i| i.name.clone())
            });
            AgentKeyResolved {
                agent: ak.clone(),
                identity_name,
                key_name: rec.map(|r| r.name.clone()),
            }
        })
        .collect()
}

fn sibling_of(source: &str, file: &str) -> Option<PathBuf> {
    for (src, ssh) in crate::ssh::toolchain::candidate_paths() {
        if src == source && ssh.exists() {
            let p = ssh.with_file_name(file);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

fn ssh_exe_name(name: &str) -> String {
    #[cfg(windows)]
    {
        format!("{name}.exe")
    }
    #[cfg(not(windows))]
    {
        name.to_string()
    }
}

fn system_ssh_add() -> Option<PathBuf> {
    sibling_of("system", &ssh_exe_name("ssh-add"))
        .or_else(|| crate::ssh::toolchain::find_ssh_tool("ssh-add"))
}

pub(crate) fn git_ssh_bin() -> Option<PathBuf> {
    crate::ssh::toolchain::find_ssh_tool("ssh")
}

pub(crate) fn git_ssh_add_bin() -> Option<PathBuf> {
    crate::ssh::toolchain::find_ssh_tool("ssh-add")
}

fn git_ssh() -> Option<PathBuf> {
    git_ssh_bin()
}

fn git_ssh_add() -> Option<PathBuf> {
    git_ssh_add_bin()
}

fn git_ssh_agent() -> Option<PathBuf> {
    crate::ssh::toolchain::find_ssh_tool("ssh-agent")
}

/// 探测套接字上限：mtime 倒序最多试这么多个，避免残留文件把启动拖成 N × 超时。
const MAX_PROBE_CANDIDATES: usize = 3;
const STALE_SOCKET_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// 固定套接字，供终端与本进程共用同一只 Git ssh-agent。
pub fn stable_git_sock_path() -> PathBuf {
    git_agent_dir().join(crate::identity::AGENT_SOCK)
}

fn git_agent_dir() -> PathBuf {
    sys::home_dir().join(".ssh").join("agent")
}

fn stable_git_pid_path() -> PathBuf {
    git_agent_dir().join(crate::identity::AGENT_PID)
}

fn persist_git_agent_meta(env: &AgentEnv, winpid: Option<u32>) {
    let Some(pid) = &env.agent_pid else {
        return;
    };
    let winpid = winpid.or_else(|| parse_pid_file(&std::fs::read_to_string(stable_git_pid_path()).unwrap_or_default()).1);
    if let Some(parent) = stable_git_pid_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut body = pid.clone();
    if let Some(w) = winpid {
        body.push('\n');
        body.push_str(&w.to_string());
    }
    let _ = std::fs::write(stable_git_pid_path(), body);
}

fn read_stable_pid() -> Option<String> {
    parse_pid_file(&std::fs::read_to_string(stable_git_pid_path()).ok()?).0
}

/// pid 文件：首行是 MSYS `SSH_AGENT_PID`，可选第二行是 Windows PID（供回收）。
fn parse_pid_file(raw: &str) -> (Option<String>, Option<u32>) {
    let mut lines = raw.lines().map(str::trim).filter(|l| !l.is_empty());
    let msys = lines.next().filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()));
    let win = lines.next().and_then(|s| s.parse().ok());
    (msys.map(|s| s.to_string()), win)
}

fn git_agent_env(sock: String, pid: Option<String>) -> Option<AgentEnv> {
    Some(AgentEnv {
        auth_sock: Some(sock),
        agent_pid: pid.or_else(read_stable_pid),
        ssh_add: Some(git_ssh_add()?.display().to_string()),
        ssh: Some(git_ssh()?.display().to_string()),
    })
}

/// 解析 ssh-add 可执行路径（与所选 ssh 同目录），退回 PATH 中的 `ssh-add`。
pub fn ssh_add_path() -> String {
    system_ssh_add()
        .or_else(git_ssh_add)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "ssh-add".to_string())
}

fn resolve_ssh_add(env: &AgentEnv) -> String {
    if let Some(p) = &env.ssh_add {
        return p.clone();
    }
    if env.auth_sock.is_some() {
        return git_ssh_add()
            .map(|p| p.display().to_string())
            .unwrap_or_else(ssh_add_path);
    }
    system_ssh_add()
        .map(|p| p.display().to_string())
        .unwrap_or_else(ssh_add_path)
}

fn apply_agent_env(cmd: &mut std::process::Command, env: &AgentEnv) {
    if let Some(sock) = &env.auth_sock {
        cmd.env("SSH_AUTH_SOCK", sock);
        if let Some(pid) = &env.agent_pid {
            cmd.env("SSH_AGENT_PID", pid);
        }
    } else {
        // 清掉进程里可能残留的 MSYS 套接字，否则 System32 ssh-add 会去找 /tmp/ssh-...
        cmd.env_remove("SSH_AUTH_SOCK");
        cmd.env_remove("SSH_AGENT_PID");
    }
}

/// 运行 `ssh-add -l`。exit code 2 通常表示 agent 未运行。
pub fn list(env: &AgentEnv) -> Result<Vec<AgentKey>> {
    let (out, _err, code) = run_ssh_add(env, &["-l"], None)?;
    if code == 2 {
        return Err(AppError::Other("ssh-agent 未运行".into()));
    }
    Ok(parse_agent_list(&out))
}

/// 把一把 key 加载进 agent：Rust 侧解密 → stdin 喂 `ssh-add -`。
pub fn load_key(v: &Vault, env: &AgentEnv, key_id: &str) -> Result<()> {
    let enc = store::load_key(v, key_id)?;
    let enc_text = String::from_utf8_lossy(&enc).to_string();
    let secrets = store::load_secrets(v)?;
    let passphrase = secrets.key_passphrases.get(key_id).map(|s| s.as_str());
    let plain = key::decrypt_to_openssh(&enc_text, passphrase)?;
    let (_o, e, code) = run_ssh_add(env, &["-"], Some(plain.as_bytes()))?;
    if code != 0 {
        let hint = e.trim();
        if is_agent_unreachable(hint) {
            return Err(AppError::Other(match crate::ssh::toolchain::host_os() {
                "windows" => {
                    "ssh-add 连不上 agent。Windows OpenSSH 服务可能未启动，已尝试改用 Git 自带 ssh-agent。请再点一次「一键加载」。"
                        .into()
                }
                _ => {
                    "ssh-add 连不上 agent。请点「确保运行」拉起本机 ssh-agent，然后重试。".into()
                }
            }));
        }
        return Err(AppError::Other(format!("ssh-add 加载失败：{hint}")));
    }
    Ok(())
}

/// 卸载一把 key（把公钥写临时文件后 `ssh-add -d`）。
pub fn unload_public(env: &AgentEnv, public_openssh: &str) -> Result<()> {
    let tmp = std::env::temp_dir().join(format!("gam-pub-{}.pub", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, format!("{}\n", public_openssh.trim()))?;
    let res = run_ssh_add(env, &["-d", &tmp.display().to_string()], None);
    let _ = std::fs::remove_file(&tmp);
    let (_o, e, code) = res?;
    if code != 0 {
        return Err(AppError::Other(format!("ssh-add -d 卸载失败：{e}")));
    }
    Ok(())
}

/// 清空 agent 全部 key（`ssh-add -D`）。
pub fn clear(env: &AgentEnv) -> Result<()> {
    let (_o, e, code) = run_ssh_add(env, &["-D"], None)?;
    if code != 0 {
        return Err(AppError::Other(format!("ssh-add -D 失败：{e}")));
    }
    Ok(())
}

/// agent 环境：免提权 fallback 时携带 SSH_AUTH_SOCK / SSH_AGENT_PID，
/// 以及与该 agent **配套** 的 ssh / ssh-add（Windows 系统 agent 与 Git/MSYS agent 不能混用）。
#[derive(Debug, Clone, Default)]
pub struct AgentEnv {
    pub auth_sock: Option<String>,
    pub agent_pid: Option<String>,
    pub ssh_add: Option<String>,
    pub ssh: Option<String>,
}

pub fn apply_to_command(cmd: &mut std::process::Command, env: &AgentEnv) {
    apply_agent_env(cmd, env);
}

pub fn ssh_bin(env: &AgentEnv) -> String {
    env.ssh.clone().unwrap_or_else(|| "ssh".into())
}

/// 当前 env 能否列出密钥（空 identities 也算 agent 已运行）。
pub fn is_ready(env: &AgentEnv) -> bool {
    list(env).is_ok()
}

fn is_agent_unreachable(hint: &str) -> bool {
    let h = hint.to_ascii_lowercase();
    h.contains("connection refused")
        || h.contains("no such file or directory")
        || h.contains("could not open a connection")
        || h.contains("error connecting to agent")
}

/// 统一使用 Git 自带 ssh-agent：先复用固定套接字，没有再启动。
pub fn ensure() -> Result<AgentEnv> {
    if let Some(env) = probe_existing_git_agent() {
        persist_git_agent_meta(&env, None);
        cleanup_stale_agent_sockets(env.auth_sock.as_deref());
        return Ok(env);
    }
    clear_stale_managed_auth_sock();
    recycle_previous_git_agent();
    cleanup_stale_agent_sockets(None);
    start_git_agent()
}

/// 把 Windows 路径转成 Git/MSYS 的 `SSH_AUTH_SOCK` 形式：`C:\Users\a` → `/c/Users/a`。
pub fn to_msys_sock_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let s = raw.replace('\\', "/");
    if s.len() >= 2 && s.as_bytes().get(1) == Some(&b':') {
        let drive = s.chars().next().unwrap_or('c').to_ascii_lowercase();
        return format!("/{drive}{}", &s[2..]);
    }
    s
}

/// `/c/Users/a` → `C:\Users\a`；已是 Windows 路径则只统一分隔符。
pub fn from_msys_sock_path(sock: &str) -> PathBuf {
    let s = sock.trim().replace('\\', "/");
    let bytes = s.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[2] == b'/' && bytes[1].is_ascii_alphabetic() {
        let drive = (bytes[1] as char).to_ascii_uppercase();
        let rest = s[3..].replace('/', std::path::MAIN_SEPARATOR_STR);
        return PathBuf::from(format!("{drive}:{sep}{rest}", sep = std::path::MAIN_SEPARATOR));
    }
    PathBuf::from(s.replace('/', std::path::MAIN_SEPARATOR_STR))
}

fn normalized_sock(sock: &str) -> String {
    sock.trim().replace('\\', "/").to_ascii_lowercase()
}

fn socks_eq(a: &str, b: &str) -> bool {
    normalized_sock(a) == normalized_sock(b)
}

/// 是否为本程序写过的 `~/.ssh/agent/...` 套接字（不含 `agent-xxx` 其它目录）。
pub fn is_managed_agent_sock(sock: &str) -> bool {
    normalized_sock(sock).contains("/.ssh/agent/")
}

/// `SSH_AUTH_SOCK` 指向的文件是否存在（MSYS 与 Windows 路径都认）。
pub fn auth_sock_target_exists(sock: &str) -> bool {
    if sock.trim().is_empty() {
        return false;
    }
    from_msys_sock_path(sock).exists()
}

/// 探测失败时：只清我们写入的随机/陈旧路径，不动用户其它合法 sock，也不清固定套接字名。
fn should_clear_managed_auth_sock(sock: &str, stable_msys: &str) -> bool {
    is_managed_agent_sock(sock) && !socks_eq(sock, stable_msys)
}

/// 按 mtime 倒序选出最多 `limit` 个待探测套接字。`prefer` 若有则固定占第一位并计入上限。
pub(crate) fn select_probe_socks(
    prefer: Option<&str>,
    mut others: Vec<(String, Option<SystemTime>)>,
    limit: usize,
) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    others.sort_by(|a, b| b.1.cmp(&a.1));
    let mut out = Vec::new();
    if let Some(p) = prefer {
        if !p.trim().is_empty() {
            out.push(p.to_string());
        }
    }
    for (sock, _) in others {
        if out.len() >= limit {
            break;
        }
        if !out.iter().any(|s| socks_eq(s, &sock)) {
            out.push(sock);
        }
    }
    out
}

fn probe_existing_git_agent() -> Option<AgentEnv> {
    let dir = git_agent_dir();
    let stable = stable_git_sock_path();
    let prefer = if stable.exists() {
        Some(to_msys_sock_path(&stable))
    } else {
        None
    };
    let mut others: Vec<(String, Option<SystemTime>)> = Vec::new();
    if let Ok(existing) = std::env::var("SSH_AUTH_SOCK") {
        let existing = existing.trim().to_string();
        if !existing.is_empty() && auth_sock_target_exists(&existing) {
            let mtime = std::fs::metadata(from_msys_sock_path(&existing))
                .and_then(|m| m.modified())
                .ok();
            others.push((existing, mtime));
        }
    }
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("pid") {
                continue;
            }
            if !p.exists() {
                continue;
            }
            let sock = to_msys_sock_path(&p);
            if prefer.as_deref().is_some_and(|s| socks_eq(s, &sock)) {
                continue;
            }
            let mtime = entry.metadata().and_then(|m| m.modified()).ok();
            others.push((sock, mtime));
        }
    }
    for sock in select_probe_socks(prefer.as_deref(), others, MAX_PROBE_CANDIDATES) {
        if let Some(env) = git_agent_env(sock, None) {
            if list(&env).is_ok() {
                return Some(env);
            }
        }
    }
    None
}

fn clear_stale_managed_auth_sock() {
    let stable = to_msys_sock_path(&stable_git_sock_path());
    let proc = std::env::var("SSH_AUTH_SOCK").ok();
    let user = user_env_var("SSH_AUTH_SOCK");
    let clear_proc = proc
        .as_deref()
        .is_some_and(|s| should_clear_managed_auth_sock(s, &stable));
    let clear_user = user
        .as_deref()
        .is_some_and(|s| should_clear_managed_auth_sock(s, &stable));
    if clear_proc {
        std::env::remove_var("SSH_AUTH_SOCK");
        std::env::remove_var("SSH_AGENT_PID");
    }
    if clear_user {
        delete_user_env_var("SSH_AUTH_SOCK");
        delete_user_env_var("SSH_AGENT_PID");
    }
}

fn recycle_previous_git_agent() {
    let raw = std::fs::read_to_string(stable_git_pid_path()).unwrap_or_default();
    let (msys, win) = parse_pid_file(&raw);
    if let Some(winpid) = win {
        if winpid != 0 && winpid != std::process::id() {
            sys::kill_pid(winpid);
        }
    } else if let Some(pid) = msys {
        if let Some(exe) = git_ssh_agent() {
            let sock = to_msys_sock_path(&stable_git_sock_path());
            let _ = sys::run_timeout_with_env(
                &exe.display().to_string(),
                &["-k", "-s"],
                Duration::from_secs(8),
                &[("SSH_AGENT_PID", &pid), ("SSH_AUTH_SOCK", &sock)],
            );
        }
    }
    let _ = std::fs::remove_file(stable_git_pid_path());
    let _ = std::fs::remove_file(stable_git_sock_path());
}

fn cleanup_stale_agent_sockets(keep_sock: Option<&str>) {
    let dir = git_agent_dir();
    let keep = keep_sock.map(from_msys_sock_path);
    let pid_path = stable_git_pid_path();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p == pid_path {
            continue;
        }
        if keep.as_ref().is_some_and(|k| k == &p) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        let Ok(age) = mtime.elapsed() else {
            continue;
        };
        if age <= STALE_SOCKET_MAX_AGE {
            continue;
        }
        let _ = std::fs::remove_file(&p);
    }
}

#[cfg(windows)]
fn user_env_var(name: &str) -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey("Environment").ok()?;
    key.get_value::<String, _>(name)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

#[cfg(not(windows))]
fn user_env_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}

#[cfg(windows)]
fn delete_user_env_var(name: &str) {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags("Environment", KEY_SET_VALUE) {
        let _ = key.delete_value(name);
    }
}

#[cfg(not(windows))]
fn delete_user_env_var(_name: &str) {}

fn parse_tasklist_ssh_agent_pids(output: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    for line in output.lines() {
        let cols: Vec<&str> = line
            .split(',')
            .map(|s| s.trim().trim_matches('"'))
            .collect();
        if cols.len() >= 2 && cols[0].to_ascii_lowercase().contains("ssh-agent") {
            if let Ok(pid) = cols[1].parse() {
                pids.push(pid);
            }
        }
    }
    pids
}

fn list_ssh_agent_winpids() -> Vec<u32> {
    #[cfg(windows)]
    {
        let Ok((out, _, _)) = sys::run_timeout(
            "tasklist",
            &["/FI", "IMAGENAME eq ssh-agent.exe", "/FO", "CSV", "/NH"],
            Duration::from_secs(3),
        ) else {
            return Vec::new();
        };
        return parse_tasklist_ssh_agent_pids(&out);
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

fn run_ssh_add(env: &AgentEnv, args: &[&str], stdin: Option<&[u8]>) -> Result<(String, String, i32)> {
    use std::process::{Command, Stdio};
    let exe = resolve_ssh_add(env);
    let mut cmd = Command::new(&exe);
    cmd.args(args);
    apply_agent_env(&mut cmd, env);
    crate::sys::hide_console(&mut cmd);
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Io(format!("执行 ssh-add 失败：{e}")))?;
    if let (Some(data), Some(mut si)) = (stdin, child.stdin.take()) {
        use std::io::Write;
        si.write_all(data).map_err(|e| AppError::Io(e.to_string()))?;
    }
    sys::wait_output_timeout(child, Duration::from_secs(8), "ssh-add")
}

/// 启动 Git 自带 ssh-agent，并绑到 `~/.ssh/agent/git-keymaster`。
///
/// 本机 Git for Windows（OpenSSH 10.5p1）实测：`-a /c/Users/.../git-keymaster`
/// 会把套接字落到该文件，并在 `-s` 输出里回显同一路径。不再回退到无 `-a` 的
/// `ssh-agent -s`——那会在目录里留下 `s.*.agent.*` 随机套接字，复用从未生效。
pub fn start_git_agent() -> Result<AgentEnv> {
    let ssh = git_ssh().ok_or_else(|| AppError::Other(crate::ssh::toolchain::missing_ssh_tool("ssh")))?;
    let exe = git_ssh_agent().ok_or_else(|| AppError::Other(crate::ssh::toolchain::missing_ssh_tool("ssh-agent")))?;
    let add = git_ssh_add().ok_or_else(|| AppError::Other(crate::ssh::toolchain::missing_ssh_tool("ssh-add")))?;
    std::fs::create_dir_all(git_agent_dir()).map_err(|e| AppError::Io(e.to_string()))?;
    let sock_path = stable_git_sock_path();
    let sock_msys = to_msys_sock_path(&sock_path);
    if sock_path.exists() {
        if let Some(env) = git_agent_env(sock_msys.clone(), None) {
            if list(&env).is_ok() {
                persist_git_agent_meta(&env, None);
                return Ok(env);
            }
        }
        let _ = std::fs::remove_file(&sock_path);
    }
    let exe_s = exe.display().to_string();
    let before = list_ssh_agent_winpids();
    let (out, err, code) = sys::run_timeout(&exe_s, &["-a", &sock_msys, "-s"], Duration::from_secs(8))?;
    let mut env = parse_agent_env(&out);
    if env.auth_sock.is_none() {
        env = parse_agent_env(&err);
    }
    if env.auth_sock.is_none() {
        recycle_previous_git_agent();
        let _ = std::fs::remove_file(&sock_path);
        let (out2, err2, code2) = sys::run_timeout(&exe_s, &["-a", &sock_msys, "-s"], Duration::from_secs(8))?;
        env = parse_agent_env(&out2);
        if env.auth_sock.is_none() {
            env = parse_agent_env(&err2);
        }
        if env.auth_sock.is_none() {
            let detail = [err, out, err2, out2]
                .into_iter()
                .find(|s| !s.trim().is_empty())
                .unwrap_or_default();
            return Err(AppError::Other(format!(
                "启动 Git ssh-agent 失败（exit {code}/{code2}）：{}",
                detail.trim()
            )));
        }
    }
    env.ssh_add = Some(add.display().to_string());
    env.ssh = Some(ssh.display().to_string());
    if env.auth_sock.is_none() {
        env.auth_sock = Some(sock_msys);
    }
    if list(&env).is_err() {
        return Err(AppError::Other(crate::ssh::toolchain::agent_add_mismatch()));
    }
    let after = list_ssh_agent_winpids();
    let winpid = after.into_iter().find(|p| !before.contains(p));
    persist_git_agent_meta(&env, winpid);
    Ok(env)
}

/// 解析 `ssh-agent -s` 输出中的 SSH_AUTH_SOCK 与 SSH_AGENT_PID。
pub fn parse_agent_env(output: &str) -> AgentEnv {
    let mut env = AgentEnv::default();
    for line in output.lines() {
        if let Some(v) = extract_env(line, "SSH_AUTH_SOCK") {
            env.auth_sock = Some(v);
        }
        if let Some(v) = extract_env(line, "SSH_AGENT_PID") {
            env.agent_pid = Some(v);
        }
    }
    env
}

fn extract_env(line: &str, key: &str) -> Option<String> {
    // 形如: `SSH_AUTH_SOCK=/tmp/ssh-xxx/agent.123; export SSH_AUTH_SOCK;`
    let idx = line.find(&format!("{key}="))?;
    let rest = &line[idx + key.len() + 1..];
    let val = rest.split(';').next()?.trim();
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, name: &str, fp: &str) -> KeyRecord {
        KeyRecord {
            id: id.into(),
            name: name.into(),
            algorithm: "ed25519".into(),
            fingerprint: fp.into(),
            public_openssh: "ssh-ed25519 AAAA x".into(),
            bits: Some(256),
            has_passphrase: true,
            weak: false,
            source_path: None,
            deployed_path: None,
            imported_at: "now".into(),
        }
    }

    fn ident(id: &str, name: &str, key_id: &str) -> Identity {
        Identity {
            id: id.into(),
            name: name.into(),
            platform: "github".into(),
            host_alias: format!("github-{name}"),
            real_host: "github.com".into(),
            user: "git".into(),
            email: None,
            git_user_name: None,
            key_id: Some(key_id.into()),
            owners: vec![],
            strict_mode: false,
            updated_at: String::new(),
        }
    }

    #[test]
    fn parses_ssh_add_list() {
        let out = "256 SHA256:abcDEF123 techn4950@gam (ED25519)\n2048 SHA256:zzz my rsa key (RSA)\n";
        let keys = parse_agent_list(out);
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].fingerprint, "SHA256:abcDEF123");
        assert_eq!(keys[0].comment, "techn4950@gam");
        assert_eq!(keys[0].algo, "ED25519");
        assert_eq!(keys[1].bits, Some(2048));
        assert_eq!(keys[1].comment, "my rsa key");
    }

    #[test]
    fn empty_agent_yields_nothing() {
        assert!(parse_agent_list("The agent has no identities.").is_empty());
        assert!(parse_agent_list("").is_empty());
    }

    #[test]
    fn reverse_lookup_maps_fingerprint_to_identity() {
        let agent = parse_agent_list("256 SHA256:FP_TECHN techn@gam (ED25519)\n256 SHA256:UNKNOWN x (ED25519)\n");
        let keys = vec![rec("k1", "id_ed25519_techn", "SHA256:FP_TECHN")];
        let ids = vec![ident("i1", "techn4950", "k1")];
        let resolved = reverse_lookup(&agent, &keys, &ids);
        assert_eq!(resolved[0].identity_name.as_deref(), Some("techn4950"));
        assert_eq!(resolved[0].key_name.as_deref(), Some("id_ed25519_techn"));
        // 未登记的指纹反查为空。
        assert_eq!(resolved[1].identity_name, None);
    }

    #[test]
    fn parse_agent_env_extracts_sock_and_pid() {
        let out = "SSH_AUTH_SOCK=/tmp/ssh-AbC/agent.4242; export SSH_AUTH_SOCK;\nSSH_AGENT_PID=4243; export SSH_AGENT_PID;\necho Agent pid 4243;\n";
        let env = parse_agent_env(out);
        assert_eq!(env.auth_sock.as_deref(), Some("/tmp/ssh-AbC/agent.4242"));
        assert_eq!(env.agent_pid.as_deref(), Some("4243"));
    }

    #[test]
    fn parse_agent_env_supports_git_for_windows_home_socket() {
        let out = "SSH_AUTH_SOCK=/c/Users/Jeck/.ssh/agent/s.abc.agent.xyz; export SSH_AUTH_SOCK;\nSSH_AGENT_PID=847; export SSH_AGENT_PID;\n";
        let env = parse_agent_env(out);
        assert_eq!(
            env.auth_sock.as_deref(),
            Some("/c/Users/Jeck/.ssh/agent/s.abc.agent.xyz")
        );
        assert_eq!(env.agent_pid.as_deref(), Some("847"));
    }

    #[test]
    fn to_msys_sock_path_converts_windows_drive() {
        let p = std::path::Path::new(r"C:\Users\Jeck\.ssh\agent\s.abc");
        assert_eq!(to_msys_sock_path(p), "/c/Users/Jeck/.ssh/agent/s.abc");
    }

    #[test]
    fn from_msys_sock_path_roundtrips_drive() {
        let msys = "/c/Users/Jeck/.ssh/agent/git-keymaster";
        let win = from_msys_sock_path(msys);
        assert_eq!(
            win,
            PathBuf::from(format!(
                "C:{sep}Users{sep}Jeck{sep}.ssh{sep}agent{sep}git-keymaster",
                sep = std::path::MAIN_SEPARATOR
            ))
        );
        assert_eq!(to_msys_sock_path(&win), msys);
        // 用斜杠归一化比较，避免 Unix 把未转换的 `C:\...` 当成单路径组件。
        assert_eq!(
            from_msys_sock_path(r"C:\Users\Jeck\.ssh\agent\s.abc")
                .to_string_lossy()
                .replace('\\', "/"),
            "C:/Users/Jeck/.ssh/agent/s.abc"
        );
    }

    #[test]
    fn managed_sock_only_matches_ssh_agent_dir() {
        assert!(is_managed_agent_sock("/c/Users/Jeck/.ssh/agent/git-keymaster"));
        assert!(is_managed_agent_sock("/c/Users/Jeck/.ssh/agent/git-account-manager"));
        assert!(is_managed_agent_sock(r"C:\Users\Jeck\.ssh\agent\s.abc.agent.xyz"));
        assert!(!is_managed_agent_sock("/c/Users/Jeck/.ssh/agent-p0a-test/git-account-manager"));
        assert!(!is_managed_agent_sock("/tmp/ssh-AbC/agent.4242"));
        assert!(!is_managed_agent_sock(r"\\.\pipe\openssh-ssh-agent"));
    }

    #[test]
    fn clear_managed_sock_skips_foreign_and_stable() {
        let stable = "/c/Users/Jeck/.ssh/agent/git-keymaster";
        assert!(!should_clear_managed_auth_sock(stable, stable));
        assert!(!should_clear_managed_auth_sock("/tmp/ssh-AbC/agent.1", stable));
        assert!(should_clear_managed_auth_sock(
            "/c/Users/Jeck/.ssh/agent/s.abc.agent.xyz",
            stable
        ));
    }

    #[test]
    fn select_probe_socks_prefers_stable_and_caps_at_three() {
        let t1 = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let t2 = SystemTime::UNIX_EPOCH + Duration::from_secs(200);
        let t3 = SystemTime::UNIX_EPOCH + Duration::from_secs(300);
        let t4 = SystemTime::UNIX_EPOCH + Duration::from_secs(400);
        let t5 = SystemTime::UNIX_EPOCH + Duration::from_secs(500);
        let others = vec![
            ("/c/Users/x/.ssh/agent/old1".into(), Some(t1)),
            ("/c/Users/x/.ssh/agent/old2".into(), Some(t2)),
            ("/c/Users/x/.ssh/agent/mid".into(), Some(t3)),
            ("/c/Users/x/.ssh/agent/new1".into(), Some(t4)),
            ("/c/Users/x/.ssh/agent/new2".into(), Some(t5)),
        ];
        let got = select_probe_socks(
            Some("/c/Users/x/.ssh/agent/git-keymaster"),
            others,
            MAX_PROBE_CANDIDATES,
        );
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], "/c/Users/x/.ssh/agent/git-keymaster");
        assert_eq!(got[1], "/c/Users/x/.ssh/agent/new2");
        assert_eq!(got[2], "/c/Users/x/.ssh/agent/new1");
        assert!(!got.iter().any(|s| s.ends_with("old1") || s.ends_with("old2") || s.ends_with("mid")));
    }

    #[test]
    fn select_probe_socks_without_prefer_takes_newest_three() {
        let t1 = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        let t2 = SystemTime::UNIX_EPOCH + Duration::from_secs(2);
        let t3 = SystemTime::UNIX_EPOCH + Duration::from_secs(3);
        let t4 = SystemTime::UNIX_EPOCH + Duration::from_secs(4);
        let others = vec![
            ("a".into(), Some(t1)),
            ("b".into(), Some(t2)),
            ("c".into(), Some(t3)),
            ("d".into(), Some(t4)),
            ("e".into(), None),
        ];
        let got = select_probe_socks(None, others, MAX_PROBE_CANDIDATES);
        assert_eq!(got, vec!["d".to_string(), "c".to_string(), "b".to_string()]);
    }

    #[test]
    fn parse_pid_file_reads_msys_and_optional_winpid() {
        assert_eq!(parse_pid_file("4243\n"), (Some("4243".into()), None));
        assert_eq!(parse_pid_file("4243\n14880\n"), (Some("4243".into()), Some(14880)));
        assert_eq!(parse_pid_file(""), (None, None));
        assert_eq!(parse_pid_file("not-a-pid\n12\n"), (None, Some(12)));
    }

    #[test]
    fn parse_tasklist_extracts_ssh_agent_pids() {
        let csv = "\"ssh-agent.exe\",\"7572\",\"Console\",\"1\",\"3,200 K\"\r\n\"ssh-agent.exe\",\"14260\",\"Console\",\"1\",\"2,100 K\"\r\n";
        assert_eq!(parse_tasklist_ssh_agent_pids(csv), vec![7572, 14260]);
        assert!(parse_tasklist_ssh_agent_pids("INFO: No tasks are running").is_empty());
    }

    #[test]
    fn agent_unreachable_detects_connection_refused() {
        assert!(is_agent_unreachable("Error connecting to agent: Connection refused"));
        assert!(is_agent_unreachable("Could not open a connection to your authentication agent."));
        assert!(!is_agent_unreachable("Identity added: (stdin)"));
    }
}
