#!/usr/bin/env bash
# macOS 自带 bash 3.2 没有 mapfile；实际同步逻辑在 Node 脚本里。
set -euo pipefail
dir="$(cd "$(dirname "$0")" && pwd)"
exec node "$dir/sync-release-assets.mjs" "$@"
