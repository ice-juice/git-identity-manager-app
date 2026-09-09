import { useEffect, useState } from "react";
import { api, errMessage, type AgentStatus, type AgentUnifyReport } from "../lib/ipc";
import { PageHead, Card, Empty, Badge } from "../ui/common";
import { EnvUnifyDialog } from "../ui/EnvUnifyDialog";

export function AgentPage() {
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);
  const [scanning, setScanning] = useState(true);
  const [report, setReport] = useState<AgentUnifyReport | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);

  async function detect() {
    setErr("");
    setScanning(true);
    try {
      setStatus(await api.agentStatus());
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setScanning(false);
    }
  }

  useEffect(() => {
    detect();
  }, []);

  async function run(fn: () => Promise<unknown>, ok: string) {
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      await fn();
      if (ok) setMsg(ok);
      await detect();
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const unify = status?.unify;
  const needsApply = unify && !unify.aligned;
  const checks = unify?.checks ?? [];
  const failCount = checks.filter((c) => !c.ok).length;

  return (
    <div className="stack-lg">
      <PageHead
        title="Agent 管理"
        desc="检测本机是否已用同一套 Git 自带 ssh-agent；密钥加载情况与终端、git 是否对齐都会列在下面。"
        actions={
          <>
            <button className="btn" disabled={busy || scanning} onClick={() => detect()}>
              {scanning ? "检测中…" : "重新检测"}
            </button>
            <button className="btn" disabled={busy} onClick={() => run(() => api.agentEnsure(), "已确保 Git ssh-agent 运行")}>
              确保运行
            </button>
            <button className="btn" disabled={busy} onClick={() => setConfirmOpen(true)}>
              应用到本机环境
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
      {err && !confirmOpen && <div className="err-text">{err}</div>}
      {msg && <div className="callout info">{msg}</div>}

      <Card
        title="本机环境检测"
        actions={
          unify ? (
            <Badge kind={unify.aligned ? "good" : "warn"}>
              {scanning ? "检测中" : unify.aligned ? "已使用 Git 内置 agent" : `${failCount} 项未就绪`}
            </Badge>
          ) : null
        }
      >
        {scanning && !status ? (
          <div className="muted sm">正在检测 Git ssh、ssh-agent、用户环境变量与终端启动脚本…</div>
        ) : (
          <>
            <div className="row" style={{ marginBottom: 12, gap: 8 }}>
              <Badge kind={status?.running ? "good" : "danger"}>{status?.running ? "Agent 运行中" : "Agent 未运行"}</Badge>
              {status?.usingFallback && <Badge kind="good">当前进程：Git 内置 agent</Badge>}
            </div>

            {needsApply && (
              <div className="callout warn" style={{ marginBottom: 12 }}>
                <div>
                  本机终端和 <span className="mono">git</span> 还没有完整接到 Git 自带 ssh-agent。Cursor / PowerShell
                  里的 <span className="mono">ssh</span> 可能仍走系统 OpenSSH，严格模式密钥会加载失败。
                  是否把当前 Git agent 应用到本机环境，由你决定；不会自动改系统。
                </div>
                <button type="button" className="btn sm" disabled={busy} onClick={() => setConfirmOpen(true)}>
                  应用到本机环境
                </button>
              </div>
            )}

            {unify?.aligned && (
              <div className="callout good" style={{ marginBottom: 12 }}>
                检测通过：git 全局配置、用户环境变量和终端启动脚本已指向同一套 Git ssh-agent。
              </div>
            )}

            {checks.length > 0 ? (
              <div className="list env-check-list">
                <div className="env-check-head">
                  <span>检测项</span>
                  <span>结果</span>
                  <span>当前值</span>
                </div>
                {checks.map((c) => (
                  <div className="list-row env-check-row" key={c.key}>
                    <div className="env-check-label">{c.label}</div>
                    <Badge kind={c.ok ? "good" : "warn"}>{c.ok ? "通过" : "未配置"}</Badge>
                    <div className="mono muted sm env-check-value">{c.current || "（空）"}</div>
                  </div>
                ))}
              </div>
            ) : (
              <Empty icon="🔍" text="尚未取得检测结果，请点「重新检测」。" />
            )}

            {report && report.steps.length > 0 && (
              <div className="callout info" style={{ marginTop: 12 }}>
                {report.steps.map((s, i) => (
                  <div key={i} className="mono sm">
                    {s}
                  </div>
                ))}
              </div>
            )}
          </>
        )}
      </Card>

      <Card title="已加载密钥">
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

      <EnvUnifyDialog
        open={confirmOpen}
        busy={busy}
        error={confirmOpen ? err : undefined}
        ssh={status?.ssh || unify?.gitSsh}
        sock={status?.authSock || unify?.authSock}
        onCancel={() => setConfirmOpen(false)}
        onConfirm={() =>
          run(async () => {
            const r = await api.agentUnifyEnv(true);
            setReport(r);
            setMsg(r.hint);
            setConfirmOpen(false);
          }, "")
        }
      />
    </div>
  );
}
