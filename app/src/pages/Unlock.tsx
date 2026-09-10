import { useState } from "react";
import { Sun, Moon, Palette } from "lucide-react";
import { api, errMessage } from "../lib/ipc";
import { useApp } from "../store";
import { AppLogo } from "../ui/AppLogo";

export function Unlock() {
  const { refresh, theme, toggleTheme, unlockAnimEnabled, startUnlockAnim } = useApp();
  const [pw, setPw] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [recoveryMode, setRecoveryMode] = useState(false);
  const [recovery, setRecovery] = useState("");

  async function unlock() {
    setErr("");
    setBusy(true);
    try {
      if (recoveryMode) {
        await api.vaultUnlockRecovery(recovery);
      } else {
        await api.vaultUnlock(pw);
      }
      // 只看应用内开关。系统「减少动态效果」不再偷偷跳过，否则用户会以为动画坏了。
      if (unlockAnimEnabled) {
        startUnlockAnim();
      }
      await refresh();
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="unlock-stage">
      <div className="unlock-card">
        <AppLogo size={46} style={{ margin: "0 auto 10px" }} />
        <div className="title-lg">解锁工作空间</div>
        <div className="muted" style={{ marginBottom: 20 }}>
          {recoveryMode ? "输入恢复密钥以解锁（忘记密码时使用）" : "输入访问密码继续"}
        </div>

        {recoveryMode ? (
          <textarea
            className="input mono"
            placeholder="恢复密钥 GAM1-..."
            value={recovery}
            onChange={(e) => setRecovery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && e.ctrlKey && unlock()}
          />
        ) : (
          <input
            className="input"
            type="password"
            placeholder="访问密码"
            value={pw}
            autoFocus
            onChange={(e) => setPw(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && unlock()}
          />
        )}

        {err && <div className="err-text">{err}</div>}

        <button type="button" className="btn primary lg" style={{ width: "100%", marginTop: 16 }} disabled={busy} onClick={unlock}>
          {busy ? "解锁中…" : "解锁"}
        </button>
        <button
          type="button"
          className="btn ghost sm"
          style={{ marginTop: 8 }}
          onClick={() => {
            setErr("");
            setRecoveryMode((m) => !m);
          }}
        >
          {recoveryMode ? "改用访问密码" : "忘记密码？用恢复密钥"}
        </button>

        <div style={{ marginTop: 12, borderTop: "1px solid var(--border)", paddingTop: 8 }}>
          <button
            type="button"
            className="btn ghost sm"
            onClick={toggleTheme}
            style={{ display: "inline-flex", gap: 5, color: "var(--text-mute)", fontSize: 11 }}
          >
            {theme === "light" ? <Sun size={12} /> : theme === "dark" ? <Moon size={12} /> : <Palette size={12} />}
            <span>皮肤：{theme === "light" ? "极简浅色" : theme === "dark" ? "冷萃深色" : "沉稳黛蓝"}</span>
          </button>
        </div>
      </div>
    </div>
  );
}
