import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Cloud, Eye, EyeOff, FolderPlus, Moon, Palette, Sun } from "lucide-react";
import { api, errMessage, type CloudRestorePreview, type S3Config } from "../lib/ipc";
import { useApp } from "../store";
import { APP_LANG } from "../lib/config";

const CREATE_STEPS = ["选择工作空间", "设置访问密码", "保存恢复密钥", "回填校验", "完成"];
const RESTORE_STEPS = ["选择工作空间", "云存储", "恢复密钥", "新访问密码", "完成"];

type Mode = "create" | "restore";

function samePath(a: string, b: string): boolean {
  return a.trim().replace(/[\\/]+$/, "").toLowerCase() === b.trim().replace(/[\\/]+$/, "").toLowerCase();
}

function emptyS3(): S3Config {
  return {
    endpoint: "",
    bucket: "",
    region: "auto",
    accessKeyId: "",
    secretAccessKey: "",
    prefix: "gam-sync/",
  };
}

export function InitWizard() {
  const { refresh, theme, toggleTheme } = useApp();
  const [mode, setMode] = useState<Mode | null>(null);
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
  const [s3, setS3] = useState<S3Config>(emptyS3);
  const [showSecret, setShowSecret] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; msg: string } | null>(null);
  const [restoreKey, setRestoreKey] = useState("");
  const [preview, setPreview] = useState<CloudRestorePreview | null>(null);
  const [restoreResult, setRestoreResult] = useState<string | null>(null);
  const [includeRepos, setIncludeRepos] = useState(false);

  const steps = mode === "restore" ? RESTORE_STEPS : CREATE_STEPS;
  const strength = pwStrength(pw);
  const alreadyCreated = !!createdPath;
  const switchingPath = alreadyCreated && !samePath(path, createdPath);

  function resetErr() {
    setErr("");
  }

  function chooseMode(next: Mode) {
    resetErr();
    setMode(next);
    setStep(0);
  }

  function backToMode() {
    resetErr();
    setMode(null);
    setStep(0);
  }

  async function pickDir() {
    resetErr();
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
    resetErr();
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
    resetErr();
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
    resetErr();
    const norm = (s: string) => s.replace(/[\s-]/g, "").toUpperCase();
    if (norm(confirm) !== norm(recovery)) return setErr("恢复密钥不一致，请仔细核对（大小写与连字符可忽略）");
    setStep(4);
  }

  function s3Ready(): boolean {
    return !!(s3.endpoint.trim() && s3.bucket.trim() && s3.accessKeyId.trim() && s3.secretAccessKey.trim());
  }

  async function testS3() {
    resetErr();
    if (!s3Ready()) return setErr("请先填写 Endpoint、Bucket、Access Key 与 Secret Key");
    setBusy(true);
    setTestResult(null);
    try {
      const ms = await api.testCloudSyncConfig(s3);
      setTestResult({ ok: true, msg: `连接成功，探测耗时 ${ms} ms` });
    } catch (e) {
      setTestResult({ ok: false, msg: errMessage(e) });
    } finally {
      setBusy(false);
    }
  }

  async function importS3File() {
    resetErr();
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        title: "导入 S3/R2 配置文件",
        filters: [{ name: "GAM S3/R2 配置", extensions: ["json"] }],
      });
      if (!selected || typeof selected !== "string") return;
      const cfg = await api.importS3Config(selected);
      setS3(cfg);
      setTestResult({ ok: true, msg: "已导入配置文件，可先测试连通性再继续" });
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  function goRestoreCloud() {
    resetErr();
    if (!s3Ready()) return setErr("请先填写完整的云存储连接信息");
    setStep(2);
  }

  async function doPreview() {
    resetErr();
    if (!restoreKey.trim()) return setErr("请输入旧设备的恢复密钥");
    setBusy(true);
    setPreview(null);
    try {
      const r = await api.previewCloudRestore(s3, restoreKey);
      setPreview(r);
      if (!r.hasManifest) {
        setErr("已解开云端头部，但还没有加密清单。请先在旧设备上打开本应用并执行一次「推送到云端」。");
      }
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function doRestore() {
    resetErr();
    if (pw.length < 8) return setErr("访问密码至少 8 位");
    if (pw !== pw2) return setErr("两次输入的密码不一致");
    if (!preview?.hasManifest) return setErr("请先验证恢复密钥并确认云端有可还原的数据");
    setBusy(true);
    try {
      const r = await api.restoreFromCloud({
        path,
        password: pw,
        recoveryKey: restoreKey,
        syncConfig: s3,
        includeRepos,
      });
      setRestoreResult(r.message);
      setStep(4);
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  function goBack() {
    if (!mode) return;
    resetErr();
    if (step <= 0) {
      backToMode();
      return;
    }
    if (mode === "create" && step >= 4) return;
    if (mode === "restore" && step >= 4) return;
    setStep((s) => s - 1);
  }

  function jumpTo(i: number) {
    if (!mode || i >= step || i >= 4) return;
    resetErr();
    setStep(i);
  }

  const title =
    !mode ? "选择初始化方式"
    : mode === "restore"
      ? ["选择加密工作空间目录", "填写云存储连接", "输入旧设备恢复密钥", "设置本机访问密码", "已从云端恢复"][step]
      : [
          "选择加密工作空间目录",
          "设置日常访问密码",
          "妥善保存恢复密钥",
          "回填校验恢复密钥",
          "工作空间已就绪",
        ][step];

  const desc =
    !mode ? "全新使用请创建工作空间；另一台设备已有数据时，用那台设备的恢复密钥从云端还原。"
    : mode === "restore"
      ? [
          "还原后的身份与密钥会加密写入这个本地目录。请选择尚未初始化的空目录。",
          "填写与旧设备完全相同的 R2 / S3 连接信息（含路径前缀）。",
          "输入旧设备初始化时保存的恢复密钥，用于解开云端数据。",
          "为本机设置日常访问密码，可以与旧设备不同。旧恢复密钥仍然有效。",
          "已用同一把主密钥重建工作空间，之后两台设备可以继续云同步。",
        ][step]
      : [
          "所有密钥和配置文件均加密存储在你的本地目录中，绝不明文上传。",
          "密码用于本机日常解锁；请牢记，一旦遗忘只能使用恢复密钥重置。",
          "恢复密钥仅显示一次，是换机或忘记密码时唯一的救命稻草，请离线保存。",
          "请把刚才保存的恢复密钥完整填入下方，确认你已真正抄录备份。",
          "加密工作空间已初始化完成，你可以直接进入应用。",
        ][step];

  return (
    <div className="window">
      <div className="body">
        <aside className="sidebar" style={{ width: 176, flex: "0 0 176px", padding: "10px 8px" }}>
          {mode && (
            <div
              className="muted"
              style={{ padding: "6px 8px 10px", fontSize: 12, fontWeight: 600, color: "var(--sidebar-text)" }}
            >
              {mode === "restore"
                ? APP_LANG === "en"
                  ? "Cloud Restore"
                  : "从云端恢复"
                : APP_LANG === "en"
                  ? "Workspace Init"
                  : "工作空间初始化"}
            </div>
          )}
          {mode && (
            <>
              <div className="nav-group">向导步骤</div>
              <div className="stack" style={{ gap: 3 }}>
                {steps.map((s, i) => (
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
            </>
          )}

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
              width: mode === "restore" ? 560 : 520,
              maxWidth: "100%",
              padding: "20px 22px",
              boxShadow: "var(--shadow-lg)",
            }}
          >
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", marginBottom: 16 }}>
              <div>
                <div className="title-lg" style={{ fontSize: 15, fontWeight: 700 }}>{title}</div>
                <div className="muted" style={{ marginTop: 4, lineHeight: 1.45 }}>{desc}</div>
              </div>
              {mode && (
                <span className="badge info" style={{ flexShrink: 0, marginLeft: 12 }}>
                  {step + 1} / {steps.length}
                </span>
              )}
            </div>

            <div>
              {!mode && (
                <div className="stack" style={{ gap: 10 }}>
                  <button type="button" className="mode-pick" onClick={() => chooseMode("create")}>
                    <FolderPlus size={18} />
                    <span>
                      <strong>创建新工作空间</strong>
                      <small>首次使用。生成本机访问密码与恢复密钥。</small>
                    </span>
                  </button>
                  <button type="button" className="mode-pick" onClick={() => chooseMode("restore")}>
                    <Cloud size={18} />
                    <span>
                      <strong>从云端恢复</strong>
                      <small>已有另一台设备。填写云存储并导入那台设备的恢复密钥。</small>
                    </span>
                  </button>
                </div>
              )}

              {mode && step === 0 && (
                <div className="stack" style={{ gap: 10 }}>
                  {mode === "create" && alreadyCreated && (
                    <div className="callout warn">
                      已在「{createdPath}」创建过工作空间。更换目录会创建新空间，原目录不会自动删除。
                    </div>
                  )}
                  {mode === "restore" && (
                    <div className="callout info">请选择一个尚未初始化的空目录作为本机工作空间。</div>
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

              {mode === "create" && step === 1 && (
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
                  <PasswordFields pw={pw} pw2={pw2} setPw={setPw} setPw2={setPw2} strength={strength} onEnter={doInit} />
                  <div className="callout info">🔑 访问密码仅保存在本地进行加密校验，请牢记。</div>
                </div>
              )}

              {mode === "create" && step === 2 && (
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

              {mode === "create" && step === 3 && (
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

              {mode === "create" && step === 4 && (
                <div className="stack" style={{ textAlign: "center", padding: "16px 0", gap: 6 }}>
                  <div style={{ fontSize: 36, marginBottom: 2 }}>🎉</div>
                  <div className="title-lg" style={{ fontSize: 16 }}>加密工作空间已就绪</div>
                  <div className="muted">你可以开始导入现有 SSH 密钥，或者新建多平台 Git 身份。</div>
                </div>
              )}

              {mode === "restore" && step === 1 && (
                <div className="stack" style={{ gap: 10 }}>
                  <div className="row" style={{ gap: 6, flexWrap: "wrap" }}>
                    <span className="muted" style={{ fontSize: 11, alignSelf: "center" }}>快速预设</span>
                    <button type="button" className="btn ghost sm" onClick={() => setS3((p) => ({ ...p, endpoint: "https://<account_id>.r2.cloudflarestorage.com", region: "auto", prefix: p.prefix || "gam-sync/" }))}>
                      Cloudflare R2
                    </button>
                    <button type="button" className="btn ghost sm" onClick={() => setS3((p) => ({ ...p, endpoint: "https://s3.us-east-1.amazonaws.com", region: "us-east-1", prefix: p.prefix || "gam-sync/" }))}>
                      AWS S3
                    </button>
                    <button type="button" className="btn ghost sm" onClick={() => setS3((p) => ({ ...p, endpoint: "http://127.0.0.1:9000", region: "us-east-1", prefix: p.prefix || "gam-sync/" }))}>
                      MinIO
                    </button>
                  </div>
                  <div className="grid grid-cols-2 gap-3">
                    <div className="field">
                      <label className="field-label">存储端点</label>
                      <input className="input" placeholder="https://<account_id>.r2.cloudflarestorage.com" value={s3.endpoint} onChange={(e) => setS3({ ...s3, endpoint: e.target.value })} />
                    </div>
                    <div className="field">
                      <label className="field-label">存储桶</label>
                      <input className="input" placeholder="my-git-vault-backup" value={s3.bucket} onChange={(e) => setS3({ ...s3, bucket: e.target.value })} />
                    </div>
                    <div className="field">
                      <label className="field-label">区域</label>
                      <input className="input" placeholder="auto 或 us-east-1" value={s3.region} onChange={(e) => setS3({ ...s3, region: e.target.value })} />
                    </div>
                    <div className="field">
                      <label className="field-label">路径前缀</label>
                      <input className="input" placeholder="gam-sync/" value={s3.prefix} onChange={(e) => setS3({ ...s3, prefix: e.target.value })} />
                    </div>
                    <div className="field">
                      <label className="field-label">Access Key ID</label>
                      <input className="input" placeholder="AKIA..." value={s3.accessKeyId} onChange={(e) => setS3({ ...s3, accessKeyId: e.target.value })} />
                    </div>
                    <div className="field">
                      <label className="field-label">Secret Access Key</label>
                      <div style={{ position: "relative" }}>
                        <input
                          type={showSecret ? "text" : "password"}
                          className="input"
                          placeholder="Secret Key"
                          value={s3.secretAccessKey}
                          onChange={(e) => setS3({ ...s3, secretAccessKey: e.target.value })}
                          style={{ paddingRight: 32 }}
                        />
                        <button type="button" className="btn ghost sm" onClick={() => setShowSecret((v) => !v)} style={{ position: "absolute", right: 4, top: 4, padding: "2px 6px" }}>
                          {showSecret ? <EyeOff size={13} /> : <Eye size={13} />}
                        </button>
                      </div>
                    </div>
                  </div>
                  {testResult && (
                    <div className={testResult.ok ? "callout good" : "callout danger"}>
                      {testResult.ok ? `✅ ${testResult.msg}` : `❌ ${testResult.msg}`}
                    </div>
                  )}
                  <div className="row">
                    <button type="button" className="btn ghost sm" disabled={busy} onClick={importS3File}>
                      导入配置文件
                    </button>
                    <button type="button" className="btn ghost sm" disabled={busy} onClick={testS3}>
                      {busy ? "正在探测…" : "测试连通性"}
                    </button>
                  </div>
                </div>
              )}

              {mode === "restore" && step === 2 && (
                <div className="stack" style={{ gap: 10 }}>
                  <div className="callout warn">
                    请输入旧设备初始化时保存的恢复密钥（GAM1-…）。填错无法解密云端数据。
                  </div>
                  <textarea
                    className="input mono"
                    placeholder="粘贴恢复密钥（如 GAM1-...）"
                    value={restoreKey}
                    onChange={(e) => {
                      setRestoreKey(e.target.value);
                      setPreview(null);
                    }}
                    style={{ minHeight: 76 }}
                  />
                  <div>
                    <button type="button" className="btn ghost sm" disabled={busy} onClick={doPreview}>
                      {busy ? "正在验证…" : "验证恢复密钥并预览"}
                    </button>
                  </div>
                  {preview && preview.hasManifest && (
                    <div className="stack" style={{ gap: 10 }}>
                      <div className="callout good">
                        已解开工作空间 {preview.workspaceId.slice(0, 8)}…
                        {preview.updatedAt ? ` · 云端更新于 ${new Date(preview.updatedAt).toLocaleString()}` : ""}
                        <br />
                        {preview.identityCount} 个身份 · {preview.keyCount} 把密钥
                        {preview.repoCount > 0 ? ` · 云端另有 ${preview.repoCount} 条其他机器的仓库登记` : ""}
                      </div>
                      <RestoreScope includeRepos={includeRepos} setIncludeRepos={setIncludeRepos} repoCount={preview.repoCount} />
                    </div>
                  )}
                </div>
              )}

              {mode === "restore" && step === 3 && (
                <div className="stack" style={{ gap: 10 }}>
                  {preview && (
                    <div className="stack" style={{ gap: 10 }}>
                      <div className="callout info">
                        即将还原 {preview.identityCount} 个身份、{preview.keyCount} 把密钥到「{path}」。
                        {includeRepos
                          ? ` 并导入 ${preview.repoCount} 条仓库登记（路径按本机重写归属）。`
                          : " 不会导入其他机器上的仓库登记。"}
                      </div>
                      <RestoreScope includeRepos={includeRepos} setIncludeRepos={setIncludeRepos} repoCount={preview.repoCount} />
                    </div>
                  )}
                  <PasswordFields pw={pw} pw2={pw2} setPw={setPw} setPw2={setPw2} strength={strength} onEnter={doRestore} />
                  <div className="callout info">本机访问密码可以重新设置；旧恢复密钥保持有效，请继续妥善保存。</div>
                </div>
              )}

              {mode === "restore" && step === 4 && (
                <div className="stack" style={{ textAlign: "center", padding: "16px 0", gap: 6 }}>
                  <div style={{ fontSize: 36, marginBottom: 2 }}>🎉</div>
                  <div className="title-lg" style={{ fontSize: 16 }}>已从云端恢复</div>
                  <div className="muted">{restoreResult || "工作空间已用同一把主密钥重建。"}</div>
                  {preview && (
                    <div className="muted">
                      {preview.identityCount} 个身份 · {preview.keyCount} 把密钥
                      {includeRepos ? ` · ${preview.repoCount} 个仓库` : " · 未导入仓库登记"}
                    </div>
                  )}
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
                disabled={!mode || (mode === "create" && step >= 4) || (mode === "restore" && step >= 4)}
                onClick={goBack}
              >
                {mode && step === 0 ? "返回" : "上一步"}
              </button>

              <div>
                {mode && step === 0 && (
                  <button type="button" className="btn primary" onClick={checkPath}>
                    下一步
                  </button>
                )}
                {mode === "create" && step === 1 && (
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
                {mode === "create" && step === 2 && (
                  <button type="button" className="btn primary" onClick={() => setStep(3)}>
                    我已安全保存，继续
                  </button>
                )}
                {mode === "create" && step === 3 && (
                  <button type="button" className="btn primary" onClick={verifyConfirm}>
                    校验并完成
                  </button>
                )}
                {mode === "create" && step === 4 && (
                  <button type="button" className="btn primary" onClick={() => refresh()}>
                    进入主界面
                  </button>
                )}
                {mode === "restore" && step === 1 && (
                  <button type="button" className="btn primary" onClick={goRestoreCloud}>
                    下一步
                  </button>
                )}
                {mode === "restore" && step === 2 && (
                  <button
                    type="button"
                    className="btn primary"
                    disabled={!preview?.hasManifest}
                    onClick={() => {
                      resetErr();
                      setStep(3);
                    }}
                  >
                    确认预览，继续
                  </button>
                )}
                {mode === "restore" && step === 3 && (
                  <button type="button" className="btn primary" disabled={busy} onClick={doRestore}>
                    {busy ? "正在解密并还原…" : "恢复到本机"}
                  </button>
                )}
                {mode === "restore" && step === 4 && (
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

function PasswordFields({
  pw,
  pw2,
  setPw,
  setPw2,
  strength,
  onEnter,
}: {
  pw: string;
  pw2: string;
  setPw: (v: string) => void;
  setPw2: (v: string) => void;
  strength: string;
  onEnter: () => void;
}) {
  return (
    <>
      <div className="field">
        <label className="field-label">访问密码</label>
        <input className="input" type="password" value={pw} autoFocus onChange={(e) => setPw(e.target.value)} />
        <div className="hint">密码强度：{strength}（至少 8 位，包含字母、数字或符号更佳）</div>
      </div>
      <div className="field">
        <label className="field-label">确认访问密码</label>
        <input
          className="input"
          type="password"
          value={pw2}
          onChange={(e) => setPw2(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && onEnter()}
        />
      </div>
    </>
  );
}

function RestoreScope({
  includeRepos,
  setIncludeRepos,
  repoCount,
}: {
  includeRepos: boolean;
  setIncludeRepos: (v: boolean) => void;
  repoCount: number;
}) {
  return (
    <div className="card" style={{ padding: "12px 14px" }}>
      <div className="field-label" style={{ marginBottom: 10, fontWeight: 600 }}>
        自定义恢复范围
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <label
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: 10,
            padding: "10px 12px",
            borderRadius: 6,
            border: "1px solid var(--border)",
            backgroundColor: "rgba(0, 0, 0, 0.02)",
            cursor: "not-allowed",
            opacity: 0.85,
          }}
        >
          <input
            type="checkbox"
            checked
            disabled
            readOnly
            style={{ marginTop: 2, flexShrink: 0, cursor: "not-allowed" }}
          />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontWeight: 500, fontSize: 13, display: "flex", alignItems: "center", gap: 8 }}>
              <span>身份、密钥、SSH 配置</span>
              <span className="badge muted sm">必须恢复</span>
            </div>
            <div className="muted sm" style={{ marginTop: 3, fontSize: 12 }}>
              包含全局身份信息、私钥加密副本以及本地 SSH Bridge 引导配置。
            </div>
          </div>
        </label>

        <label
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: 10,
            padding: "10px 12px",
            borderRadius: 6,
            border: includeRepos ? "1px solid var(--accent, #4f46e5)" : "1px solid var(--border)",
            backgroundColor: includeRepos ? "rgba(99, 102, 241, 0.05)" : "transparent",
            cursor: "pointer",
            transition: "all 0.15s ease",
          }}
        >
          <input
            type="checkbox"
            checked={includeRepos}
            onChange={(e) => setIncludeRepos(e.target.checked)}
            style={{ marginTop: 2, flexShrink: 0, cursor: "pointer" }}
          />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontWeight: 500, fontSize: 13, display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
              <span>仓库登记</span>
              {repoCount > 0 && <span className="badge sm">云端 {repoCount} 条</span>}
              <span className="muted sm" style={{ fontWeight: 400 }}>（默认不恢复）</span>
            </div>
            <div className="muted sm" style={{ marginTop: 3, fontSize: 12, lineHeight: 1.4 }}>
              其他机器上的本地目录路径在本机通常无效。不勾选可保持本机仓库列表干净；恢复后也可随时通过「扫描入库」重新识别本机代码库。
            </div>
          </div>
        </label>
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
