import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { getVersion } from "@tauri-apps/api/app";
import {
  Palette,
  ShieldCheck,
  FolderKanban,
  Rocket,
  AlertTriangle,
  ExternalLink,
  Globe,
  RefreshCw,
  Cloud,
  FileCog,
  Play,
  Fingerprint,
} from "lucide-react";
import {
  api,
  errMessage,
  type BiometricStatus,
  type NetworkProxy,
  type ProxyTestResult,
  type SecurityChecklist,
  type SecurityFinding,
  type SecurityLevel,
  type UpdateCheckResult,
  type UpdateSource,
} from "../lib/ipc";
import { writeClipboard } from "../lib/clipboard";
import { useApp } from "../store";
import { THEME_OPTIONS } from "../lib/theme";
import { UNLOCK_ANIM_STYLES } from "../lib/prefs";
import { PageHead, Card, FieldLabel, Badge, ErrorDialog } from "../ui/common";

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
      <ErrorDialog message={err} onClose={() => setErr("")} />
      {msg && <div className="callout info">{msg}</div>}
      <div className="muted" style={{ fontSize: 12 }}>
        用于自动把公钥添加到 GitHub，以及导入组织列表。请创建 classic token，勾选{" "}
        <code>write:public_key</code> 或 <code>admin:public_key</code>。令牌只保存在加密库，这里不会回显。
      </div>
      <div className="kv">
        <span className="muted">当前状态</span>
        <span>
          {configured ? (
            <Badge kind="good">已配置 {login ? `· ${login}` : ""}</Badge>
          ) : (
            <Badge kind="warn">未配置</Badge>
          )}
        </span>
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
      <div className="row" style={{ flexWrap: "wrap", marginTop: 4 }}>
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
          <ExternalLink size={12} style={{ marginRight: 3 }} />
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
      <ErrorDialog message={err} onClose={() => setErr("")} />
      <div className="callout danger">
        ⚠️ 将清空工作空间数据、还原 ~/.ssh 入口、撤销本程序写入的用户环境变量与 Git/终端挂钩，并删除本机配置。云端数据不会删除。此操作不可撤销。
      </div>
      {stage === "idle" ? (
        <div>
          <button type="button" className="btn danger sm" disabled={busy} onClick={() => setStage("confirm")}>
            准备清空还原…
          </button>
        </div>
      ) : (
        <div className="stack" style={{ maxWidth: 360, marginTop: 4 }}>
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

const DEFAULT_UPDATE_REPO = "ice-juice/git-keymaster-app";

function formatWhen(iso: string | null | undefined): string {
  if (!iso) return "尚未检查";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString();
}

function UpdateNotes({ notes }: { notes: string }) {
  const blocks = notes
    .split(/\r?\n/)
    .map((line) => line.trimEnd())
    .filter((line, i, arr) => line.trim() !== "" || (i > 0 && arr[i - 1].trim() !== ""));

  return (
    <div className="update-notes">
      {blocks.map((line, i) => {
        const heading = line.match(/^#{2,3}\s+(.+)/);
        if (heading) {
          return (
            <div key={i} className="update-notes-h">
              {heading[1]}
            </div>
          );
        }
        const item = line.match(/^[-*]\s+(.+)/);
        if (item) {
          return (
            <div key={i} className="update-notes-li">
              {renderInline(item[1])}
            </div>
          );
        }
        return (
          <div key={i} className="update-notes-p">
            {renderInline(line)}
          </div>
        );
      })}
    </div>
  );
}

function renderInline(text: string) {
  const parts = text.split(/(\*\*[^*]+\*\*)/g);
  return parts.map((part, i) => {
    const bold = part.match(/^\*\*([^*]+)\*\*$/);
    if (bold) return <strong key={i}>{bold[1]}</strong>;
    return <span key={i}>{part}</span>;
  });
}

const EMPTY_PROXY: NetworkProxy = {
  enabled: false,
  scheme: "http",
  host: "127.0.0.1",
  port: 7890,
  username: "",
  password: "",
  applyToGitHttps: true,
  applyToSsh: true,
  applyToCloudSync: true,
};

function NetworkProxyCard() {
  const [form, setForm] = useState<NetworkProxy>(EMPTY_PROXY);
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");
  const [test, setTest] = useState<ProxyTestResult | null>(null);

  useEffect(() => {
    api
      .getNetworkProxy()
      .then((p) => {
        if (p) {
          setForm({
            ...EMPTY_PROXY,
            ...p,
            username: p.username ?? "",
            password: p.password ?? "",
          });
        }
      })
      .catch((e) => setErr(errMessage(e)));
  }, []);

  function patch(partial: Partial<NetworkProxy>) {
    setForm((prev) => ({ ...prev, ...partial }));
  }

  function payload(): NetworkProxy {
    const port = Number(form.port);
    return {
      ...form,
      scheme: form.scheme || "http",
      host: form.host.trim(),
      port: Number.isFinite(port) ? port : 0,
      username: form.username?.trim() || null,
      password: form.password || null,
    };
  }

  async function save() {
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      await api.saveNetworkProxy(payload());
      setMsg(payload().enabled ? "代理已保存并启用" : "代理已保存（当前关闭）");
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function clearProxy() {
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      await api.saveNetworkProxy(null);
      setForm(EMPTY_PROXY);
      setTest(null);
      setMsg("已清除代理，恢复直连");
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function testNow() {
    setErr("");
    setMsg("");
    setTesting(true);
    try {
      const r = await api.testNetworkProxy({ ...payload(), enabled: true });
      setTest(r);
      setMsg(r.httpsOk ? `GitHub HTTPS 通，耗时 ${r.httpsMs} ms` : "GitHub HTTPS 未通，请检查代理");
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setTesting(false);
    }
  }

  return (
    <Card title="网络代理">
      <div className="stack">
        <ErrorDialog message={err} onClose={() => setErr("")} />
        {msg && <div className="callout info">{msg}</div>}
        <div className="field">
          <div className="between">
            <div>
              <FieldLabel
                name="启用代理"
                tip="只作用于本应用发起的请求（检查更新、GitHub API、Git HTTPS、SSH、云同步），不改系统代理。"
              />
              <div className="hint">部分地区访问 GitHub / GitLab 不稳定时打开。密码保存在本机配置文件。</div>
            </div>
            <button
              type="button"
              className={"switch" + (form.enabled ? "" : " off")}
              disabled={busy}
              onClick={() => patch({ enabled: !form.enabled })}
            />
          </div>
        </div>

        <div className="field">
          <label className="field-label">协议</label>
          <div className="choice-row">
            {(["http", "https", "socks5"] as const).map((s) => (
              <button
                key={s}
                type="button"
                className={"choice" + (form.scheme === s ? " on" : "")}
                onClick={() => patch({ scheme: s })}
              >
                {s.toUpperCase()}
              </button>
            ))}
          </div>
        </div>

        <div className="row" style={{ flexWrap: "wrap" }}>
          <div className="field grow">
            <label className="field-label">主机</label>
            <input
              className="input mono"
              value={form.host}
              onChange={(e) => patch({ host: e.target.value })}
              placeholder="127.0.0.1"
            />
          </div>
          <div className="field" style={{ width: 110 }}>
            <label className="field-label">端口</label>
            <input
              className="input mono"
              type="number"
              min={1}
              max={65535}
              value={form.port}
              onChange={(e) => patch({ port: Number(e.target.value) })}
            />
          </div>
        </div>

        <div className="row" style={{ flexWrap: "wrap" }}>
          <div className="field grow">
            <label className="field-label">用户名（可选）</label>
            <input className="input" value={form.username ?? ""} onChange={(e) => patch({ username: e.target.value })} />
          </div>
          <div className="field grow">
            <label className="field-label">密码（可选）</label>
            <input
              className="input"
              type="password"
              autoComplete="new-password"
              value={form.password ?? ""}
              onChange={(e) => patch({ password: e.target.value })}
            />
          </div>
        </div>

        <div className="field">
          <div className="between">
            <FieldLabel name="Git HTTPS 走代理" tip="本应用执行 git clone 等 HTTPS 请求时写入 HTTP(S)_PROXY。" />
            <button
              type="button"
              className={"switch" + (form.applyToGitHttps ? "" : " off")}
              disabled={busy}
              onClick={() => patch({ applyToGitHttps: !form.applyToGitHttps })}
            />
          </div>
        </div>
        <div className="field">
          <div className="between">
            <FieldLabel
              name="SSH 走代理"
              tip="为本应用拉起的 ssh / git 注入 ProxyCommand。需要本机有 Git 的 connect 或 ncat。不修改 ~/.ssh/config。"
            />
            <button
              type="button"
              className={"switch" + (form.applyToSsh ? "" : " off")}
              disabled={busy}
              onClick={() => patch({ applyToSsh: !form.applyToSsh })}
            />
          </div>
        </div>
        <div className="field">
          <div className="between">
            <FieldLabel name="云同步走代理" tip="R2 / S3 备份默认跟随总开关。国内对象存储可关掉此项。" />
            <button
              type="button"
              className={"switch" + (form.applyToCloudSync ? "" : " off")}
              disabled={busy}
              onClick={() => patch({ applyToCloudSync: !form.applyToCloudSync })}
            />
          </div>
        </div>

        {test && (
          <div className={"callout " + (test.httpsOk ? "good" : "warn")}>
            <div>{test.httpsOk ? `GitHub HTTPS 成功（${test.httpsMs} ms）` : test.httpsError}</div>
            {test.sshNote && <div>{test.sshNote}</div>}
          </div>
        )}

        <div className="row" style={{ flexWrap: "wrap", marginTop: 4 }}>
          <button type="button" className="btn primary sm" disabled={busy || testing} onClick={testNow}>
            <Globe size={13} style={{ marginRight: 3 }} />
            {testing ? "正在测试…" : "测试连接"}
          </button>
          <button type="button" className="btn sm" disabled={busy} onClick={save}>
            保存
          </button>
          <button type="button" className="btn ghost sm" disabled={busy} onClick={clearProxy}>
            清除代理
          </button>
        </div>
      </div>
    </Card>
  );
}

function AboutUpdateCard() {
  const [version, setVersion] = useState("");
  const [lastCheck, setLastCheck] = useState<string | null>(null);
  const [kind, setKind] = useState<UpdateSource["kind"]>("github");
  const [repo, setRepo] = useState("");
  const [manifestUrl, setManifestUrl] = useState("");
  const [includePrerelease, setIncludePrerelease] = useState(false);
  const [autoCheck, setAutoCheck] = useState(true);
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");
  const [result, setResult] = useState<UpdateCheckResult | null>(null);

  async function loadPrefs() {
    const [src, last, auto] = await Promise.all([
      api.getUpdateSource(),
      api.getLastUpdateCheck(),
      api.getAutoCheckUpdate(),
    ]);
    setKind(src.kind === "manifest" ? "manifest" : "github");
    setRepo(src.repo ?? "");
    setManifestUrl(src.manifestUrl ?? "");
    setIncludePrerelease(!!src.includePrerelease);
    setLastCheck(last);
    setAutoCheck(auto);
  }

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion("1.3.0"));
    loadPrefs().catch((e) => setErr(errMessage(e)));
  }, []);

  function currentSource(): UpdateSource {
    if (kind === "manifest") {
      return { kind: "manifest", manifestUrl: manifestUrl.trim(), includePrerelease: false };
    }
    return {
      kind: "github",
      repo: repo.trim() || undefined,
      includePrerelease,
    };
  }

  async function saveSource() {
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      await api.saveUpdateSource(currentSource());
      await loadPrefs();
      setMsg("更新源已保存");
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function restoreDefault() {
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      await api.saveUpdateSource(null);
      await loadPrefs();
      setKind("github");
      setRepo("");
      setManifestUrl("");
      setIncludePrerelease(false);
      setMsg("已恢复为默认 GitHub 更新源");
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function toggleAuto(enabled: boolean) {
    setErr("");
    setBusy(true);
    try {
      await api.setAutoCheckUpdate(enabled);
      setAutoCheck(enabled);
      setMsg(enabled ? "已开启启动时自动检查更新" : "已关闭启动时自动检查更新");
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function checkNow() {
    setErr("");
    setMsg("");
    setChecking(true);
    try {
      const r = await api.checkUpdate();
      setResult(r);
      setVersion(r.currentVersion);
      setLastCheck(await api.getLastUpdateCheck());
      setMsg(r.available ? `发现新版本 ${r.latestVersion}` : "当前已是最新版本，或远端尚未发布更新清单");
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setChecking(false);
    }
  }

  async function install() {
    setErr("");
    setMsg("");
    setInstalling(true);
    try {
      await api.downloadAndInstallUpdate();
      setMsg("更新已安装，即将重启…");
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setInstalling(false);
    }
  }

  async function skip() {
    const ver = result?.latestVersion;
    if (!ver) return;
    setErr("");
    setBusy(true);
    try {
      await api.skipUpdateVersion(ver);
      setMsg(`已跳过版本 ${ver}，启动时不再提示`);
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const showInstall = !!result?.available && result.selfUpdateSupported;
  const showManualOnly = !!result?.available && !result.selfUpdateSupported;

  return (
    <div className="stack-lg">
      <Card title="应用与系统信息">
        <div className="stack">
          <ErrorDialog message={err} onClose={() => setErr("")} />
          {msg && <div className="callout info">{msg}</div>}
          <div className="kv">
            <span className="muted">当前版本</span>
            <span className="mono">{version || "—"}</span>
          </div>
          <div className="kv">
            <span className="muted">最近检查</span>
            <span>{formatWhen(lastCheck)}</span>
          </div>
          <div className="kv">
            <span className="muted">运行平台</span>
            <span className="mono">{result?.platform ?? "检查后显示"}</span>
          </div>
          <div style={{ marginTop: 4 }}>
            <button type="button" className="btn primary sm" disabled={checking || busy} onClick={checkNow}>
              <RefreshCw size={13} className={checking ? "animate-spin" : ""} style={{ marginRight: 3 }} />
              {checking ? "正在检查…" : "立即检查更新"}
            </button>
          </div>

          {result?.available && (
            <div className="callout info update-result">
              <div>
                <strong>新版本 {result.latestVersion}</strong>
                {result.pubDate ? ` · ${formatWhen(result.pubDate)}` : ""}
              </div>
              {result.notes && <UpdateNotes notes={result.notes} />}
              {showManualOnly && (
                <div className="muted" style={{ marginTop: 8, fontSize: 12 }}>
                  当前安装方式不支持应用内更新（例如 Linux 非 AppImage），请使用手动下载。
                </div>
              )}
              <div className="row" style={{ flexWrap: "wrap", marginTop: 8 }}>
                {showInstall && (
                  <button type="button" className="btn primary sm" disabled={installing} onClick={install}>
                    {installing ? "正在下载安装…" : "下载并安装"}
                  </button>
                )}
                <button type="button" className="btn sm" disabled={busy} onClick={skip}>
                  跳过此版本
                </button>
                {result.downloadUrl && (
                  <button
                    type="button"
                    className="btn ghost sm"
                    onClick={() => api.openUrl(result.downloadUrl!)}
                  >
                    手动下载
                  </button>
                )}
              </div>
            </div>
          )}
        </div>
      </Card>

      <Card title="更新偏好与数据源">
        <div className="stack">
          <div className="field">
            <div className="between">
              <div>
                <FieldLabel name="自动检查更新" tip="解锁后约 5 秒静默检查一次。发现新版本且未被跳过时，右下角提示。" />
                <div className="hint">关闭后仍可手动检查。</div>
              </div>
              <button
                type="button"
                className={"switch" + (autoCheck ? "" : " off")}
                disabled={busy}
                onClick={() => toggleAuto(!autoCheck)}
              />
            </div>
          </div>

          <div className="field" style={{ marginTop: 4 }}>
            <FieldLabel name="更新源配置" tip="默认使用内置 GitHub 仓库。也可改成其它仓库，或填写一份静态 latest.json 的 HTTPS 地址（内网镜像 / 其它 Git 平台）。无论何种源，安装包都必须通过内置公钥校验。" />
            <div className="choice-row">
              <button
                type="button"
                className={"choice" + (kind === "github" ? " on" : "")}
                onClick={() => setKind("github")}
              >
                GitHub 官方仓库
              </button>
              <button
                type="button"
                className={"choice" + (kind === "manifest" ? " on" : "")}
                onClick={() => setKind("manifest")}
              >
                自定义清单 URL
              </button>
            </div>
          </div>

          {kind === "github" ? (
            <>
              <div className="field">
                <label className="field-label">仓库 owner/repo</label>
                <input
                  className="input mono"
                  placeholder={DEFAULT_UPDATE_REPO}
                  value={repo}
                  onChange={(e) => setRepo(e.target.value)}
                />
                <div className="hint">留空则使用出厂默认仓库 {DEFAULT_UPDATE_REPO}</div>
              </div>
              <div className="field">
                <div className="between">
                  <FieldLabel name="包含预发布版本" tip="开启后走 GitHub Releases API，可能包含 pre-release。" />
                  <button
                    type="button"
                    className={"switch" + (includePrerelease ? "" : " off")}
                    disabled={busy}
                    onClick={() => setIncludePrerelease(!includePrerelease)}
                  />
                </div>
              </div>
            </>
          ) : (
            <div className="field">
              <label className="field-label">清单 URL</label>
              <input
                className="input mono"
                placeholder="https://mirror.example.com/latest.json"
                value={manifestUrl}
                onChange={(e) => setManifestUrl(e.target.value)}
              />
              <div className="hint">必须是 HTTPS。支持 {"{{target}}"} / {"{{arch}}"} / {"{{current_version}}"} 占位符。</div>
            </div>
          )}

          <div className="row" style={{ flexWrap: "wrap", marginTop: 4 }}>
            <button type="button" className="btn primary sm" disabled={busy} onClick={saveSource}>
              保存更新源
            </button>
            <button type="button" className="btn ghost sm" disabled={busy} onClick={restoreDefault}>
              恢复默认
            </button>
          </div>
        </div>
      </Card>

      <NetworkProxyCard />
    </div>
  );
}

function formatExpiry(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString();
}

type SettingsTab = "general" | "security" | "workspace" | "about" | "danger";

const SETTING_ANCHOR_TAB: Record<string, SettingsTab> = {
  "workspace-path": "workspace",
  "auto-lock": "workspace",
  "lock-on-sleep": "workspace",
  "reveal-grace": "security",
  "clipboard-clear": "security",
};

function scrollToSettingAnchor(anchor: string) {
  const el = document.getElementById(`setting-${anchor}`);
  el?.scrollIntoView({ behavior: "smooth", block: "center" });
}

function levelBadge(level: SecurityLevel): { kind: string; label: string } {
  if (level === "risk") return { kind: "danger", label: "有风险" };
  if (level === "caution") return { kind: "warn", label: "需留意" };
  return { kind: "good", label: "安全" };
}

function findingCallout(severity: SecurityFinding["severity"]): string {
  if (severity === "warn") return "warn";
  if (severity === "info") return "info";
  return "good";
}

function SecurityChecklistCard({
  refreshNonce,
  onJump,
}: {
  refreshNonce: number;
  onJump: (anchor: string) => void;
}) {
  const [list, setList] = useState<SecurityChecklist | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [showOk, setShowOk] = useState(false);

  async function load() {
    setErr("");
    setBusy(true);
    try {
      setList(await api.securityChecklist());
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    load().catch((e) => setErr(errMessage(e)));
  }, [refreshNonce]);

  const todos = list?.items.filter((i) => i.severity !== "ok") ?? [];
  const oks = list?.items.filter((i) => i.severity === "ok") ?? [];
  const badge = list ? levelBadge(list.level) : null;

  return (
    <Card
      title="安全自查"
      actions={
        <button type="button" className="btn ghost sm" disabled={busy} onClick={load}>
          <RefreshCw size={13} className={busy ? "animate-spin" : ""} style={{ marginRight: 3 }} />
          {busy ? "正在检查…" : "刷新"}
        </button>
      }
    >
      <div className="stack">
        <ErrorDialog message={err} onClose={() => setErr("")} />
        {badge && (
          <div className="kv">
            <span className="muted">当前状态</span>
            <span>
              <Badge kind={badge.kind}>{badge.label}</Badge>
              {list?.checkedAt && (
                <span className="muted" style={{ marginLeft: 8, fontSize: 12 }}>
                  {formatWhen(list.checkedAt)}
                </span>
              )}
            </span>
          </div>
        )}

        <div className="callout info" style={{ fontSize: 12 }}>
          <div>这份清单只读本机配置与工作空间路径，全程在本机计算，不上报、零遥测。</div>
          <div style={{ marginTop: 6 }}>
            系统不会告诉本应用谁在监听剪贴板、谁在截屏或录屏，因此无法指认监听者或截屏调用方。
          </div>
          <div style={{ marginTop: 6 }}>
            剪贴板排除是给系统历史、云剪贴板和守规矩的管理器看的协作式约定，恶意程序可以无视。它不能防木马，也不会让截图变黑。
          </div>
        </div>

        {todos.length === 0 && list && (
          <div className="muted" style={{ fontSize: 12 }}>
            当前没有待处理项。下面折叠的是已通过的检查。
          </div>
        )}

        {todos.map((item) => (
          <div key={item.id} className={"callout " + findingCallout(item.severity)}>
            <div className="between" style={{ alignItems: "flex-start", gap: 8 }}>
              <strong>{item.title}</strong>
              <Badge kind={item.severity === "warn" ? "danger" : "warn"}>
                {item.severity === "warn" ? "建议处理" : "留意"}
              </Badge>
            </div>
            <div style={{ marginTop: 6 }}>{item.detail}</div>
            <div className="muted" style={{ marginTop: 6, fontSize: 12 }}>
              {item.advice}
            </div>
            {item.limitation && (
              <div className="hint" style={{ marginTop: 6 }}>
                {item.limitation}
              </div>
            )}
            {item.settingsAnchor && (
              <div style={{ marginTop: 8 }}>
                <button type="button" className="btn sm" onClick={() => onJump(item.settingsAnchor!)}>
                  前往对应设置
                </button>
              </div>
            )}
          </div>
        ))}

        {oks.length > 0 && (
          <div>
            <button type="button" className="btn ghost sm" onClick={() => setShowOk((v) => !v)}>
              {showOk ? "收起已通过的检查" : `已通过的检查（${oks.length}）`}
            </button>
            {showOk && (
              <div className="stack" style={{ marginTop: 8 }}>
                {oks.map((item) => (
                  <div key={item.id} className="callout good">
                    <div className="between">
                      <strong>{item.title}</strong>
                      <Badge kind="good">通过</Badge>
                    </div>
                    <div className="muted" style={{ marginTop: 6, fontSize: 12 }}>
                      {item.detail}
                    </div>
                    {item.limitation && (
                      <div className="hint" style={{ marginTop: 6 }}>
                        {item.limitation}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </Card>
  );
}

export function Settings() {
  const navigate = useNavigate();
  const {
    status,
    refresh,
    theme,
    setTheme,
    unlockAnimEnabled,
    setUnlockAnimEnabled,
    unlockAnimStyle,
    setUnlockAnimStyle,
    startUnlockAnim,
  } = useApp();
  const [activeTab, setActiveTab] = useState<SettingsTab>("general");
  const [checklistNonce, setChecklistNonce] = useState(0);
  const pendingAnchor = useRef<string | null>(null);

  const [oldPw, setOldPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [newPw2, setNewPw2] = useState("");
  const [graceDays, setGraceDays] = useState(String(status?.graceDays ?? 0));
  const [revealGrace, setRevealGrace] = useState("5");
  const [clipSec, setClipSec] = useState("20");
  const [histLimit, setHistLimit] = useState("10");
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);
  const [newRecovery, setNewRecovery] = useState("");
  const [bio, setBio] = useState<BiometricStatus | null>(null);
  const [bioPw, setBioPw] = useState("");

  useEffect(() => {
    setGraceDays(String(status?.graceDays ?? 0));
  }, [status?.graceDays]);

  useEffect(() => {
    api.getRevealSettings().then((s) => {
      setRevealGrace(String(s.revealGraceMinutes));
      setClipSec(String(s.clipboardClearSeconds));
      setHistLimit(String(s.accountHistoryLimit));
    }).catch(() => {});
    api.biometricStatus().then(setBio).catch(() => setBio(null));
  }, []);

  useEffect(() => {
    const anchor = pendingAnchor.current;
    if (!anchor) return;
    pendingAnchor.current = null;
    const id = window.setTimeout(() => scrollToSettingAnchor(anchor), 0);
    return () => window.clearTimeout(id);
  }, [activeTab]);

  function jumpToSetting(anchor: string) {
    const tab = SETTING_ANCHOR_TAB[anchor] ?? "security";
    pendingAnchor.current = anchor;
    if (tab !== activeTab) {
      setActiveTab(tab);
    } else {
      scrollToSettingAnchor(anchor);
      pendingAnchor.current = null;
    }
  }

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

  const NAV_ITEMS: { id: SettingsTab; label: string; icon: typeof Palette; danger?: boolean }[] = [
    { id: "general", label: "通用与外观", icon: Palette },
    { id: "security", label: "安全与凭据", icon: ShieldCheck },
    { id: "workspace", label: "工作空间与服务", icon: FolderKanban },
    { id: "about", label: "关于与更新", icon: Rocket },
    { id: "danger", label: "高危与还原", icon: AlertTriangle, danger: true },
  ];

  return (
    <div className="stack-lg">
      <PageHead title="设置" desc="工作空间安全、应用偏好与账户配置" />

      <ErrorDialog message={err} onClose={() => setErr("")} />
      {msg && <div className="callout info">{msg}</div>}

      <div className="settings-layout">
        {/* 左侧/顶部 分栏导航Tab */}
        <nav className="settings-nav" aria-label="设置导航">
          {NAV_ITEMS.map((item) => {
            const Icon = item.icon;
            const isActive = activeTab === item.id;
            return (
              <button
                key={item.id}
                type="button"
                className={
                  "settings-nav-item" +
                  (isActive ? " active" : "") +
                  (item.danger ? " danger-nav" : "")
                }
                onClick={() => {
                  setErr("");
                  setMsg("");
                  setActiveTab(item.id);
                }}
              >
                <Icon size={15} />
                <span>{item.label}</span>
              </button>
            );
          })}
        </nav>

        {/* 右侧主设置面板内容 */}
        <div className="settings-content">
          {/* TAB 1: 通用与外观 */}
          {activeTab === "general" && (
            <>
              <Card title="界面主题与色彩风格">
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

              <Card title="解锁开门动画">
                <div className="field">
                  <div className="between">
                    <div>
                      <FieldLabel
                        name="解锁过场动画"
                        tip="每次手动输入访问密码/恢复密钥解锁成功后播放开门过场动画。免验证静默解锁不会触发。"
                      />
                      <div className="hint">
                        关闭后解锁将直接进入主界面。播放中可点击任意处或按 Esc / 空格 / 回车跳过。
                      </div>
                    </div>
                    <button
                      type="button"
                      className={"switch" + (unlockAnimEnabled ? "" : " off")}
                      onClick={() => setUnlockAnimEnabled(!unlockAnimEnabled)}
                    />
                  </div>

                  {unlockAnimEnabled && (
                    <div style={{ marginTop: 12, display: "flex", flexDirection: "column", gap: 10 }}>
                      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                        <span className="hint" style={{ fontWeight: 600, color: "var(--text-1)" }}>
                          选择动画风格
                        </span>
                        <button
                          type="button"
                          className="btn ghost sm"
                          onClick={() => startUnlockAnim(unlockAnimStyle)}
                          title="预览当前选中的动画风格"
                          style={{ display: "inline-flex", alignItems: "center", gap: 6 }}
                        >
                          <Play size={13} /> 预览当前动画
                        </button>
                      </div>

                      <div
                        style={{
                          display: "grid",
                          gridTemplateColumns: "repeat(auto-fit, minmax(210px, 1fr))",
                          gap: 10,
                        }}
                      >
                        {UNLOCK_ANIM_STYLES.map((st) => {
                          const active = unlockAnimStyle === st.id;
                          return (
                            <div
                              key={st.id}
                              role="button"
                              tabIndex={0}
                              onClick={() => setUnlockAnimStyle(st.id)}
                              style={{
                                padding: "12px 14px",
                                borderRadius: "var(--radius, 10px)",
                                border: active
                                  ? "1.5px solid var(--accent, #6366f1)"
                                  : "1px solid var(--border, rgba(255, 255, 255, 0.08))",
                                background: active
                                  ? "var(--accent-dim, rgba(99, 102, 241, 0.08))"
                                  : "var(--bg-card, rgba(255, 255, 255, 0.02))",
                                cursor: "pointer",
                                display: "flex",
                                flexDirection: "column",
                                gap: 6,
                                transition: "all 0.15s ease",
                              }}
                            >
                              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                                  <span style={{ fontSize: 18 }}>{st.icon}</span>
                                  <span style={{ fontWeight: 600, fontSize: 13, color: active ? "var(--accent)" : "var(--text-1)" }}>
                                    {st.label}
                                  </span>
                                </div>
                                <button
                                  type="button"
                                  className="btn ghost sm"
                                  style={{ padding: "2px 8px", fontSize: 11, height: 24 }}
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    setUnlockAnimStyle(st.id);
                                    startUnlockAnim(st.id);
                                  }}
                                  title={`试看${st.label}`}
                                >
                                  试看
                                </button>
                              </div>
                              <div style={{ fontSize: 11, color: "var(--text-3)", lineHeight: 1.4 }}>
                                {st.desc}
                              </div>
                            </div>
                          );
                        })}
                      </div>
                    </div>
                  )}
                </div>
              </Card>

              <Card title="系统与窗口行为">
                <div className="stack">
                  <div className="field">
                    <div className="between">
                      <div>
                        <FieldLabel
                          name="开机自启动"
                          tip="登录系统后自动打开本软件。Windows 写入当前用户启动项，macOS 写入登录启动项。若同时开启了免验证，会尝试把密钥加载进 ssh-agent。"
                        />
                        <div className="hint">
                          {status?.launchAtLoginSupported === false
                            ? "当前系统暂不支持开机自启动。"
                            : "示例：出差换电脑后重新装好，再打开此项；默认关闭。"}
                        </div>
                      </div>
                      <button
                        type="button"
                        className={"switch" + (status?.launchAtLogin ? "" : " off")}
                        disabled={busy || status?.launchAtLoginSupported === false}
                        onClick={() => toggleLaunch(!status?.launchAtLogin)}
                      />
                    </div>
                  </div>

                  <hr className="sep" style={{ margin: "4px 0" }} />

                  <div className="field">
                    <FieldLabel name="关闭主窗口动作" tip="点击主界面右上方关闭按钮时的响应规则。" />
                    <div className="muted" style={{ fontSize: 12, marginBottom: 6 }}>
                      当前设置：<strong>{closeActionLabel(status?.closeAction)}</strong>。可在确认框勾选「记住我的选择」。从托盘重新打开时，按免验证天数决定是否重新解锁。
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
                </div>
              </Card>
            </>
          )}

          {/* TAB 2: 安全与凭据 */}
          {activeTab === "security" && (
            <>
              <SecurityChecklistCard refreshNonce={checklistNonce} onJump={jumpToSetting} />

              <Card title="开机免验证 (Windows DPAPI)">
                <div className="stack">
                  <div className="field">
                    <FieldLabel
                      name="免验证天数"
                      tip="0 表示每次启动都要访问密码。1–30 表示在该天数内，本机可用 Windows DPAPI 记住解锁状态：开机后自动解锁并加载 agent。过期必须重新输入密码。换 Windows 用户或改密码会失效。"
                    />
                    <div className="row" style={{ marginTop: 4 }}>
                      <input
                        className="input"
                        type="number"
                        min={0}
                        max={30}
                        style={{ width: 88, minWidth: 88, flex: "0 0 88px" }}
                        value={graceDays}
                        onChange={(e) => setGraceDays(e.target.value)}
                      />
                      <button type="button" className="btn primary sm" disabled={busy} onClick={saveGrace}>
                        保存设置
                      </button>
                    </div>
                    <div className="hint" style={{ marginTop: 4 }}>
                      示例：填 7 表示一周内开机不用输密码；填 0 则每次都验证。
                    </div>
                  </div>
                  {status?.graceActive && status.graceExpiresAt && (
                    <div className="callout info" style={{ marginTop: 4 }}>
                      当前免验证有效至 {formatExpiry(status.graceExpiresAt)}，之后需重新输入访问密码。
                    </div>
                  )}
                  {!status?.graceActive && (status?.graceDays ?? 0) > 0 && (
                    <div className="callout warn" style={{ marginTop: 4 }}>
                      已设置免验证天数，但本机会话尚未建立或已过期。下次用访问密码解锁后会重新生效。
                    </div>
                  )}
                </div>
              </Card>

              <Card title="指纹 / 生物识别解锁">
                <div className="stack">
                  <div className="muted">
                    使用系统已录入的 Windows Hello / Touch ID，应用不会采集或保存指纹。开启须先输入访问密码。
                  </div>
                  {!bio?.available && (
                    <div className="callout warn" style={{ marginTop: 4 }}>
                      本机未检测到可用的指纹 / 人脸 / Windows Hello 硬件。
                    </div>
                  )}
                  {bio?.stale && (
                    <div className="callout warn" style={{ marginTop: 4 }}>
                      指纹凭据已失效，请用访问密码解锁后重新开启。
                    </div>
                  )}
                  <div className="field">
                    <FieldLabel name="启用指纹解锁" tip="开启后解锁工作空间可刷系统指纹，不必再输访问密码。密码与恢复密钥始终可回落。" />
                    {bio?.available && !bio.enabled && (
                      <div className="row" style={{ marginTop: 4 }}>
                        <input
                          className="input"
                          type="password"
                          placeholder="输入访问密码以开启"
                          value={bioPw}
                          onChange={(e) => setBioPw(e.target.value)}
                          style={{ flex: 1 }}
                        />
                        <button
                          type="button"
                          className="btn primary sm"
                          disabled={busy || !bioPw.trim()}
                          onClick={async () => {
                            setBusy(true);
                            setErr("");
                            try {
                              await api.biometricEnable(bioPw);
                              setBioPw("");
                              setBio(await api.biometricStatus());
                              setMsg("已开启指纹解锁");
                            } catch (e) {
                              setErr(errMessage(e));
                            } finally {
                              setBusy(false);
                            }
                          }}
                        >
                          <Fingerprint size={14} /> 开启
                        </button>
                      </div>
                    )}
                    {bio?.enabled && (
                      <div className="row" style={{ marginTop: 4 }}>
                        <span className="hint">已开启，解锁与查看验证码/账密可用系统指纹。</span>
                        <button
                          type="button"
                          className="btn ghost sm"
                          disabled={busy}
                          onClick={async () => {
                            setBusy(true);
                            setErr("");
                            try {
                              await api.biometricDisable();
                              setBio(await api.biometricStatus());
                              setMsg("已关闭指纹解锁");
                            } catch (e) {
                              setErr(errMessage(e));
                            } finally {
                              setBusy(false);
                            }
                          }}
                        >
                          关闭
                        </button>
                      </div>
                    )}
                  </div>
                  <div className="field">
                    <FieldLabel name="查看验证码 / 账密时允许指纹" tip="vault 已解锁后，查看一次性验证码或账户密码可用指纹代替访问密码。" />
                    <label className="row" style={{ marginTop: 4, gap: 8 }}>
                      <input
                        type="checkbox"
                        disabled={!bio?.enabled || busy}
                        checked={!!bio?.revealEnabled}
                        onChange={async (e) => {
                          setBusy(true);
                          try {
                            await api.setBiometricRevealEnabled(e.target.checked);
                            setBio(await api.biometricStatus());
                          } catch (err) {
                            setErr(errMessage(err));
                          } finally {
                            setBusy(false);
                          }
                        }}
                      />
                      <span className="hint">默认随主开关打开</span>
                    </label>
                  </div>
                  <div className="field">
                    <FieldLabel name="取回 TOTP 原始密钥也允许指纹" tip="这是导出级操作。默认仍要求访问密码，打开后可用指纹替代。" />
                    <label className="row" style={{ marginTop: 4, gap: 8 }}>
                      <input
                        type="checkbox"
                        disabled={!bio?.enabled || busy}
                        checked={!!bio?.revealSecret}
                        onChange={async (e) => {
                          setBusy(true);
                          try {
                            await api.setBiometricRevealSecret(e.target.checked);
                            setBio(await api.biometricStatus());
                          } catch (err) {
                            setErr(errMessage(err));
                          } finally {
                            setBusy(false);
                          }
                        }}
                      />
                      <span className="hint">有风险，默认关闭</span>
                    </label>
                  </div>
                </div>
              </Card>

              <Card title="查看 OTP / 密码的免密时效">
                <div className="stack">
                  <div className="muted">独立于开机免验证。锁定或退出后立即失效。取回 TOTP 原始密钥仍每次都要密码。</div>
                  <div className="field" id="setting-reveal-grace">
                    <FieldLabel name="免密查看时效" tip="首次验证后，在该时间内再看验证码或账号密码不用重复输入。0 表示每次都验。" />
                    <div className="row" style={{ marginTop: 4 }}>
                      <select className="input" style={{ width: 160 }} value={revealGrace} onChange={(e) => setRevealGrace(e.target.value)}>
                        <option value="0">每次都验证</option>
                        <option value="1">1 分钟</option>
                        <option value="5">5 分钟（推荐）</option>
                        <option value="15">15 分钟</option>
                        <option value="30">30 分钟</option>
                      </select>
                      <button
                        type="button"
                        className="btn primary sm"
                        disabled={busy}
                        onClick={async () => {
                          setBusy(true);
                          try {
                            await api.setRevealGraceMinutes(Number(revealGrace));
                            setMsg("已保存免密查看时效");
                            setChecklistNonce((n) => n + 1);
                          } catch (e) {
                            setErr(errMessage(e));
                          } finally {
                            setBusy(false);
                          }
                        }}
                      >
                        保存
                      </button>
                    </div>
                  </div>
                  <div className="field" id="setting-clipboard-clear">
                    <label className="field-label">复制后清空剪贴板</label>
                    <div className="row">
                      <select className="input" style={{ width: 140 }} value={clipSec} onChange={(e) => setClipSec(e.target.value)}>
                        <option value="0">不清空</option>
                        <option value="10">10 秒</option>
                        <option value="20">20 秒</option>
                        <option value="60">60 秒</option>
                      </select>
                      <button
                        type="button"
                        className="btn sm"
                        disabled={busy}
                        onClick={async () => {
                          await api.setClipboardClearSeconds(Number(clipSec));
                          setMsg("已保存剪贴板清空时间");
                          setChecklistNonce((n) => n + 1);
                        }}
                      >
                        保存
                      </button>
                    </div>
                  </div>
                  <div className="field">
                    <label className="field-label">密码历史保留条数</label>
                    <div className="row">
                      <select className="input" style={{ width: 120 }} value={histLimit} onChange={(e) => setHistLimit(e.target.value)}>
                        <option value="5">5 条</option>
                        <option value="10">10 条</option>
                        <option value="20">20 条</option>
                      </select>
                      <button
                        type="button"
                        className="btn sm"
                        disabled={busy}
                        onClick={async () => {
                          await api.setAccountHistoryLimit(Number(histLimit));
                          setMsg("已保存历史条数上限");
                        }}
                      >
                        保存
                      </button>
                    </div>
                  </div>
                </div>
              </Card>

              <Card title="修改主访问密码">
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
                  <div style={{ marginTop: 4 }}>
                    <button className="btn primary sm" disabled={busy} onClick={changePassword}>
                      更新密码
                    </button>
                  </div>
                </div>
              </Card>

              <Card title="轮换灾难恢复密钥">
                <div className="stack">
                  <div className="muted">生成新的恢复密钥并作废旧的。请务必重新保存。</div>
                  {newRecovery ? (
                    <>
                      <div className="callout danger">⚠️ 仅显示一次，请立即保存：</div>
                      <div className="reckey">{newRecovery}</div>
                      <div className="row">
                        <button className="btn sm" onClick={() => writeClipboard(newRecovery, true)}>
                          复制
                        </button>
                        <button className="btn ghost sm" onClick={() => setNewRecovery("")}>
                          我已保存
                        </button>
                      </div>
                    </>
                  ) : (
                    <div>
                      <button className="btn danger sm" disabled={busy} onClick={rotate}>
                        轮换恢复密钥
                      </button>
                    </div>
                  )}
                </div>
              </Card>
            </>
          )}

          {/* TAB 3: 工作空间与服务 */}
          {activeTab === "workspace" && (
            <>
              <Card title="工作空间信息">
                <div className="stack">
                  <div className="kv" id="setting-workspace-path">
                    <span className="muted">存储路径</span>
                    <span className="mono">{status?.workspacePath ?? "—"}</span>
                  </div>
                  <div className="kv">
                    <span className="muted">SSH 配置</span>
                    <span>
                      <button type="button" className="btn ghost sm" onClick={() => navigate("/config")}>
                        <FileCog size={13} style={{ marginRight: 3 }} />
                        查看 / 用记事本编辑 →
                      </button>
                    </span>
                  </div>
                  <div className="kv">
                    <span className="muted">空间 ID</span>
                    <span className="mono">{status?.workspaceId ?? "—"}</span>
                  </div>
                  <div className="kv" id="setting-auto-lock">
                    <span className="muted">自动锁定</span>
                    <span>{status?.autoLockMinutes ?? 0} 分钟</span>
                  </div>
                  <div className="kv" id="setting-lock-on-sleep">
                    <span className="muted">休眠/锁屏锁定</span>
                    <span className="muted" style={{ fontSize: 12 }}>
                      由本机配置 lockOnSleep 控制，见上方安全自查建议
                    </span>
                  </div>
                </div>
              </Card>

              <Card title="GitHub PAT 个人访问令牌">
                <GithubPatSettings writesLocked={!!status?.writesLocked} />
              </Card>

              <Card title="数据备份与云端同步">
                <div className="stack">
                  <div className="muted" style={{ fontSize: 12 }}>
                    支持本地离线加密备份导出 (.gambackup) 与换机还原，以及 S3 / Cloudflare R2 兼容的端到端零知识加密云端同步。
                  </div>
                  <div>
                    <button
                      type="button"
                      className="btn primary sm"
                      onClick={() => navigate("/sync")}
                    >
                      <Cloud size={13} style={{ marginRight: 3 }} />
                      前往云端同步与备份管理 →
                    </button>
                  </div>
                </div>
              </Card>
            </>
          )}

          {/* TAB 4: 关于与更新 */}
          {activeTab === "about" && <AboutUpdateCard />}

          {/* TAB 5: 高危区与还原 */}
          {activeTab === "danger" && (
            <div className="danger-card card">
              <div className="card-head">
                <div className="card-title" style={{ display: "flex", alignItems: "center", gap: 5 }}>
                  <AlertTriangle size={14} />
                  高危操作区 · 出厂清空还原
                </div>
              </div>
              <div className="card-body">
                <FactoryResetPanel onDone={refresh} />
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
