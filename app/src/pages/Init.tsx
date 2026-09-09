import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Sun, Moon, Palette } from "lucide-react";
import { api, errMessage } from "../lib/ipc";
import { useApp } from "../store";
import { AppLogo } from "../ui/AppLogo";

const STEPS = ["选择工作空间", "设置访问密码", "保存恢复密钥", "回填校验", "完成"];

function samePath(a: string, b: string): boolean {
  return a.trim().replace(/[\\/]+$/, "").toLowerCase() === b.trim().replace(/[\\/]+$/, "").toLowerCase();
}

export function InitWizard() {
  const { refresh, theme, toggleTheme } = useApp();
  const [step, setStep] = useState(0);
  const [path, setPath] = useState("");
  const [warning, setWarning] = useState<string | null>(null);
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [recovery, setRecovery] = useState("");
  const [confirm, setConfirm] = useState("");
  const [createdPath, setCreatedPath] = useState<string | null>(null);
  const [createdPw, setCreatedPw] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  const strength = pwStrength(pw);
  const alreadyCreated = !!createdPath;
  const switchingPath = alreadyCreated && !samePath(path, createdPath);

  async function pickDir() {
    setErr("");
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "选择工作空间目录",
        defaultPath: path.trim() || undefined,
      });
      if (typeof selected !== "string" || !selected) return;
      setPath(selected);
      const r = await api.checkWorkspacePath(selected);
      setWarning(r.warning);
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  async function checkPath() {
    setErr("");
    if (!path.trim()) return setErr("请选择或填写工作空间目录");
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
      if (createdPath && samePath(path, createdPath)) {
        if (pw !== createdPw) {
          await api.changePassword(createdPw, pw);
          setCreatedPw(pw);
        }
        setStep(2);
        return;
      }
      const r = await api.vaultInit(path, pw);
      setRecovery(r.recoveryKey);
      setCreatedPath(path);
      setCreatedPw(pw);
      setConfirm("");
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

  function goBack() {
    if (step <= 0 || step >= 4) return;
    setErr("");
    setStep((s) => s - 1);
  }

  function jumpTo(i: number) {
    if (i >= step || i >= 4) return;
    setErr("");
    setStep(i);
  }

  return (
    <div className="window">
      <div className="body">
        <aside className="sidebar" style={{ width: 176, flex: "0 0 176px", padding: "10px 8px" }}>
          <div className="brand" style={{ padding: "4px 6px 12px" }}>
            <AppLogo size={28} />
            <div className="name">
              Git 多账号管理器
              <small>工作空间初始化</small>
            </div>
          </div>
          <div className="nav-group">向导步骤</div>
          <div className="stack" style={{ gap: 3 }}>
            {STEPS.map((s, i) => (
              <div
                key={s}
                className={"nav-item" + (i === step ? " active" : "")}
                style={{
                  cursor: i < step ? "pointer" : "default",
                  color: i === step ? "var(--accent)" : i < step ? "var(--text)" : "var(--text-soft)",
                  fontWeight: i === step ? 600 : 500,
                }}
                onClick={() => jumpTo(i)}
              >
                <span
                  className="ic"
                  style={{
                    fontSize: 10.5,
                    fontWeight: 700,
                    color: i < step ? "var(--green)" : i === step ? "var(--accent)" : "var(--text-mute)",
                  }}
                >
                  {i < step ? "✓" : i + 1}
                </span>
                <span style={{ fontSize: 12 }}>{s}</span>
              </div>
            ))}
          </div>

          <div className="grow" />

          <div className="side-foot">
            <button
              type="button"
              className="nav-item"
              onClick={toggleTheme}
              title="点击切换界面皮肤风格"
            >
              <span className="ic">
                {theme === "light" ? <Sun size={14} /> : theme === "dark" ? <Moon size={14} /> : <Palette size={14} />}
              </span>
              <span style={{ fontSize: 11.5, color: "var(--text-soft)" }}>
                皮肤：{theme === "light" ? "浅色" : theme === "dark" ? "深色" : "黛蓝"}
              </span>
            </button>
          </div>
        </aside>

        <main
          className="main"
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            padding: "20px",
            background: "var(--bg)",
          }}
        >
          <div
            className="card"
            style={{
              width: 520,
              maxWidth: "100%",
              padding: "20px 22px",
              boxShadow: "var(--shadow-lg)",
            }}
          >
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", marginBottom: 16 }}>
              <div>
                <div className="title-lg" style={{ fontSize: 15, fontWeight: 700 }}>
                  {step === 0 && "选择加密工作空间目录"}
                  {step === 1 && "设置日常访问密码"}
                  {step === 2 && "妥善保存恢复密钥"}
                  {step === 3 && "回填校验恢复密钥"}
                  {step === 4 && "工作空间已就绪"}
                </div>
                <div className="muted" style={{ marginTop: 4, lineHeight: 1.45 }}>
                  {step === 0 && "所有密钥和配置文件均加密存储在你的本地目录中，绝不明文上传。"}
                  {step === 1 && "密码用于本机日常解锁；请牢记，一旦遗忘只能使用恢复密钥重置。"}
                  {step === 2 && "恢复密钥仅显示一次，是换机或忘记密码时唯一的救命稻草，请离线保存。"}
                  {step === 3 && "请把刚才保存的恢复密钥完整填入下方，确认你已真正抄录备份。"}
                  {step === 4 && "加密工作空间已初始化完成，你可以直接进入应用。"}
                </div>
              </div>
              <span className="badge info" style={{ flexShrink: 0, marginLeft: 12 }}>
                {step + 1} / {STEPS.length}
              </span>
            </div>

            <div>
              {step === 0 && (
                <div className="stack" style={{ gap: 10 }}>
                  {alreadyCreated && (
                    <div className="callout warn">
                      已在「{createdPath}」创建过工作空间。更换目录会创建新空间，原目录不会自动删除。
                    </div>
                  )}
                  <div className="field">
                    <label className="field-label">工作空间目录</label>
                    <div className="path-pick">
                      <input
                        className="input mono"
                        placeholder="点击「浏览」选择，或直接粘贴路径"
                        value={path}
                        onChange={(e) => setPath(e.target.value)}
                        onKeyDown={(e) => e.key === "Enter" && checkPath()}
                      />
                      <button type="button" className="btn" onClick={pickDir}>
                        浏览…
                      </button>
                    </div>
                    <div className="hint">建议选择不被 OneDrive / 坚果云等第三方同步盘接管的本地独立目录。</div>
                  </div>
                  {warning && <div className="callout warn">⚠️ {warning}</div>}
                </div>
              )}

              {step === 1 && (
                <div className="stack" style={{ gap: 10 }}>
                  {warning && <div className="callout warn">⚠️ {warning}</div>}
                  {alreadyCreated && !switchingPath && (
                    <div className="callout info">
                      工作空间已创建。在此修改密码只会更新访问密码，恢复密钥不变。
                    </div>
                  )}
                  {switchingPath && (
                    <div className="callout warn">
                      ⚠️ 将在新目录创建另一个工作空间，并生成新的恢复密钥。
                    </div>
                  )}
                  <div className="field">
                    <label className="field-label">访问密码</label>
                    <input
                      className="input"
                      type="password"
                      value={pw}
                      autoFocus
                      onChange={(e) => setPw(e.target.value)}
                    />
                    <div className="hint">密码强度：{strength}（至少 8 位，包含字母、数字或符号更佳）</div>
                  </div>
                  <div className="field">
                    <label className="field-label">确认访问密码</label>
                    <input
                      className="input"
                      type="password"
                      value={pw2}
                      onChange={(e) => setPw2(e.target.value)}
                      onKeyDown={(e) => e.key === "Enter" && doInit()}
                    />
                  </div>
                  <div className="callout info">
                    🔑 访问密码仅保存在本地进行加密校验，请牢记。
                  </div>
                </div>
              )}

              {step === 2 && (
                <div className="stack" style={{ gap: 10 }}>
                  <div className="callout danger">
                    ⚠️ 这是唯一一次完整显示恢复密钥。请立刻复制并妥善离线保存！
                  </div>
                  <div className="reckey">{recovery}</div>
                  <div className="row">
                    <button type="button" className="btn primary sm" onClick={() => navigator.clipboard.writeText(recovery)}>
                      复制恢复密钥
                    </button>
                  </div>
                </div>
              )}

              {step === 3 && (
                <div className="stack" style={{ gap: 10 }}>
                  <div className="muted">请把刚才保存的恢复密钥完整填入下方（支持粘贴）：</div>
                  <textarea
                    className="input mono"
                    placeholder="粘贴或输入恢复密钥（如 GAM1-...）"
                    value={confirm}
                    onChange={(e) => setConfirm(e.target.value)}
                    style={{ minHeight: 76 }}
                  />
                </div>
              )}

              {step === 4 && (
                <div className="stack" style={{ textAlign: "center", padding: "16px 0", gap: 6 }}>
                  <div style={{ fontSize: 36, marginBottom: 2 }}>🎉</div>
                  <div className="title-lg" style={{ fontSize: 16 }}>加密工作空间已就绪</div>
                  <div className="muted">你可以开始导入现有 SSH 密钥，或者新建多平台 Git 身份。</div>
                </div>
              )}

              {err && <div className="err-text" style={{ marginTop: 8 }}>{err}</div>}
            </div>

            <div
              className="between"
              style={{
                marginTop: 20,
                paddingTop: 12,
                borderTop: "1px solid var(--border)",
              }}
            >
              <button
                type="button"
                className="btn ghost sm"
                disabled={step === 0 || step >= 4}
                onClick={goBack}
              >
                上一步
              </button>

              <div>
                {step === 0 && (
                  <button type="button" className="btn primary" onClick={checkPath}>
                    下一步
                  </button>
                )}
                {step === 1 && (
                  <button type="button" className="btn primary" disabled={busy} onClick={doInit}>
                    {busy
                      ? "处理中…"
                      : switchingPath
                        ? "在新目录创建工作空间"
                        : alreadyCreated
                          ? "保存并继续"
                          : "创建工作空间"}
                  </button>
                )}
                {step === 2 && (
                  <button type="button" className="btn primary" onClick={() => setStep(3)}>
                    我已安全保存，继续
                  </button>
                )}
                {step === 3 && (
                  <button type="button" className="btn primary" onClick={verifyConfirm}>
                    校验并完成
                  </button>
                )}
                {step === 4 && (
                  <button type="button" className="btn primary" onClick={() => refresh()}>
                    进入主界面
                  </button>
                )}
              </div>
            </div>
          </div>
        </main>
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
