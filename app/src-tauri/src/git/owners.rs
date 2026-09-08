//! 归属标识匹配：大小写不敏感、支持通配 `vortaq-*`、支持多段路径前缀 `bx4/vchangr-com`。

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchKind {
    /// 精确/路径前缀匹配（用户显式声明的意图）。
    Exact,
    /// 通配匹配。
    Wildcard,
    None,
}

/// 判断某条已登记归属标识是否命中目标 owner 路径。
pub fn owner_matches(registered: &str, target_owner: &str) -> MatchKind {
    let reg = registered.trim().to_lowercase();
    let target = target_owner.trim().to_lowercase();
    if reg.is_empty() || target.is_empty() {
        return MatchKind::None;
    }
    if let Some(prefix) = reg.strip_suffix('*') {
        let prefix = prefix.trim_end_matches('/');
        // `vortaq-*` → 前缀匹配；`*` 匹配一切。
        if prefix.is_empty() || target == prefix || target.starts_with(prefix) {
            return MatchKind::Wildcard;
        }
        return MatchKind::None;
    }
    // 精确或路径前缀（`bx4/vchangr-com` 命中 `bx4/vchangr-com` 或其子路径）。
    if target == reg || target.starts_with(&format!("{reg}/")) {
        return MatchKind::Exact;
    }
    MatchKind::None
}

/// 在一组归属标识中找最强匹配（Exact 优先于 Wildcard）。
pub fn best_match(registered_list: &[String], target_owner: &str) -> MatchKind {
    let mut best = MatchKind::None;
    for r in registered_list {
        match owner_matches(r, target_owner) {
            MatchKind::Exact => return MatchKind::Exact,
            MatchKind::Wildcard => best = MatchKind::Wildcard,
            MatchKind::None => {}
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_case_insensitive() {
        assert_eq!(owner_matches("vortaq-trad", "Vortaq-Trad"), MatchKind::Exact);
    }

    #[test]
    fn wildcard_prefix() {
        assert_eq!(owner_matches("vortaq-*", "vortaq-trad"), MatchKind::Wildcard);
        assert_eq!(owner_matches("vortaq-*", "vortaq-new-org"), MatchKind::Wildcard);
        assert_eq!(owner_matches("vortaq-*", "othergroup"), MatchKind::None);
    }

    #[test]
    fn multi_segment_path_prefix() {
        assert_eq!(owner_matches("bx4/vchangr-com", "bx4/vchangr-com"), MatchKind::Exact);
        // 登记父前缀命中子路径。
        assert_eq!(owner_matches("bx4", "bx4/vchangr-com"), MatchKind::Exact);
    }

    #[test]
    fn no_partial_segment_false_positive() {
        // "vortaq" 不应精确命中 "vortaq-trad"（不是路径前缀）。
        assert_eq!(owner_matches("vortaq", "vortaq-trad"), MatchKind::None);
    }

    #[test]
    fn best_match_prefers_exact() {
        let list = vec!["vortaq-*".to_string(), "vortaq-trad".to_string()];
        assert_eq!(best_match(&list, "vortaq-trad"), MatchKind::Exact);
        assert_eq!(best_match(&list, "vortaq-other"), MatchKind::Wildcard);
    }
}
