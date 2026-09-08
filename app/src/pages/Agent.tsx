import { useEffect, useState } from "react";
import { api, errMessage, type AgentStatus } from "../lib/ipc";
import { PageHead, Card, Empty, Badge } from "../ui/common";

export function AgentPage() {
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);

  async function load() {
    setErr("");
    try {
      setStatus(await api.agentStatus());
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  useEffect(() => {
    load();
  }, []);

  async function run(fn: () => Promise<unknown>, ok: string) {
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      await fn();
      setMsg(ok);
      await load();
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="stack-lg">
      <PageHead
        title="Agent 管理"
        desc="查看 ssh-agent 已加载的密钥，并与库内身份反查匹配"
        actions={
          <>
            <button className="btn" disabled={busy} onClick={() => run(() => api.agentEnsure(), "已确保 agent 运行")}>
              确保运行
            </button>
            <button
              className="btn"
              disabled={busy}
              onClick={() => run(async () => setMsg(`已加载 ${await api.agentLoadAll()} 个密钥`), "")}
            >
              加载全部
            </button>
            <button className="btn danger" disabled={busy} onClick={() => run(() => api.agentClear(), "已清空 agent")}>
              清空
            </button>
          </>
        }
      />
      {err && <div className="err-text">{err}</div>}
      {msg && <div className="callout info">{msg}</div>}

      <Card>
        <div className="row" style={{ marginBottom: 12, gap: 8 }}>
          <Badge kind={status?.running ? "good" : "danger"}>{status?.running ? "运行中" : "未运行"}</Badge>
          {status?.usingFallback && <Badge kind="warn">使用 Git 内置 agent</Badge>}
        </div>
        {!status || status.keys.length === 0 ? (
          <Empty icon="🧩" text="agent 中没有已加载的密钥。" />
        ) : (
          <div className="list">
            {status.keys.map((k, i) => (
              <div className="list-row" key={i}>
                <div className="grow">
                  <div className="row" style={{ gap: 8 }}>
                    <strong>{k.identityName ?? k.keyName ?? "未知密钥"}</strong>
                    <Badge kind="info">{k.agent.algo}</Badge>
                    {!k.identityName && !k.keyName && <Badge kind="warn">库外</Badge>}
                  </div>
                  <div className="mono muted sm">
                    {k.agent.fingerprint} {k.agent.comment && `· ${k.agent.comment}`}
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </Card>
    </div>
  );
}
