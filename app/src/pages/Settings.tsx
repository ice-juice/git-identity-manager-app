import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, errMessage } from "../lib/ipc";
import { useApp } from "../store";
import { THEME_OPTIONS } from "../lib/theme";
import { PageHead, Card, FieldLabel } from "../ui/common";

function closeActionLabel(action: "tray" | "quit" | null | undefined): string {
  if (action === "tray") return "自动最小化到托盘";
  if (action === "quit") return "直接退出程序";
  return "弹出二次确认";
}

function GithubPatSettings({ writesLocked }: { writesLocked: boolean }) {
  const [configured, setConfigured] = useState(false);
  const [login, setLogin] = useState("");
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");

  async function refresh() {
    const s = await api.githubPatStatus();
    setConfigured(s.configured);
    if (s.configured) {
      try {
        setLogin(await api.testGithubPat());
      } catch {
        setLogin("");
      }
    } else {
      setLogin("");
    }
  }

  useEffect(() => {
    refresh().catch((e) => setErr(errMessage(e)));
  }, []);

  async function save() {
    if (!token.trim()) return setErr("请粘贴 GitHub PAT");
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      await api.setGithubPat(token.trim());
      const name = await api.testGithubPat();
      setLogin(name);
      setConfigured(true);
      setToken("");
      setMsg(`PAT 已保存，当前 GitHub 账号 ${name}`);
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function test() {
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      const name = await api.testGithubPat();
      setLogin(name);
      setMsg(`校验通过，GitHub 账号 ${name}`);
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function clear() {
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      await api.clearGithubPat();
      setConfigured(false);
      setLogin("");
      setMsg("已清除 PAT");
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="stack">
      {err && <div className="err-text">{err}</div>}
      {msg && <div className="callout info">{msg}</div>}
      <div className="muted" style={{ fontSize: 12 }}>
        用于自动把公钥添加到 GitHub，以及导入组织列表。请创建 classic token，勾选{" "}
        <code>write:public_key</code> 或 <code>admin:public_key</code>。令牌只保存在加密库，这里不会回显。
      </div>
      <div className="kv">
        <span className="muted">状态</span>
        <span>{configured ? (login ? `已配置 · ${login}` : "已配置") : "未配置"}</span>
      </div>
      <div className="field">
        <FieldLabel name="粘贴新令牌" tip="在 GitHub Settings → Developer settings → Personal access tokens 生成。保存后立即校验一次。" />
        <input
          className="input mono"
          type="password"
          autoComplete="off"
          placeholder="ghp_… 或 github_pat_…"
          value={token}
          onChange={(e) => setToken(e.target.value)}
        />
      </div>
      <div className="row" style={{ flexWrap: "wrap" }}>
        <button type="button" className="btn primary sm" disabled={busy || writesLocked || !token.trim()} onClick={save}>
          保存并校验
        </button>
        <button type="button" className="btn sm" disabled={busy || !configured} onClick={test}>
          测试连接
        </button>
        <button type="button" className="btn ghost sm" disabled={busy || writesLocked || !configured} onClick={clear}>
          清除
        </button>
        <button type="button" className="btn ghost sm" onClick={() => api.openUrl("https://github.com/settings/tokens")}>
          打开 GitHub 令牌页
        </button>
      </div>
    </div>
  );
}

function FactoryResetPanel({ onDone }: { onDone: () => Promise<void> }) {
  const [stage, setStage] = useState<"idle" | "confirm">("idle");
  const [phrase, setPhrase] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [steps, setSteps] = useState<string[]>([]);

  async function run() {
    setErr("");
    setBusy(true);
    try {
      const r = await api.factoryReset(true, phrase);
      setSteps(r.steps);
      try {
        localStorage.removeItem("gam.theme");
      } catch {
        /* ignore */
      }
      await onDone();
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="stack">
      <div className="callout danger">
        将清空工作空间数据、还原 ~/.ssh 入口、撤销本程序写入的用户环境变量与 Git/终端挂钩，并删除本机配置。云端数据不会删除。此操作不可撤销。
      </div>
      {err && <div className="err-text">{err}</div>}
      {stage === "idle" ? (
        <div>
          <button type="button" className="btn danger sm" disabled={busy} onClick={() => setStage("confirm")}>
            准备清空还原…
          </button>
        </div>
      ) : (
        <div className="stack">
          <div className="muted" style={{ fontSize: 12 }}>
            请再次确认：在下方输入「清空」，然后执行。
          </div>
          <input
            className="input"
            placeholder="输入 清空"
            value={phrase}
            onChange={(e) => setPhrase(e.target.value)}
          />
          <div className="row">
            <button
              type="button"
              className="btn danger sm"
              disabled={busy || phrase.trim() !== "清空"}
              onClick={run}
            >
              {busy ? "正在还原…" : "确认清空并还原"}
            </button>
            <button
              type="button"
              className="btn ghost sm"
              disabled={busy}
              onClick={() => {
                setStage("idle");
                setPhrase("");
                setErr("");
              }}
            >
              取消
            </button>
          </div>
        </div>
      )}
      {steps.length > 0 && (
        <div className="callout info">
          {steps.map((s) => (
            <div key={s}>{s}</div>
          ))}
        </div>
      )}
    </div>
  );
}

function formatExpiry(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString();
}

export function Settings() {
  const navigate = useNavigate();
  const { status, refresh, theme, setTheme } = useApp();
  const [oldPw, setOldPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [newPw2, setNewPw2] = useState("");
  const [graceDays, setGraceDays] = useState(String(status?.graceDays ?? 0));
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);
  const [newRecovery, setNewRecovery] = useState("");

  useEffect(() => {
    setGraceDays(String(status?.graceDays ?? 0));
  }, [status?.graceDays]);

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
      await refresh();
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

  async function toggleLaunch(enabled: boolean) {
    setErr("");
    setBusy(true);
    try {
      await api.setLaunchAtLogin(enabled);
      setMsg(enabled ? "已打开开机自启动" : "已关闭开机自启动");
      await refresh();
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function saveGrace() {
    setErr("");
    const n = Number(graceDays);
    if (!Number.isFinite(n) || n < 0 || n > 30) return setErr("免验证天数请填写 0 到 30");
    setBusy(true);
    try {
      await api.setGraceDays(Math.floor(n));
      setMsg(n === 0 ? "已关闭免验证，下次启动需要访问密码" : `已设置 ${Math.floor(n)} 天内开机免验证`);
      await refresh();
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

      <Card title="界面主题与风格">
        <div className="stack">
          <div className="choice-row">
            {THEME_OPTIONS.map((opt) => {
              const active = theme === opt.id;
              return (
                <button
                  key={opt.id}
                  type="button"
                  className={"choice" + (active ? " on" : "")}
                  style={{ flex: "1 1 180px", padding: "8px 10px" }}
                  onClick={() => setTheme(opt.id)}
                >
                  <div className="row" style={{ justifyContent: "space-between", marginBottom: 3 }}>
                    <strong>{opt.name}</strong>
                    <span
                      style={{
                        display: "inline-block",
                        width: 12,
                        height: 12,
                        borderRadius: "50%",
                        backgroundColor: opt.previewAccent,
                        boxShadow: "0 0 0 1px var(--border-strong)",
                      }}
                    />
                  </div>
                  <div className="muted sm">{opt.desc}</div>
                </button>
              );
            })}
          </div>
        </div>
      </Card>

      <Card title="工作空间">
        <div className="kv">
          <span className="muted">路径</span>
          <span className="mono">{status?.workspacePath ?? "—"}</span>
        </div>
        <div className="kv">
          <span className="muted">SSH 配置</span>
          <span>
            <button type="button" className="btn ghost sm" onClick={() => navigate("/config")}>
              查看 / 用记事本编辑 →
            </button>
          </span>
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

      <Card title="关闭主窗口">
        <div className="stack">
          <div className="muted" style={{ fontSize: 12 }}>
            点击窗口关闭按钮时：{closeActionLabel(status?.closeAction)}。可在确认框勾选「记住我的选择」。
            从托盘重新打开时，会按下方「免验证天数」决定是否需要再次输入访问密码。
          </div>
          {status?.closeAction && (
            <div>
              <button
                type="button"
                className="btn ghost sm"
                disabled={busy}
                onClick={async () => {
                  setErr("");
                  setBusy(true);
                  try {
                    await api.clearClosePreference();
                    setMsg("已恢复为每次关闭都询问");
                    await refresh();
                  } catch (e) {
                    setErr(errMessage(e));
                  } finally {
                    setBusy(false);
                  }
                }}
              >
                恢复为每次询问
              </button>
            </div>
          )}
        </div>
      </Card>

      <Card title="开机自启动与免验证">
        <div className="stack">
          <div className="field">
            <div className="between">
              <div>
                <FieldLabel
                  name="开机自启动"
                  tip="写入当前 Windows 用户的开机启动项。登录后自动打开本软件；若同时开启了免验证，会尝试把密钥加载进 ssh-agent。"
                />
                <div className="hint">示例：出差换电脑后重新装好，再打开此项；默认关闭。</div>
              </div>
              <button
                type="button"
                className={"switch" + (status?.launchAtLogin ? "" : " off")}
                disabled={busy}
                onClick={() => toggleLaunch(!status?.launchAtLogin)}
              />
            </div>
          </div>
          <div className="field">
            <FieldLabel
              name="免验证天数"
              tip="0 表示每次启动都要访问密码。1–30 表示在该天数内，本机可用 Windows DPAPI 记住解锁状态：开机后自动解锁并加载 agent。过期必须重新输入密码。换 Windows 用户或改密码会失效。"
            />
            <div className="row">
              <input
                className="input"
                type="number"
                min={0}
                max={30}
                style={{ width: 88, minWidth: 88, flex: "0 0 88px" }}
                value={graceDays}
                onChange={(e) => setGraceDays(e.target.value)}
              />
              <button type="button" className="btn primary" disabled={busy} onClick={saveGrace}>
                保存
              </button>
            </div>
            <div className="hint">示例：填 7 表示一周内开机不用输密码；填 0 则每次都验证。</div>
          </div>
          {status?.graceActive && status.graceExpiresAt && (
            <div className="callout info">当前免验证有效至 {formatExpiry(status.graceExpiresAt)}，之后需重新输入访问密码。</div>
          )}
          {!status?.graceActive && (status?.graceDays ?? 0) > 0 && (
            <div className="callout warn">已设置免验证天数，但本机会话尚未建立或已过期。下次用访问密码解锁后会重新生效。</div>
          )}
        </div>
      </Card>

      <Card title="修改访问密码">
        <div className="stack" style={{ maxWidth: 420 }}>
          <div className="field">
            <label className="field-label">当前密码</label>
            <input className="input" type="password" value={oldPw} onChange={(e) => setOldPw(e.target.value)} />
          </div>
          <div className="field">
            <label className="field-label">新密码</label>
            <input className="input" type="password" value={newPw} onChange={(e) => setNewPw(e.target.value)} />
          </div>
          <div className="field">
            <label className="field-label">确认新密码</label>
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

      <Card title="GitHub PAT">
        <GithubPatSettings writesLocked={!!status?.writesLocked} />
      </Card>

      <Card title="一键清空还原（测试用）">
        <FactoryResetPanel onDone={refresh} />
      </Card>

      <Card title="数据备份与云端同步">
        <div className="stack">
          <div className="muted" style={{ fontSize: 12 }}>
            支持本地离线加密备份导出 (.gambackup) 与换机还原（M6），以及 S3 / Cloudflare R2 兼容的端到端零知识加密云端同步（v1.1）。
          </div>
          <div>
            <button
              type="button"
              className="btn primary sm"
              onClick={() => navigate("/sync")}
            >
              前往云端同步与备份管理 →
            </button>
          </div>
        </div>
      </Card>
    </div>
  );
}
