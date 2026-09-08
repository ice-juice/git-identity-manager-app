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

/// 解析 ssh-add 可执行路径（与所选 ssh 同目录），退回 PATH 中的 `ssh-add`。
pub fn ssh_add_path() -> String {
    for (_src, ssh) in crate::ssh::toolchain::candidate_paths() {
        if ssh.exists() {
            let add = ssh.with_file_name("ssh-add.exe");
            if add.exists() {
                return add.display().to_string();
            }
        }
    }
    "ssh-add".to_string()
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
        return Err(AppError::Other(format!("ssh-add 加载失败：{e}")));
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

/// agent 环境：免提权 fallback 时携带 SSH_AUTH_SOCK / SSH_AGENT_PID。
#[derive(Debug, Clone, Default)]
pub struct AgentEnv {
    pub auth_sock: Option<String>,
    pub agent_pid: Option<String>,
}

fn run_ssh_add(env: &AgentEnv, args: &[&str], stdin: Option<&[u8]>) -> Result<(String, String, i32)> {
    use std::process::{Command, Stdio};
    let exe = ssh_add_path();
    let mut cmd = Command::new(exe);
    cmd.args(args);
    if let Some(sock) = &env.auth_sock {
        cmd.env("SSH_AUTH_SOCK", sock);
    }
    if let Some(pid) = &env.agent_pid {
        cmd.env("SSH_AGENT_PID", pid);
    }
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
    // 找到与 ssh 同目录的 ssh-agent。
    let mut agent_exe: Option<PathBuf> = None;
    for (src, ssh) in crate::ssh::toolchain::candidate_paths() {
        if src == "git" && ssh.exists() {
            let a = ssh.with_file_name("ssh-agent.exe");
            if a.exists() {
                agent_exe = Some(a);
                break;
            }
        }
    }
    let exe = agent_exe.ok_or_else(|| AppError::Other("未找到 Git 自带 ssh-agent".into()))?;
    let (out, _e, _c) = sys::run(&exe.display().to_string(), &["-s"])?;
    Ok(parse_agent_env(&out))
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
}
