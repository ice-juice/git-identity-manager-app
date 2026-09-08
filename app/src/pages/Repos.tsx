import { useState } from "react";
import { api, errMessage, type Inference, type RepoInfo } from "../lib/ipc";
import { PageHead, Card, Empty, Badge } from "../ui/common";

const CONF_LABEL: Record<string, string> = {
  certain: "确定",
  veryHigh: "很高",
  mediumHigh: "较高",
  low: "低",
};

export function Repos() {
  const [url, setUrl] = useState("");
  const [inf, setInf] = useState<Inference | null>(null);
  const [err, setErr] = useState("");
  const [root, setRoot] = useState("");
  const [repos, setRepos] = useState<RepoInfo[]>([]);
  const [busy, setBusy] = useState(false);

  async function resolve() {
    setErr("");
    setInf(null);
    if (!url.trim()) return;
    try {
      setInf(await api.resolveUrl(url));
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  async function scan() {
    setErr("");
    if (!root.trim()) return setErr("请填写扫描根目录");
    setBusy(true);
    try {
      setRepos(await api.scanRepos(root, 3));
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="stack-lg">
      <PageHead title="仓库与地址" desc="智能识别仓库地址对应的身份并改写为别名 URL" />
      {err && <div className="err-text">{err}</div>}

      <Card title="地址智能识别">
        <div className="stack">
          <div className="row">
            <input
              className="input mono grow"
              placeholder="粘贴仓库地址：git@github.com:owner/repo.git 或 https://github.com/owner/repo"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && resolve()}
            />
            <button className="btn primary" onClick={resolve}>
              识别
            </button>
          </div>

          {inf && (
            <div className="stack">
              {inf.rewrittenUrl && (
                <div className="callout good">
                  <div className="row" style={{ justifyContent: "space-between" }}>
                    <span className="mono">{inf.rewrittenUrl}</span>
                    <button className="btn sm" onClick={() => navigator.clipboard.writeText(inf.rewrittenUrl!)}>
                      复制
                    </button>
                  </div>
                </div>
              )}
              {inf.recommended && (
                <div className="callout info">
                  推荐身份：<strong>{inf.recommended.identityName}</strong>（别名 {inf.recommended.hostAlias}）
                  <Badge kind="good">{CONF_LABEL[inf.recommended.confidence]}</Badge>
                  <div className="muted sm">依据：{inf.recommended.basis}</div>
                </div>
              )}
              {inf.candidates.length > 1 && (
                <div className="list">
                  {inf.candidates.map((c) => (
                    <div className="list-row" key={c.identityId}>
                      <div className="grow">
                        <strong>{c.identityName}</strong>
                        <span className="mono muted sm"> · {c.hostAlias}</span>
                      </div>
                      <Badge>{CONF_LABEL[c.confidence]}</Badge>
                      <span className="muted sm">{c.basis}</span>
                    </div>
                  ))}
                </div>
              )}
              {inf.needsProbe && <div className="callout warn">⚠️ 无法确定身份，建议配置归属标识或用 PAT 探测。</div>}
              {!inf.recommended && !inf.needsProbe && <div className="muted">未匹配到身份。</div>}
            </div>
          )}
        </div>
      </Card>

      <Card
        title="本地仓库扫描"
        actions={
          <div className="row">
            <input
              className="input mono"
              style={{ width: 280 }}
              placeholder="扫描根目录"
              value={root}
              onChange={(e) => setRoot(e.target.value)}
            />
            <button className="btn" disabled={busy} onClick={scan}>
              {busy ? "扫描中…" : "扫描"}
            </button>
          </div>
        }
      >
        {repos.length === 0 ? (
          <Empty icon="📦" text="填写根目录后点击扫描，查看各仓库的 remote 与推断身份。" />
        ) : (
          <div className="list">
            {repos.map((r) => (
              <div className="list-row" key={r.path}>
                <div className="grow">
                  <div className="mono sm">{r.path}</div>
                  <div className="mono muted sm">{r.remoteUrl ?? "无 remote"}</div>
                  {r.needsAliasFix && r.fixCommand && (
                    <div className="callout warn sm" style={{ marginTop: 6 }}>
                      建议：<span className="mono">{r.fixCommand}</span>
                    </div>
                  )}
                </div>
                {r.inferredIdentity && <Badge kind="info">{r.inferredIdentity}</Badge>}
              </div>
            ))}
          </div>
        )}
      </Card>
    </div>
  );
}
