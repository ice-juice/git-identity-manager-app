/**
 * 从 CHANGELOG 抽出三份功能清单，再拼上下载建议。
 * 用法：node scripts/build-release-notes.mjs X.Y.Z
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const TITLES = new Map([
  ["新增", "新增功能"],
  ["新增功能", "新增功能"],
  ["优化", "优化功能"],
  ["优化功能", "优化功能"],
  ["修复", "修复问题"],
  ["修复问题", "修复问题"],
  ["下载建议", "下载建议"],
]);

export function normalizeTitle(raw) {
  const title = String(raw || "")
    .replace(/\r/g, "")
    .trim();
  return TITLES.get(title) || "";
}

export function extractLists(markdown, version = "") {
  const lines = String(markdown || "").replace(/\r/g, "").split("\n");
  const out = [];
  let inVersion = version === "";
  let keep = false;
  let started = false;

  for (const line of lines) {
    if (version && /^## \[/.test(line)) {
      if (line.startsWith(`## [${version}]`)) {
        inVersion = true;
        keep = false;
        continue;
      }
      if (inVersion) {
        break;
      }
      continue;
    }
    if (!inVersion) {
      continue;
    }
    const heading = line.match(/^###[ \t]+(.+)$/);
    if (heading) {
      const mapped = normalizeTitle(heading[1]);
      keep = mapped !== "";
      if (keep) {
        if (started) {
          out.push("");
        }
        out.push(`### ${mapped}`);
        started = true;
      }
      continue;
    }
    if (keep && /^[-*][ \t]/.test(line)) {
      out.push(line);
    }
  }
  return out.join("\n");
}

export function buildReleaseNotes(version, { changelog, guide } = {}) {
  if (!version) {
    throw new Error("usage: build-release-notes.mjs X.Y.Z");
  }
  const notes = extractLists(changelog, version);
  if (!notes.replace(/\s/g, "")) {
    throw new Error(`CHANGELOG.md 缺少版本 ${version} 的说明（需要 ## [${version}] 下的新增功能 / 优化功能 / 修复问题）`);
  }
  let guideNotes = extractLists(guide, "");
  if (!guideNotes.replace(/\s/g, "")) {
    throw new Error("docs/release-download-guide.md 缺少「下载建议」清单");
  }
  guideNotes = guideNotes.split("{{VERSION}}").join(version);
  return `${notes}\n\n${guideNotes}\n`;
}

function runSelfTest() {
  const headings = ["新增功能", "优化功能", "修复问题", "下载建议"].map(normalizeTitle);
  if (headings.join(",") !== "新增功能,优化功能,修复问题,下载建议") {
    throw new Error(`title map broken: ${headings.join(",")}`);
  }
  const sample = buildReleaseNotes("9.9.9", {
    changelog: [
      "## [9.9.9] - 2026-01-01",
      "### 新增功能",
      "- 新",
      "### 优化功能",
      "- 优",
      "### 修复问题",
      "- 修",
    ].join("\n"),
    guide: ["### 下载建议", "- v{{VERSION}}"].join("\n"),
  });
  const got = [...sample.matchAll(/^### (.+)$/gm)].map((m) => m[1]);
  if (got.join(",") !== "新增功能,优化功能,修复问题,下载建议") {
    throw new Error(`notes headings: ${got.join(",")}`);
  }
  console.log("[notes] self-test ok");
}

const invoked = process.argv[1] && path.basename(process.argv[1]) === "build-release-notes.mjs";
if (invoked) {
  const arg = process.argv[2];
  if (arg === "--test") {
    runSelfTest();
  } else {
    const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
    process.stdout.write(
      buildReleaseNotes(arg, {
        changelog: fs.readFileSync(path.join(root, "CHANGELOG.md"), "utf8"),
        guide: fs.readFileSync(path.join(root, "docs/release-download-guide.md"), "utf8"),
      }),
    );
  }
}
