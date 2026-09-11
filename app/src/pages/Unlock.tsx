import { useEffect, useRef, useState } from "react";
import { Fingerprint, KeyRound, Sun, Moon, Palette } from "lucide-react";
import { api, errCode, errMessage, type BiometricStatus } from "../lib/ipc";
import { useApp } from "../store";
import { AppLogo } from "../ui/AppLogo";

type Gate = "bio" | "password" | "recovery";

export function Unlock() {
  const { refresh, theme, toggleTheme, unlockAnimEnabled, startUnlockAnim } = useApp();
  const [pw, setPw] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [recovery, setRecovery] = useState("");
  const [bio, setBio] = useState<BiometricStatus | null>(null);
  const [gate, setGate] = useState<Gate>("password");
  const userPickedGate = useRef(false);

  const fingerprintReady = !!(bio?.enabled && bio.available);

  useEffect(() => {
    api
      .biometricStatus()
      .then((s) => {
        setBio(s);
        if (s.enabled && s.available && !userPickedGate.current) {
          setGate("bio");
        }
      })
      .catch(() => setBio(null));
  }, []);

  async function afterUnlock() {
    if (unlockAnimEnabled) {
      startUnlockAnim();
    }
    await refresh();
  }

  async function unlockByFingerprint() {
    setErr("");
    setBusy(true);
    try {
      await api.vaultUnlockBiometric();
      await afterUnlock();
    } catch (e) {
      if (errCode(e) !== "BIOMETRIC_CANCELLED") {
        setErr(errMessage(e));
      }
    } finally {
      setBusy(false);
    }
  }

  async function unlock() {
    setErr("");
    setBusy(true);
    try {
      if (gate === "recovery") {
        await api.vaultUnlockRecovery(recovery);
      } else {
        await api.vaultUnlock(pw);
      }
      await afterUnlock();
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  function switchGate(next: Gate) {
    if (busy) return;
    userPickedGate.current = true;
    setErr("");
    setGate(next);
  }

  const subtitle =
    gate === "recovery"
      ? "输入恢复密钥以解锁"
      : gate === "bio"
        ? "优先使用系统指纹 / Windows Hello，也可改用密码"
        : fingerprintReady
          ? "输入访问密码，或改回指纹解锁"
          : "输入访问密码继续";

  return (
    <div className="unlock-stage">
      <div className="unlock-card">
        <AppLogo size={46} style={{ margin: "0 auto 10px" }} />
        <div className="title-lg">解锁工作空间</div>
        <div className="muted" style={{ marginBottom: 18 }}>
          {subtitle}
        </div>

        {gate === "bio" && (
          <div className="unlock-bio">
            <button
              type="button"
              className={`unlock-bio-pad${busy ? " is-busy" : ""}`}
              disabled={busy}
              onClick={unlockByFingerprint}
              aria-label="使用指纹解锁"
            >
              <Fingerprint size={36} />
            </button>
            <div className="unlock-bio-hint">
              {busy ? "请在系统验证窗口中完成指纹或面部识别" : "点击上方图标开始验证"}
            </div>
          </div>
        )}

        {gate === "password" && (
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

        {gate === "recovery" && (
          <textarea
            className="input mono"
            placeholder="恢复密钥 GAM1-..."
            value={recovery}
            autoFocus
            onChange={(e) => setRecovery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && e.ctrlKey && unlock()}
          />
        )}

        {err && <div className="err-text">{err}</div>}

        {gate !== "bio" && (
          <button type="button" className="btn primary lg" style={{ width: "100%", marginTop: 16 }} disabled={busy} onClick={unlock}>
            {busy ? "解锁中…" : gate === "recovery" ? "用恢复密钥解锁" : "解锁"}
          </button>
        )}

        <div className="unlock-alts">
          {gate === "bio" && (
            <button type="button" className="unlock-switch" disabled={busy} onClick={() => switchGate("password")}>
              <KeyRound size={12} />
              使用密码
            </button>
          )}
          {gate === "password" && fingerprintReady && (
            <button type="button" className="unlock-switch" disabled={busy} onClick={() => switchGate("bio")}>
              <Fingerprint size={12} />
              使用指纹
            </button>
          )}
          {gate !== "recovery" ? (
            <button type="button" className="unlock-switch" disabled={busy} onClick={() => switchGate("recovery")}>
              忘记密码？使用恢复密钥
            </button>
          ) : (
            <button
              type="button"
              className="unlock-switch"
              disabled={busy}
              onClick={() => switchGate(fingerprintReady ? "bio" : "password")}
            >
              {fingerprintReady ? "返回指纹解锁" : "改用访问密码"}
            </button>
          )}
        </div>

        <div style={{ marginTop: 10, borderTop: "1px solid var(--border)", paddingTop: 8 }}>
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
