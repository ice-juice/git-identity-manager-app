import { useEffect, useState } from "react";
import { api, errMessage, type ConfigView } from "../lib/ipc";
import { PageHead, Card, Empty, Badge } from "../ui/common";
import { useApp } from "../store";

export function ConfigPage() {
  const [view, setView] = useState<ConfigView | null>(null);
  const [err, setErr] = useState("");
  const { writesLocked } = useApp();

  async function load() {
    setErr("");
    try {
      setView(await api.readSshConfig());
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  useEffect(() => {
    load();
  }, [writesLocked]);

  return (
    <div className="stack-lg">
      <PageHead
        title="SSH 配置"
        desc="真实配置在工作空间 ssh/config。本机 ~/.ssh/config 只保留 Include，供系统 ssh/git 自动加载。"
        actions={
          <div className="row" style={{ gap: 6 }}>
            <button
              className="btn primary"
              disabled={writesLocked}
              onClick={async () => {
                setErr("");
                try {
                  await api.openSshConfig();
                } catch (e) {
                  setErr(errMessage(e));
                }
              }}
            >
              用记事本打开并编辑
            </button>
            <button className="btn ghost" onClick={load}>刷新</button>
          </div>
        }
      />
      {err && <div className="err-text">{err}</div>}
      {view && (
        <div className="muted sm">
          工作空间正本（下面 Host / 原文都来自这里）：<span className="mono">{view.path}</span>
          <br />
          系统入口 ~/.ssh/config：<span className="mono">{view.systemPath}</span>
          {view.registered ? "（只含 Include，供 ssh/git 加载正本）" : "（尚未注册 Include，解锁后会自动写入）"}
        </div>
      )}

      <Card title="健康检查">
        {!view || view.diagnostics.length === 0 ? (
          <div className="callout good">✅ 未发现明显问题</div>
        ) : (
          <div className="list">
            {view.diagnostics.map((d, i) => (
              <div className="list-row" key={i}>
                <div className="grow">
                  <div className="row" style={{ gap: 8 }}>
                    <Badge kind={d.severity === "error" ? "danger" : "warn"}>{d.severity}</Badge>
                    {d.host && <span className="mono sm">{d.host}</span>}
                  </div>
                  <div className="muted sm">{d.message}</div>
                </div>
              </div>
            ))}
          </div>
        )}
      </Card>

      <Card title={`Host 块（${view?.blocks.length ?? 0}）`}>
        {!view || view.blocks.length === 0 ? (
          <Empty icon="📄" text="config 中还没有 Host 块。" />
        ) : (
          <div className="list">
            {view.blocks.map((b, i) => (
              <div className="list-row" key={i}>
                <div className="grow">
                  <div className="mono">{b.patterns.join(" ")}</div>
                  <div className="mono muted sm">
                    {b.options.map(([k, v]) => `${k} ${v}`).join("  ·  ")}
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </Card>

      <Card title="工作空间正本内容">
        <pre className="code-block">{view?.raw || "（空）"}</pre>
      </Card>
    </div>
  );
}
