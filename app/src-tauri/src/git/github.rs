//! GitHub API 薄封装（PAT 可选功能）：上传公钥、拉取账号与所属组织。
//! Token 只从 vault 取用，绝不落明文、日志脱敏。

use crate::error::{AppError, Result};
use serde::Serialize;

const API: &str = "https://api.github.com";
const UA: &str = "git-account-manager";

fn client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| AppError::Other(format!("HTTP 客户端构建失败：{e}")))
}

/// 校验 PAT 并返回账号名（`GET /user`）。
pub fn whoami(token: &str) -> Result<String> {
    let resp = client()?
        .get(format!("{API}/user"))
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| AppError::Other(format!("请求失败：{e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Invalid(format!("PAT 校验失败：HTTP {}", resp.status())));
    }
    let v: serde_json::Value = resp.json().map_err(|e| AppError::Serde(e.to_string()))?;
    v.get("login")
        .and_then(|l| l.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::Other("响应缺少 login 字段".into()))
}

/// 拉取账号所属组织（`GET /user/orgs`）。
pub fn list_orgs(token: &str) -> Result<Vec<String>> {
    let resp = client()?
        .get(format!("{API}/user/orgs?per_page=100"))
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| AppError::Other(format!("请求失败：{e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Invalid(format!("拉取组织失败：HTTP {}", resp.status())));
    }
    let arr: serde_json::Value = resp.json().map_err(|e| AppError::Serde(e.to_string()))?;
    Ok(arr
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|o| o.get("login").and_then(|l| l.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default())
}

#[derive(Serialize)]
struct AddKeyBody<'a> {
    title: &'a str,
    key: &'a str,
}

/// 上传公钥（`POST /user/keys`，需 `admin:public_key`）。
pub fn upload_public_key(token: &str, title: &str, public_openssh: &str) -> Result<()> {
    let body = AddKeyBody {
        title,
        key: public_openssh.trim(),
    };
    let resp = client()?
        .post(format!("{API}/user/keys"))
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .json(&body)
        .send()
        .map_err(|e| AppError::Other(format!("请求失败：{e}")))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        let code = resp.status();
        let text = resp.text().unwrap_or_default();
        if text.contains("key is already in use") {
            return Err(AppError::Invalid("该公钥已在此账号中".into()));
        }
        Err(AppError::Invalid(format!("上传公钥失败：HTTP {code}")))
    }
}
