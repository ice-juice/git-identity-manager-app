import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderGit2, X, AlertTriangle } from "lucide-react";
import {
  api,
  errMessage,
  type Inference,
  type ClonePlan,
  type CloneResult,
  type Candidate,
} from "../lib/ipc";
import { PageHead, Card, Badge } from "../ui/common";
import { writeClipboard } from "../lib/clipboard";
import { useApp } from "../store";

const CONF_LABEL: Record<string, string> = {
  certain: "确定",
  veryHigh: "很高",
  mediumHigh: "较高",
  low: "低",
};

function guessRepoName(url: string): string {
  const cleaned = url.trim().replace(/\.git$/i, "").replace(/[\\/]+$/, "");
  const parts = cleaned.split(/[/:]/).filter(Boolean);
  return parts[parts.length - 1] || "repo";
}

function modeLabel(mode: string): string {
  if (mode === "clone") return "git clone";
  if (mode === "init") return "git init + 绑定远程";
  if (mode === "addRemote") return "绑定 origin 远程";
  return mode;
}

export function ClonePage() {
  const { writesLocked } = useApp();
  const nav = useNavigate();
  const [url, setUrl] = useState("");
  const [inf, setInf] = useState<Inference | null>(null);
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");
  const [cloneBusy, setCloneBusy] = useState(false);
  const [pickedIdentityId, setPickedIdentityId] = useState("");
  const [pending, setPending] = useState<{
    destDir: string;
    plan: ClonePlan;
    identity: Candidate;
    cloneUrl: string;
  } | null>(null);
  const [lastClone, setLastClone] = useState<CloneResult | null>(null);

  const identityOptions = useMemo(() => {
    if (!inf) return [];
    if (inf.candidates.length > 0) return inf.candidates;
    return inf.recommended ? [inf.recommended] : [];
  }, [inf]);

  const activeIdentity =
    identityOptions.find((c) => c.identityId === pickedIdentityId) ?? inf?.recommended ?? identityOptions[0] ?? null;

  async function resolve() {
    setErr("");
    setMsg("");
    setInf(null);
    setLastClone(null);
    setPending(null);
    if (!url.trim()) return;
    try {
      const result = await api.resolveUrl(url);
      setInf(result);
      setPickedIdentityId(result.recommended?.identityId ?? result.candidates[0]?.identityId ?? "");
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  async function startClone() {
    setErr("");
    setMsg("");
    if (!inf) return setErr("请先识别仓库地址");
    if (!activeIdentity) return setErr("未匹配到身份，无法用别名地址克隆。请先新建身份或配置归属标识。");

    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "选择克隆或初始化的目标文件夹",
      });
      if (typeof selected !== "string" || !selected) return;

      const repoName = guessRepoName(inf.rewrittenUrl || url);
      const plan = await api.inspectCloneTarget(selected, repoName);
      setPending({
        destDir: selected,
        plan,
        identity: activeIdentity,
        cloneUrl: inf.rewrittenUrl || url.trim(),
      });
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  async function confirmClone() {
    if (!pending) return;
    if (!pending.plan.canProceed) {
      setPending(null);
      return;
    }
    setCloneBusy(true);
    setErr("");
    try {
      const result = await api.cloneRepo({
        url: pending.cloneUrl,
        destDir: pending.destDir,
        identityId: pending.identity.identityId,
        mode: pending.plan.suggestedMode,
      });
      setLastClone(result);
      setMsg(`已完成并加入仓库列表：${result.dest}`);
      setPending(null);
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setCloneBusy(false);
    }
  }

  return (
    <div className="stack-lg">
      <PageHead title="克隆仓库" desc="识别地址、按身份改写别名 URL，再克隆到空目录或初始化已有项目" />
      {err && <div className="err-text">{err}</div>}
      {msg && <div className="callout good">{msg}</div>}

      <Card title="地址智能识别">
        <div className="stack">
          <div className="path-pick">
            <input
              className="input mono"
              placeholder="git@github.com:owner/repo.git 或 https://github.com/owner/repo"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && resolve()}
            />
            <button type="button" className="btn primary" onClick={resolve}>
              识别
            </button>
          </div>

          {inf && (
            <div className="stack">
              {inf.rewrittenUrl && (
                <div className="callout good between">
                  <span className="mono grow">{inf.rewrittenUrl}</span>
                  <button
                    type="button"
                    className="btn sm"
                    onClick={() => writeClipboard(inf.rewrittenUrl!)}
                  >
                    复制
                  </button>
                </div>
              )}
              {inf.recommended && (
                <div className="callout info">
                  <span>
                    推荐身份：<strong>{inf.recommended.identityName}</strong>
                    （别名 {inf.recommended.hostAlias}）
                  </span>
                  <Badge kind="good">{CONF_LABEL[inf.recommended.confidence]}</Badge>
                  <div className="muted sm" style={{ flexBasis: "100%" }}>
                    依据：{inf.recommended.basis}
                  </div>
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
              {inf.needsProbe && <div className="callout warn">无法确定身份，建议配置归属标识或用 PAT 探测。</div>}
              {!inf.recommended && !inf.needsProbe && <div className="muted">未匹配到身份。</div>}

              {identityOptions.length > 1 && (
                <div className="field">
                  <label className="field-label">克隆使用的身份</label>
                  <select
                    className="input"
                    value={activeIdentity?.identityId ?? ""}
                    onChange={(e) => setPickedIdentityId(e.target.value)}
                  >
                    {identityOptions.map((c) => (
                      <option key={c.identityId} value={c.identityId}>
                        {c.identityName}（{c.hostAlias}）
                      </option>
                    ))}
                  </select>
                </div>
              )}

              <div className="row" style={{ justifyContent: "flex-end" }}>
                <button
                  type="button"
                  className="btn primary"
                  disabled={!activeIdentity || cloneBusy || writesLocked}
                  onClick={startClone}
                >
                  <FolderGit2 size={14} />
                  <span>{cloneBusy ? "处理中…" : "克隆 / 初始化到本地"}</span>
                </button>
              </div>

              {lastClone && (
                <div className="callout info sm">
                  已自动加入仓库列表。身份 <strong>{lastClone.identityName}</strong>，
                  操作 {modeLabel(lastClone.mode)}，远程 <span className="mono">{lastClone.usedUrl}</span>
                  <button type="button" className="btn sm" style={{ marginLeft: 8 }} onClick={() => nav("/repos")}>
                    去仓库管理
                  </button>
                </div>
              )}
            </div>
          )}
        </div>
      </Card>

      {pending && (
        <CloneConfirmModal
          pending={pending}
          busy={cloneBusy}
          onCancel={() => setPending(null)}
          onConfirm={confirmClone}
        />
      )}
    </div>
  );
}

function CloneConfirmModal({
  pending,
  busy,
  onCancel,
  onConfirm,
}: {
  pending: { destDir: string; plan: ClonePlan; identity: Candidate; cloneUrl: string };
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const blocked = !pending.plan.canProceed;
  return (
    <div className="wizard-overlay" style={{ zIndex: 60 }}>
      <div
        className="card"
        style={{
          width: 460,
          maxWidth: "95%",
          border: blocked ? "1px solid var(--amber)" : "1px solid var(--border)",
        }}
      >
        <div className="card-head" style={blocked ? { background: "var(--amber-soft)" } : undefined}>
          <div className="row" style={{ gap: 6 }}>
            {blocked ? <AlertTriangle size={15} style={{ color: "var(--amber)" }} /> : <FolderGit2 size={15} />}
            <div className="card-title">{blocked ? "无法在此目录继续" : "确认落地方式"}</div>
          </div>
          <button type="button" className="btn ghost sm" onClick={onCancel} disabled={busy}>
            <X size={15} />
          </button>
        </div>

        <div className="card-body stack" style={{ gap: 8, padding: "12px 14px" }}>
          <div className={blocked ? "callout warn sm" : "callout info sm"}>{pending.plan.message}</div>
          <div className="muted sm">完成后会自动加入仓库管理列表。</div>
          <div className="grid-sum">
            <div>
              <div className="muted">选定文件夹</div>
              <div className="mono">{pending.destDir}</div>
            </div>
            <div>
              <div className="muted">实际目标</div>
              <div className="mono">{pending.plan.targetPath}</div>
            </div>
            <div>
              <div className="muted">将执行</div>
              <div>{modeLabel(pending.plan.suggestedMode)}</div>
            </div>
            <div>
              <div className="muted">使用身份</div>
              <div>
                {pending.identity.identityName}
                <span className="mono muted sm"> · {pending.identity.hostAlias}</span>
              </div>
            </div>
          </div>
          <div>
            <div className="muted">远程地址</div>
            <div className="mono">{pending.cloneUrl}</div>
          </div>
        </div>

        <div
          className="card-head"
          style={{ justifyContent: "flex-end", gap: 8, borderTop: "1px solid var(--border)", borderBottom: "none" }}
        >
          <button type="button" className="btn ghost sm" onClick={onCancel} disabled={busy}>
            {blocked ? "关闭" : "换个目录"}
          </button>
          {!blocked && (
            <button type="button" className="btn primary sm" disabled={busy} onClick={onConfirm}>
              {busy ? "执行中…" : pending.plan.suggestedMode === "init" ? "确认初始化" : "确认克隆"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
