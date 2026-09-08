import { useState } from "react";
import { api, errMessage } from "../lib/ipc";
import { useApp } from "../store";
import { PageHead, Card } from "../ui/common";

export function Settings() {
  const { status } = useApp();
  const [oldPw, setOldPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [newPw2, setNewPw2] = useState("");
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);
  const [newRecovery, setNewRecovery] = useState("");

  async function changePassword() {
    setErr("");
    setMsg("");
    if (newPw.length < 8) return setErr("新密码至少 8 位");
    if (newPw !== newPw2) return setErr("两次输入的新密码不一致");
    setBusy(true);
    try {
      await api.changePassword(oldPw, newPw);
      setMsg("访问密码已更新");
      setOldPw("");
      setNewPw("");
      setNewPw2("");
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function rotate() {
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      const r = await api.rotateRecoveryKey();
      setNewRecovery(r.recoveryKey);
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="stack-lg">
      <PageHead title="设置" desc="工作空间安全与账户配置" />
      {err && <div className="err-text">{err}</div>}
      {msg && <div className="callout info">{msg}</div>}

      <Card title="工作空间">
        <div className="kv">
          <span className="muted">路径</span>
          <span className="mono">{status?.workspacePath ?? "—"}</span>
        </div>
        <div className="kv">
          <span className="muted">ID</span>
          <span className="mono">{status?.workspaceId ?? "—"}</span>
        </div>
        <div className="kv">
          <span className="muted">自动锁定</span>
          <span>{status?.autoLockMinutes ?? 0} 分钟</span>
        </div>
      </Card>

      <Card title="修改访问密码">
        <div className="stack" style={{ maxWidth: 420 }}>
          <div className="field">
            <label>当前密码</label>
            <input className="input" type="password" value={oldPw} onChange={(e) => setOldPw(e.target.value)} />
          </div>
          <div className="field">
            <label>新密码</label>
            <input className="input" type="password" value={newPw} onChange={(e) => setNewPw(e.target.value)} />
          </div>
          <div className="field">
            <label>确认新密码</label>
            <input className="input" type="password" value={newPw2} onChange={(e) => setNewPw2(e.target.value)} />
          </div>
          <div>
            <button className="btn primary" disabled={busy} onClick={changePassword}>
              更新密码
            </button>
          </div>
        </div>
      </Card>

      <Card title="轮换恢复密钥">
        <div className="stack">
          <div className="muted">生成新的恢复密钥并作废旧的。请务必重新保存。</div>
          {newRecovery ? (
            <>
              <div className="callout danger">⚠️ 仅显示一次，请立即保存：</div>
              <div className="reckey">{newRecovery}</div>
              <div className="row">
                <button className="btn" onClick={() => navigator.clipboard.writeText(newRecovery)}>
                  复制
                </button>
                <button className="btn ghost" onClick={() => setNewRecovery("")}>
                  我已保存
                </button>
              </div>
            </>
          ) : (
            <div>
              <button className="btn danger" disabled={busy} onClick={rotate}>
                轮换恢复密钥
              </button>
            </div>
          )}
        </div>
      </Card>
    </div>
  );
}
