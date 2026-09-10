#!/usr/bin/env bash
# 从 CHANGELOG 抽出三份功能清单，再拼上下载建议。
# 应用内更新说明与 GitHub Release notes 只保留这四份清单。
set -euo pipefail

version="${1:-}"
if [[ -z "$version" ]]; then
  echo "usage: build-release-notes.sh X.Y.Z" >&2
  exit 1
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
changelog="$root/CHANGELOG.md"
guide="$root/docs/release-download-guide.md"

if [[ ! -f "$changelog" ]]; then
  echo "::error::缺少 CHANGELOG.md，无法写入升级说明" >&2
  exit 1
fi
if [[ ! -f "$guide" ]]; then
  echo "::error::缺少 docs/release-download-guide.md，无法写入下载建议" >&2
  exit 1
fi

extract_lists() {
  local file="$1"
  local ver="${2:-}"
  awk -v ver="$ver" '
    function norm(t) {
      gsub(/\r/, "", t)
      gsub(/^[ \t]+|[ \t]+$/, "", t)
      if (t == "新增" || t == "新增功能") return "新增功能"
      if (t == "优化" || t == "优化功能") return "优化功能"
      if (t == "修复" || t == "修复问题") return "修复问题"
      if (t == "下载建议") return "下载建议"
      return ""
    }
    {
      gsub(/\r/, "")
    }
    ver != "" && $0 ~ "^## \\[" ver "\\]" { inver=1; next }
    ver != "" && inver && /^## \[/ { exit }
    ver != "" && !inver { next }
    /^###[ \t]+/ {
      title=$0
      sub(/^###[ \t]+/, "", title)
      mapped=norm(title)
      keep=(mapped != "")
      if (keep) {
        if (started) printf "\n"
        printf "### %s\n", mapped
        started=1
      }
      next
    }
    keep && /^[-*][ \t]/ { print; next }
  ' "$file"
}

notes="$(extract_lists "$changelog" "$version")"
guide_notes="$(extract_lists "$guide" "")"
guide_notes="${guide_notes//\{\{VERSION\}\}/$version}"

if [[ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]]; then
  echo "::error::CHANGELOG.md 缺少版本 ${version} 的说明（需要 ## [${version}] 下的新增功能 / 优化功能 / 修复问题）" >&2
  exit 1
fi
if [[ -z "$(printf '%s' "$guide_notes" | tr -d '[:space:]')" ]]; then
  echo "::error::docs/release-download-guide.md 缺少「下载建议」清单" >&2
  exit 1
fi

printf '%s\n\n%s\n' "$notes" "$guide_notes"
