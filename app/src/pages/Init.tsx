import { useState } from "react";
import { api, errMessage } from "../lib/ipc";
import { useApp } from "../store";

const STEPS = ["选择工作空间", "设置访问密码", "保存恢复密钥", "回填校验", "完成"];

export function InitWizard() {
  const refresh = useApp((s) => s.refresh);
  const [step, setStep] = useState(0);
  const [path, setPath] = useState("");
  const [warning, setWarning] = useState<string | null>(null);
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [recovery, setRecovery] = useState("");
  const [confirm, setConfirm] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  const strength = pwStrength(pw);

  async function checkPath() {
    setErr("");
    if (!path.trim()) return setErr("请填写工作空间目录");
    try {
      const r = await api.checkWorkspacePath(path);
      setWarning(r.warning);
      setStep(1);
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  async function doInit() {
    setErr("");
    if (pw.length < 8) return setErr("访问密码至少 8 位");
    if (pw !== pw2) return setErr("两次输入的密码不一致");
    setBusy(true);
    try {
      const r = await api.vaultInit(path, pw);
      setRecovery(r.recoveryKey);
      setStep(2);
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  function verifyConfirm() {
    setErr("");
    const norm = (s: string) => s.replace(/[\s-]/g, "").toUpperCase();
    if (norm(confirm) !== norm(recovery)) return setErr("恢复密钥不一致，请仔细核对（大小写与连字符可忽略）");
    setStep(4);
  }

  return (
    <div className="center-stage">
      <div className="wizard">
        <div className="wizard-head">
          <div className="title-lg">初始化加密工作空间</div>
          <div className="muted">所有密钥与机密都将加密存放在你指定的目录中</div>
          <div className="steps">
            {STEPS.map((s, i) => (
              <div key={s} className={"step" + (i === step ? " active" : i < step ? " done" : "")}>
                <span className="n">{i < step ? "✓" : i + 1}</span>
                {s}
              </div>
            ))}
          </div>
        </div>

        <div className="wizard-body">
          {step === 0 && (
            <div className="stack">
              <div className="field">
                <label>工作空间目录</label>
                <input
                  className="input mono"
                  placeholder="例如 D:\\gam-workspace"
                  value={path}
                  onChange={(e) => setPath(e.target.value)}
                />
                <div className="hint">建议选择不被 OneDrive / 坚果云等同步盘接管的本地目录。</div>
              </div>
            </div>
          )}

          {step === 1 && (
            <div className="stack">
              {warning && <div className="callout warn">⚠️ {warning}</div>}
              <div className="field">
                <label>访问密码</label>
                <input className="input" type="password" value={pw} onChange={(e) => setPw(e.target.value)} />
                <div className="hint">强度：{strength}</div>
              </div>
              <div className="field">
                <label>确认访问密码</label>
                <input className="input" type="password" value={pw2} onChange={(e) => setPw2(e.target.value)} />
              </div>
              <div className="callout info">
                🔑 密码只用于日常解锁；忘记密码只能靠下一步生成的恢复密钥。两者都丢将无法恢复。
              </div>
            </div>
          )}

          {step === 2 && (
            <div className="stack">
              <div className="callout danger">
                ⚠️ 这是唯一一次完整显示恢复密钥。请立刻复制并妥善离线保存——它是忘记密码/换机时唯一的救命稻草。
              </div>
              <div className="reckey">{recovery}</div>
              <div className="row">
                <button className="btn" onClick={() => navigator.clipboard.writeText(recovery)}>
                  复制
                </button>
              </div>
            </div>
          )}

          {step === 3 && (
            <div className="stack">
              <div className="muted">请把上一步保存的恢复密钥完整填回，确认你已真正保存。</div>
              <textarea
                className="input mono"
                placeholder="粘贴或输入恢复密钥"
                value={confirm}
                onChange={(e) => setConfirm(e.target.value)}
              />
            </div>
          )}

          {step === 4 && (
            <div className="stack" style={{ textAlign: "center", paddingTop: 20 }}>
              <div style={{ fontSize: 48 }}>✅</div>
              <div className="title-lg">工作空间已就绪</div>
              <div className="muted">你可以开始导入现有密钥或新建身份了。</div>
            </div>
          )}

          {err && <div className="err-text">{err}</div>}
        </div>

        <div className="wizard-foot">
          <button
            className="btn ghost"
            disabled={step === 0 || step >= 2}
            onClick={() => setStep((s) => Math.max(0, s - 1))}
          >
            上一步
          </button>
          {step === 0 && (
            <button className="btn primary" onClick={checkPath}>
              下一步
            </button>
          )}
          {step === 1 && (
            <button className="btn primary" disabled={busy} onClick={doInit}>
              {busy ? "创建中…" : "创建工作空间"}
            </button>
          )}
          {step === 2 && (
            <button className="btn primary" onClick={() => setStep(3)}>
              我已保存，继续
            </button>
          )}
          {step === 3 && (
            <button className="btn primary" onClick={verifyConfirm}>
              校验并完成
            </button>
          )}
          {step === 4 && (
            <button className="btn primary" onClick={() => refresh()}>
              进入应用
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function pwStrength(pw: string): string {
  if (pw.length < 8) return "弱";
  let score = 0;
  if (/[a-z]/.test(pw)) score++;
  if (/[A-Z]/.test(pw)) score++;
  if (/[0-9]/.test(pw)) score++;
  if (/[^a-zA-Z0-9]/.test(pw)) score++;
  if (pw.length >= 12) score++;
  return score >= 4 ? "强" : score >= 3 ? "中" : "弱";
}
