import { useEffect, useState } from "react";
import { api, errMessage, type ConfigView } from "../lib/ipc";
import { PageHead, Card, Empty, Badge } from "../ui/common";

export function ConfigPage() {
  const [view, setView] = useState<ConfigView | null>(null);
  const [err, setErr] = useState("");

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
  }, []);

  return (
    <div className="stack-lg">
      <PageHead
        title="SSH 配置"
        desc="解析 ~/.ssh/config，健康检查仅提示不自动改动用户手写区"
        actions={<button className="btn ghost" onClick={load}>刷新</button>}
      />
      {err && <div className="err-text">{err}</div>}

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

      <Card title="原始内容">
        <pre className="code-block">{view?.raw || "（空）"}</pre>
      </Card>
    </div>
  );
}
