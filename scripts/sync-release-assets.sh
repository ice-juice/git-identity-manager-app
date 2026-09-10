#!/usr/bin/env bash
# 把带语言后缀的安装包同步到 GitHub Release，并删掉旧文件名。
set -euo pipefail

tag="${1:-}"
if [[ -z "$tag" || "$tag" == "v__VERSION__" ]]; then
  echo "skip release asset sync (no real tag)"
  exit 0
fi

if [[ ! -f renamed-release-assets.json ]]; then
  echo "no renamed-release-assets.json, nothing to sync"
  exit 0
fi

mapfile -t pairs < <(node -e '
  const m = JSON.parse(require("fs").readFileSync("renamed-release-assets.json","utf8"));
  for (const [from, to] of m) console.log(from + "\t" + to);
')

if [[ ${#pairs[@]} -eq 0 ]]; then
  echo "rename mapping empty"
  exit 0
fi

shopt -s globstar nullglob
uploads=()
while IFS= read -r file; do
  [[ -f "$file" ]] && uploads+=("$file")
done < <(node -e '
  const fs = require("fs");
  const path = require("path");
  const mapping = JSON.parse(fs.readFileSync("renamed-release-assets.json","utf8"));
  const names = new Set(mapping.map(([, to]) => to));
  function walk(dir) {
    if (!fs.existsSync(dir)) return;
    for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, ent.name);
      if (ent.isDirectory()) walk(p);
      else if (names.has(ent.name) || ent.name === "latest.json") console.log(p);
    }
  }
  walk("app/src-tauri/target/release/bundle");
  walk("app/src-tauri/target/universal-apple-darwin/release/bundle");
  walk("app/src-tauri/target");
')

# 仓库已从 git-identity-manager-app 改名；旧名会 301，上传带 body 的请求会失败。
repo="ice-juice/git-keymaster-app"

if [[ ${#uploads[@]} -gt 0 ]]; then
  gh release upload "$tag" "${uploads[@]}" --repo "$repo" --clobber
fi

if gh release download "$tag" --repo "$repo" --pattern latest.json --dir /tmp/gam-latest --clobber 2>/dev/null; then
  if [[ -f /tmp/gam-latest/latest.json ]]; then
    node -e '
      const fs = require("fs");
      const mapping = JSON.parse(fs.readFileSync("renamed-release-assets.json","utf8"));
      let text = fs.readFileSync("/tmp/gam-latest/latest.json","utf8");
      for (const [from, to] of mapping) {
        if (from !== to) text = text.split(from).join(to);
      }
      fs.writeFileSync("/tmp/gam-latest/latest.json", text);
    '
    gh release upload "$tag" /tmp/gam-latest/latest.json --repo "$repo" --clobber
  fi
fi

for pair in "${pairs[@]}"; do
  old="${pair%%$'\t'*}"
  new="${pair#*$'\t'}"
  if [[ -n "$old" && "$old" != "$new" ]]; then
    gh release delete-asset "$tag" "$old" --repo "$repo" --yes 2>/dev/null || true
  fi
done
