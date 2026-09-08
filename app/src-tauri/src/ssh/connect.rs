//! 连接体检：解析 `ssh -T` 的输出，提取真实账号名并把常见报错翻译成中文。

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthResult {
    pub ok: bool,
    /// 平台返回的真实账号名（如 GitHub 的 `Hi techn4950!`）。
    pub account: Option<String>,
    /// 面向用户的中文说明。
    pub message: String,
    /// 稳定机器码，便于“一键修复”。
    pub error_code: Option<String>,
}

/// 解析 ssh -T 的合并输出（GitHub 等常把问候写到 stderr）。
pub fn parse_auth_result(stdout: &str, stderr: &str) -> AuthResult {
    let combined = format!("{}\n{}", stdout, stderr);

    if let Some(account) = extract_account(&combined) {
        return AuthResult {
            ok: true,
            account: Some(account.clone()),
            message: format!("认证成功，实测账号：{}", account),
            error_code: None,
        };
    }

    // 错误翻译。
    let lower = combined.to_lowercase();
    let (code, msg) = if lower.contains("permission denied") {
        (
            "PERMISSION_DENIED",
            "认证被拒（publickey）。可能是公钥未添加到该账号、密钥未加载进 agent，或走了错误的密钥。",
        )
    } else if lower.contains("could not resolve hostname") {
        ("DNS_FAIL", "无法解析主机名，请检查 Host/HostName 配置与网络。")
    } else if lower.contains("host key verification failed") {
        ("HOSTKEY_CHANGED", "主机指纹校验失败。若主机确实变更，请更新 known_hosts。")
    } else if lower.contains("connection timed out") || lower.contains("connection refused") {
        ("NETWORK", "连接超时/被拒，请检查网络、端口或代理设置。")
    } else if lower.contains("too many authentication failures") {
        (
            "TOO_MANY_AUTH",
            "认证尝试过多：agent 里 key 太多且缺少 IdentitiesOnly yes，建议开启后重试。",
        )
    } else {
        ("UNKNOWN", "未能识别的返回，请展开原始输出排查。")
    };

    AuthResult {
        ok: false,
        account: None,
        message: msg.to_string(),
        error_code: Some(code.to_string()),
    }
}

/// 从问候语中提取账号名，兼容 GitHub / GitLab / Gitea。
fn extract_account(text: &str) -> Option<String> {
    for line in text.lines() {
        let l = line.trim();
        // GitLab: "Welcome to GitLab, @username!"
        if let Some(rest) = l.split("Welcome to GitLab, @").nth(1) {
            if let Some(name) = rest.split('!').next() {
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
        // Gitea: "Hi there, username!"
        if let Some(rest) = l.strip_prefix("Hi there, ") {
            if let Some(name) = rest.split(['!', ',']).next() {
                let n = name.trim();
                if !n.is_empty() {
                    return Some(n.to_string());
                }
            }
        }
        // GitHub: "Hi username! You've successfully authenticated..."
        if let Some(rest) = l.strip_prefix("Hi ") {
            // 排除 "Hi there," 已在上面处理。
            if !rest.starts_with("there,") {
                if let Some(name) = rest.split('!').next() {
                    let n = name.trim();
                    if !n.is_empty() {
                        return Some(n.to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_success() {
        let stderr = "Hi techn4950! You've successfully authenticated, but GitHub does not provide shell access.";
        let r = parse_auth_result("", stderr);
        assert!(r.ok);
        assert_eq!(r.account.as_deref(), Some("techn4950"));
    }

    #[test]
    fn gitlab_success() {
        let r = parse_auth_result("Welcome to GitLab, @juice520!", "");
        assert!(r.ok);
        assert_eq!(r.account.as_deref(), Some("juice520"));
    }

    #[test]
    fn gitea_success() {
        let r = parse_auth_result("Hi there, boxadmin! You've successfully authenticated", "");
        assert!(r.ok);
        assert_eq!(r.account.as_deref(), Some("boxadmin"));
    }

    #[test]
    fn permission_denied() {
        let r = parse_auth_result("", "git@github.com: Permission denied (publickey).");
        assert!(!r.ok);
        assert_eq!(r.error_code.as_deref(), Some("PERMISSION_DENIED"));
    }

    #[test]
    fn dns_failure() {
        let r = parse_auth_result("", "ssh: Could not resolve hostname githubx: ...");
        assert_eq!(r.error_code.as_deref(), Some("DNS_FAIL"));
    }

    #[test]
    fn hostkey_changed() {
        let r = parse_auth_result("", "Host key verification failed.");
        assert_eq!(r.error_code.as_deref(), Some("HOSTKEY_CHANGED"));
    }
}
