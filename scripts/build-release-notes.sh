#!/usr/bin/env bash
# 实际抽取逻辑在 Node 脚本里，避免各平台 awk 处理中文标题不一致。
set -euo pipefail
dir="$(cd "$(dirname "$0")" && pwd)"
exec node "$dir/build-release-notes.mjs" "$@"
