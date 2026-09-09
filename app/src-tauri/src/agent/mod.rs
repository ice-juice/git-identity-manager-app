//! ssh-agent 管理（M4）。
//!
//! 加载走 M0 验证的路径：在 Rust 侧解密私钥 → 经 stdin 喂给 `ssh-add -`，
//! 私钥明文全程不落盘。列表按指纹反查身份（原生命令只给指纹）。

use crate::error::{AppError, Result};
use crate::model::{Identity, KeyRecord};
use crate::ssh::key;
use crate::store;
use crate::sys;
use crate::vault::Vault;
use serde::Serialize;
use std::path::PathBuf;

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

fn system_ssh() -> Option<PathBuf> {
    sibling_of("system", &ssh_exe_name("ssh"))
}

fn system_ssh_add() -> Option<PathBuf> {
    sibling_of("system", &ssh_exe_name("ssh-add"))
}

fn git_ssh() -> Option<PathBuf> {
    sibling_of("git", &ssh_exe_name("ssh"))
}

fn git_ssh_add() -> Option<PathBuf> {
    sibling_of("git", &ssh_exe_name("ssh-add"))
}

fn git_ssh_agent() -> Option<PathBuf> {
    sibling_of("git", &ssh_exe_name("ssh-agent"))
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
            return Err(AppError::Other(
                "ssh-add 连不上 agent。Windows OpenSSH 服务可能未启动，已尝试改用 Git 自带 ssh-agent。请再点一次「一键加载」。".into(),
            ));
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

fn system_env() -> AgentEnv {
    AgentEnv {
        auth_sock: None,
        agent_pid: None,
        ssh_add: system_ssh_add().map(|p| p.display().to_string()),
        ssh: system_ssh().map(|p| p.display().to_string()),
    }
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

/// 先探测 Windows OpenSSH 服务，不通再复用/启动 Git 自带 ssh-agent。
pub fn ensure() -> Result<AgentEnv> {
    // 不在解锁路径上执行 `sc start`：服务被禁用时会卡住很久，看起来像崩溃。
    if let Ok(env) = probe_system_agent() {
        return Ok(env);
    }
    if let Some(env) = probe_existing_git_agent() {
        return Ok(env);
    }
    start_git_agent()
}

fn probe_system_agent() -> Result<AgentEnv> {
    let env = system_env();
    list(&env)?;
    Ok(env)
}

/// 把 Windows 路径转成 Git/MSYS 的 `SSH_AUTH_SOCK` 形式：`C:\Users\a` → `/c/Users/a`。
pub fn to_msys_sock_path(path: &std::path::Path) -> String {
    let raw = path.to_string_lossy();
    let s = raw.replace('\\', "/");
    if s.len() >= 2 && s.as_bytes().get(1) == Some(&b':') {
        let drive = s.chars().next().unwrap_or('c').to_ascii_lowercase();
        return format!("/{drive}{}", &s[2..]);
    }
    s
}

fn probe_existing_git_agent() -> Option<AgentEnv> {
    let add = git_ssh_add()?;
    let ssh = git_ssh()?;
    let mut socks = Vec::new();
    if let Ok(existing) = std::env::var("SSH_AUTH_SOCK") {
        if !existing.trim().is_empty() {
            socks.push(existing);
        }
    }
    let dir = sys::home_dir().join(".ssh").join("agent");
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.exists() {
                socks.push(to_msys_sock_path(&p));
            }
        }
    }
    for sock in socks {
        let env = AgentEnv {
            auth_sock: Some(sock),
            agent_pid: None,
            ssh_add: Some(add.display().to_string()),
            ssh: Some(ssh.display().to_string()),
        };
        if list(&env).is_ok() {
            return Some(env);
        }
    }
    None
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
    let out = child.wait_with_output().map_err(|e| AppError::Io(e.to_string()))?;
    Ok((
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    ))
}

/// 启动 Git 自带 ssh-agent 作为普通用户进程（免提权 fallback），解析其输出的 sock/pid。
pub fn start_git_agent() -> Result<AgentEnv> {
    let ssh = git_ssh().ok_or_else(|| AppError::Other("未找到 Git 自带 ssh".into()))?;
    let exe = git_ssh_agent().ok_or_else(|| AppError::Other("未找到 Git 自带 ssh-agent".into()))?;
    let add = git_ssh_add().ok_or_else(|| AppError::Other("未找到 Git 自带 ssh-add".into()))?;
    let (out, err, code) = sys::run(&exe.display().to_string(), &["-s"])?;
    let mut env = parse_agent_env(&out);
    if env.auth_sock.is_none() {
        env = parse_agent_env(&err);
    }
    if env.auth_sock.is_none() {
        let detail = if !err.trim().is_empty() { err } else { out };
        return Err(AppError::Other(format!(
            "启动 Git ssh-agent 失败（exit {code}）：{}",
            detail.trim()
        )));
    }
    env.ssh_add = Some(add.display().to_string());
    env.ssh = Some(ssh.display().to_string());
    if list(&env).is_err() {
        return Err(AppError::Other(
            "已启动 Git ssh-agent，但配套 ssh-add 仍连不上。请确认已安装 Git for Windows。".into(),
        ));
    }
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
    fn agent_unreachable_detects_connection_refused() {
        assert!(is_agent_unreachable("Error connecting to agent: Connection refused"));
        assert!(is_agent_unreachable("Could not open a connection to your authentication agent."));
        assert!(!is_agent_unreachable("Identity added: (stdin)"));
    }
}
