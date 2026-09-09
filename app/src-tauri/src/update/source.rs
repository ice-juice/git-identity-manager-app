//! 把用户配置（或默认值）解析为插件可用的 endpoints。

use crate::app_config::{AppConfig, UpdateSource, DEFAULT_UPDATE_REPO};
use crate::error::{AppError, Result};
use crate::platform;
use url::Url;

const GITHUB_REPO_MAX_LEN: usize = 128;

/// 解析 endpoints 时可用的运行时上下文（占位符替换）。
#[derive(Debug, Clone)]
pub struct EndpointCtx {
    pub target: String,
    pub arch: String,
    pub current_version: String,
}

impl EndpointCtx {
    pub fn current() -> Self {
        EndpointCtx {
            target: platform::updater_target().into(),
            arch: platform::updater_arch().into(),
            current_version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

/// 返回「有效源」——None 配置回填为默认 GitHub 仓库。
pub fn effective_source(cfg: &AppConfig) -> UpdateSource {
    UpdateSource::effective(cfg.update_source.as_ref())
}

/// 把用户配置（或默认值）解析为插件可用的 endpoints。
pub fn resolve_endpoints(src: &UpdateSource) -> Result<Vec<Url>> {
    resolve_endpoints_with(src, &EndpointCtx::current())
}

pub fn resolve_endpoints_with(src: &UpdateSource, ctx: &EndpointCtx) -> Result<Vec<Url>> {
    match src.kind.trim() {
        "github" => {
            let repo = src
                .repo
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(DEFAULT_UPDATE_REPO);
            validate_github_repo(repo)?;
            let url = format!("https://github.com/{repo}/releases/latest/download/latest.json");
            Ok(vec![parse_https_url(&url)?])
        }
        "manifest" => {
            let raw = src
                .manifest_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| AppError::Invalid("未填写清单 URL".into()))?;
            let substituted = apply_placeholders(raw, ctx);
            Ok(vec![parse_https_url(&substituted)?])
        }
        other => Err(AppError::Invalid(format!("未知更新源类型：{other}"))),
    }
}

pub fn github_latest_json_url(repo: &str) -> Result<Url> {
    validate_github_repo(repo)?;
    parse_https_url(&format!(
        "https://github.com/{repo}/releases/latest/download/latest.json"
    ))
}

pub fn github_tag_json_url(repo: &str, tag: &str) -> Result<Url> {
    validate_github_repo(repo)?;
    let tag = tag.trim();
    if tag.is_empty() || tag.contains('/') || tag.contains('\\') {
        return Err(AppError::Invalid("无效的 Release 标签".into()));
    }
    parse_https_url(&format!(
        "https://github.com/{repo}/releases/download/{tag}/latest.json"
    ))
}

pub fn validate_github_repo(repo: &str) -> Result<()> {
    let repo = repo.trim();
    if repo.is_empty() || repo.len() > GITHUB_REPO_MAX_LEN {
        return Err(AppError::Invalid("仓库格式应为 owner/repo".into()));
    }
    let Some((owner, name)) = repo.split_once('/') else {
        return Err(AppError::Invalid("仓库格式应为 owner/repo".into()));
    };
    if owner.is_empty() || name.is_empty() || repo.matches('/').count() != 1 {
        return Err(AppError::Invalid("仓库格式应为 owner/repo".into()));
    }
    if !is_github_name(owner) || !is_github_name(name) {
        return Err(AppError::Invalid(
            "仓库名只允许字母、数字、点、下划线和连字符".into(),
        ));
    }
    Ok(())
}

fn is_github_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('.')
        && !s.ends_with('.')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn apply_placeholders(raw: &str, ctx: &EndpointCtx) -> String {
    raw.replace("{{target}}", &ctx.target)
        .replace("{{arch}}", &ctx.arch)
        .replace("{{current_version}}", &ctx.current_version)
}

fn parse_https_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).map_err(|e| AppError::Invalid(format!("清单 URL 无效：{e}")))?;
    if url.scheme() != "https" {
        return Err(AppError::Invalid("更新源必须使用 HTTPS".into()));
    }
    if url.host_str().is_none() {
        return Err(AppError::Invalid("清单 URL 缺少主机名".into()));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> EndpointCtx {
        EndpointCtx {
            target: "windows".into(),
            arch: "x86_64".into(),
            current_version: "1.1.0".into(),
        }
    }

    #[test]
    fn github_default_repo_resolves_latest_json() {
        let src = UpdateSource::default();
        let urls = resolve_endpoints_with(&src, &ctx()).unwrap();
        assert_eq!(
            urls[0].as_str(),
            "https://github.com/ice-juice/git-identity-manager-app/releases/latest/download/latest.json"
        );
    }

    #[test]
    fn github_custom_repo_resolves() {
        let src = UpdateSource {
            kind: "github".into(),
            repo: Some("acme/mirror".into()),
            manifest_url: None,
            include_prerelease: false,
        };
        let urls = resolve_endpoints_with(&src, &ctx()).unwrap();
        assert_eq!(
            urls[0].as_str(),
            "https://github.com/acme/mirror/releases/latest/download/latest.json"
        );
    }

    #[test]
    fn manifest_url_rejects_missing_and_http() {
        let missing = UpdateSource {
            kind: "manifest".into(),
            repo: None,
            manifest_url: None,
            include_prerelease: false,
        };
        assert!(resolve_endpoints_with(&missing, &ctx()).is_err());

        let http = UpdateSource {
            kind: "manifest".into(),
            repo: None,
            manifest_url: Some("http://mirror.example.com/latest.json".into()),
            include_prerelease: false,
        };
        let err = resolve_endpoints_with(&http, &ctx()).unwrap_err();
        assert!(err.to_string().contains("HTTPS"));
    }

    #[test]
    fn manifest_placeholders_are_substituted() {
        let src = UpdateSource {
            kind: "manifest".into(),
            repo: None,
            manifest_url: Some(
                "https://mirror.example.com/{{target}}/{{arch}}/{{current_version}}/latest.json"
                    .into(),
            ),
            include_prerelease: false,
        };
        let urls = resolve_endpoints_with(&src, &ctx()).unwrap();
        assert_eq!(
            urls[0].as_str(),
            "https://mirror.example.com/windows/x86_64/1.1.0/latest.json"
        );
    }

    #[test]
    fn unknown_kind_and_bad_repo_are_rejected() {
        let kind = UpdateSource {
            kind: "ftp".into(),
            repo: None,
            manifest_url: None,
            include_prerelease: false,
        };
        assert!(resolve_endpoints_with(&kind, &ctx()).is_err());

        for bad in ["justowner", "acme/../etc", "acme/foo/bar", "ac me/repo"] {
            let src = UpdateSource {
                kind: "github".into(),
                repo: Some(bad.into()),
                manifest_url: None,
                include_prerelease: false,
            };
            assert!(
                resolve_endpoints_with(&src, &ctx()).is_err(),
                "should reject {bad}"
            );
        }
    }

    #[test]
    fn effective_source_fills_default_from_empty_config() {
        let cfg = AppConfig::default();
        let src = effective_source(&cfg);
        assert_eq!(src.kind, "github");
        assert_eq!(src.repo.as_deref(), Some(DEFAULT_UPDATE_REPO));
    }
}
