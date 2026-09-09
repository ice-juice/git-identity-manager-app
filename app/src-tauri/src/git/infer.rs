//! 身份推断：按置信度从高到低判断某仓库地址应使用哪个身份（§3.9）。
//!
//! 纯本地查表（归属标识/主机/账号名/历史）；API 与 `git ls-remote` 兜底
//! 在命令层触发，此处只做可离线判定的部分。

use crate::git::owners::{self, MatchKind};
use crate::git::url::ParsedRepo;
use crate::model::Identity;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    Certain,
    VeryHigh,
    MediumHigh,
    Low,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub identity_id: String,
    pub identity_name: String,
    pub host_alias: String,
    pub confidence: Confidence,
    /// 推断依据（中文，明写出来供用户核对）。
    pub basis: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Inference {
    /// 改写后的别名地址（有推荐项时）。
    pub rewritten_url: Option<String>,
    pub recommended: Option<Candidate>,
    pub candidates: Vec<Candidate>,
    /// 是否需要联网/实测兜底（本地无高置信结论）。
    pub needs_probe: bool,
}

/// 本地推断。`history` 为 owner(小写) → identity_id 的历史克隆记录。
pub fn infer(
    parsed: &ParsedRepo,
    identities: &[Identity],
    history: &HashMap<String, String>,
) -> Inference {
    let owner_lc = parsed.owner.to_lowercase();
    let mut candidates: Vec<Candidate> = Vec::new();

    // 若地址已是别名形式，优先校验该别名是否有效。
    if parsed.is_alias {
        if let Some(host) = &parsed.host {
            if let Some(id) = identities.iter().find(|i| i.host_alias.eq_ignore_ascii_case(host)) {
                let cand = mk(id, Confidence::Certain, format!("地址已是别名 `{host}`，对应身份"));
                return finalize(parsed, Some(cand.clone()), vec![cand]);
            }
        }
    }

    // 1) 归属标识精确 / 2) 通配。
    for id in identities {
        match owners::best_match(&id.owners, &owner_lc) {
            MatchKind::Exact => candidates.push(mk(
                id,
                Confidence::Certain,
                format!("owner `{}` 精确匹配身份 *{}* 的归属标识", parsed.owner, id.name),
            )),
            MatchKind::Wildcard => candidates.push(mk(
                id,
                Confidence::VeryHigh,
                format!("owner `{}` 命中身份 *{}* 的通配归属规则", parsed.owner, id.name),
            )),
            MatchKind::None => {}
        }
    }

    // 3) 主机唯一匹配（仅当地址带真实主机、且只有一个身份配了它）。
    if candidates.is_empty() {
        if let Some(host) = &parsed.host {
            if !parsed.is_alias {
                let hits: Vec<&Identity> = identities
                    .iter()
                    .filter(|i| i.real_host.eq_ignore_ascii_case(host))
                    .collect();
                if hits.len() == 1 {
                    candidates.push(mk(
                        hits[0],
                        Confidence::Certain,
                        format!("主机 `{host}` 唯一对应该身份"),
                    ));
                }
            }
        }
    }

    // 4) owner 首段等于账号名。
    if candidates.is_empty() {
        let first_seg = owner_lc.split('/').next().unwrap_or(&owner_lc);
        for id in identities {
            if id.name.eq_ignore_ascii_case(first_seg) {
                candidates.push(mk(
                    id,
                    Confidence::VeryHigh,
                    format!("owner `{}` 等于身份 *{}* 的实测账号名", parsed.owner, id.name),
                ));
            }
        }
    }

    // 5) 历史记录。
    if candidates.is_empty() {
        if let Some(id_ref) = history.get(&owner_lc) {
            if let Some(id) = identities.iter().find(|i| &i.id == id_ref) {
                candidates.push(mk(
                    id,
                    Confidence::MediumHigh,
                    format!("以前用身份 *{}* 克隆过 owner `{}`", id.name, parsed.owner),
                ));
            }
        }
    }

    // 排序：置信度高在前。
    candidates.sort_by_key(|c| conf_rank(c.confidence));

    let recommended = candidates.first().cloned();
    finalize(parsed, recommended, candidates)
}

fn finalize(parsed: &ParsedRepo, recommended: Option<Candidate>, candidates: Vec<Candidate>) -> Inference {
    let rewritten_url = recommended
        .as_ref()
        .map(|c| crate::git::url::rewrite_to_alias(&c.host_alias, &parsed.repo_path));
    let needs_probe = recommended.is_none();
    Inference {
        rewritten_url,
        recommended,
        candidates,
        needs_probe,
    }
}

fn mk(id: &Identity, confidence: Confidence, basis: String) -> Candidate {
    Candidate {
        identity_id: id.id.clone(),
        identity_name: id.name.clone(),
        host_alias: id.host_alias.clone(),
        confidence,
        basis,
    }
}

fn conf_rank(c: Confidence) -> u8 {
    match c {
        Confidence::Certain => 0,
        Confidence::VeryHigh => 1,
        Confidence::MediumHigh => 2,
        Confidence::Low => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::url::parse_repo_url;

    fn id(name: &str, alias: &str, host: &str, owners: &[&str]) -> Identity {
        Identity {
            id: format!("id-{name}"),
            name: name.into(),
            platform: "github".into(),
            host_alias: alias.into(),
            real_host: host.into(),
            user: "git".into(),
            email: None,
            git_user_name: None,
            key_id: None,
            owners: owners.iter().map(|s| s.to_string()).collect(),
            strict_mode: false,
            updated_at: String::new(),
        }
    }

    #[test]
    fn exact_owner_match_wins_and_rewrites() {
        let ids = vec![
            id("techn4950", "github-techn", "github.com", &["vortaq-trad", "mgcp-afk"]),
            id("juice520", "github.com", "github.com", &[]),
        ];
        let parsed = parse_repo_url("https://github.com/vortaq-trad/vq-officail.git").unwrap();
        let inf = infer(&parsed, &ids, &HashMap::new());
        let rec = inf.recommended.unwrap();
        assert_eq!(rec.identity_name, "techn4950");
        assert_eq!(rec.confidence, Confidence::Certain);
        assert_eq!(
            inf.rewritten_url.as_deref(),
            Some("git@github-techn:vortaq-trad/vq-officail.git")
        );
        assert!(!inf.needs_probe);
    }

    #[test]
    fn wildcard_match() {
        let ids = vec![id("techn4950", "github-techn", "github.com", &["vortaq-*"])];
        let parsed = parse_repo_url("https://github.com/vortaq-newteam/x.git").unwrap();
        let inf = infer(&parsed, &ids, &HashMap::new());
        assert_eq!(inf.recommended.unwrap().confidence, Confidence::VeryHigh);
    }

    #[test]
    fn host_unique_match_for_self_hosted() {
        let ids = vec![
            id("boxadmin", "git-box", "ssh.boxexchanger.net", &[]),
            id("juice520", "github.com", "github.com", &[]),
        ];
        let parsed = parse_repo_url("git@ssh.boxexchanger.net:bx4/vchangr-com/web.git").unwrap();
        let inf = infer(&parsed, &ids, &HashMap::new());
        let rec = inf.recommended.unwrap();
        assert_eq!(rec.identity_name, "boxadmin");
        assert_eq!(rec.confidence, Confidence::Certain);
        assert_eq!(
            inf.rewritten_url.as_deref(),
            Some("git@git-box:bx4/vchangr-com/web.git")
        );
    }

    #[test]
    fn owner_equals_account_name() {
        // 真实多账号：两把都在 github.com，主机不唯一，需靠账号名区分。
        let ids = vec![
            id("octocat", "github-octo", "github.com", &[]),
            id("someoneelse", "github-else", "github.com", &[]),
        ];
        let parsed = parse_repo_url("https://github.com/octocat/hello.git").unwrap();
        let inf = infer(&parsed, &ids, &HashMap::new());
        let rec = inf.recommended.unwrap();
        assert_eq!(rec.identity_name, "octocat");
        assert_eq!(rec.confidence, Confidence::VeryHigh);
    }

    #[test]
    fn history_fallback() {
        let ids = vec![
            id("techn4950", "github-techn", "github.com", &[]),
            id("other", "github-other", "github.com", &[]),
        ];
        let mut history = HashMap::new();
        history.insert("someorg".to_string(), "id-techn4950".to_string());
        let parsed = parse_repo_url("https://github.com/someorg/repo.git").unwrap();
        let inf = infer(&parsed, &ids, &history);
        let rec = inf.recommended.unwrap();
        assert_eq!(rec.identity_name, "techn4950");
        assert_eq!(rec.confidence, Confidence::MediumHigh);
    }

    #[test]
    fn no_match_needs_probe() {
        let ids = vec![
            id("techn4950", "github-techn", "github.com", &["vortaq-*"]),
            id("other", "github-other", "github.com", &[]),
        ];
        let parsed = parse_repo_url("https://github.com/unknownorg/repo.git").unwrap();
        let inf = infer(&parsed, &ids, &HashMap::new());
        assert!(inf.recommended.is_none());
        assert!(inf.needs_probe);
        assert!(inf.rewritten_url.is_none());
    }

    #[test]
    fn single_identity_host_unique_still_recommends() {
        // 只有一个 github.com 身份时，主机唯一即可给出确定推荐（符合置信度链）。
        let ids = vec![id("solo", "github-solo", "github.com", &[])];
        let parsed = parse_repo_url("https://github.com/anyowner/repo.git").unwrap();
        let inf = infer(&parsed, &ids, &HashMap::new());
        let rec = inf.recommended.unwrap();
        assert_eq!(rec.identity_name, "solo");
        assert_eq!(rec.confidence, Confidence::Certain);
    }

    #[test]
    fn already_alias_is_validated() {
        let ids = vec![id("techn4950", "github-techn", "github.com", &[])];
        let parsed = parse_repo_url("git@github-techn:owner/repo.git").unwrap();
        let inf = infer(&parsed, &ids, &HashMap::new());
        assert_eq!(inf.recommended.unwrap().identity_name, "techn4950");
    }

    #[test]
    fn exact_precise_beats_wildcard_from_other_identity() {
        let ids = vec![
            id("a", "gh-a", "github.com", &["vortaq-*"]),
            id("b", "gh-b", "github.com", &["vortaq-trad"]),
        ];
        let parsed = parse_repo_url("https://github.com/vortaq-trad/x.git").unwrap();
        let inf = infer(&parsed, &ids, &HashMap::new());
        // 精确匹配的 b 应排在前。
        assert_eq!(inf.recommended.unwrap().identity_name, "b");
    }
}
