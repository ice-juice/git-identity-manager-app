import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Sun, Moon, Palette } from "lucide-react";
import {
  api,
  errMessage,
  type AuthResult,
  type ConfigPreview,
  type CreateIdentityResult,
  type Identity,
  type KeyRecord,
} from "../lib/ipc";
import { useApp } from "../store";
import { FieldLabel } from "../ui/common";

const STEPS = ["填信息", "密钥", "上传公钥", "验证", "归属标识"];
const DRAFT_KEY = "gam.newIdentity.draft";

const PLATFORMS = [
  { id: "github", label: "GitHub", host: "github.com", alias: "github", keysUrl: "https://github.com/settings/keys" },
  { id: "gitlab", label: "GitLab", host: "gitlab.com", alias: "gitlab", keysUrl: "https://gitlab.com/-/user_settings/ssh_keys" },
  { id: "gitea", label: "Gitea / 自建", host: "", alias: "git", keysUrl: "" },
  { id: "other", label: "其他", host: "", alias: "git", keysUrl: "" },
] as const;

interface Draft {
  step: number;
  maxReached: number;
  platform: string;
  name: string;
  email: string;
  gitUserName: string;
  hostAlias: string;
  aliasTouched: boolean;
  realHost: string;
  hostTouched: boolean;
  user: string;
  keyMode: "new" | "reuse";
  keyId: string;
  keyComment: string;
  commentTouched: boolean;
  strictMode: boolean;
  pendingKeyId: string;
  pendingPub: string;
  staged: boolean;
  created: CreateIdentityResult | null;
  uploaded: boolean;
  auth: AuthResult | null;
  ownersText: string;
}

function emptyDraft(): Draft {
  return {
    step: 0,
    maxReached: 0,
    platform: "github",
    name: "",
    email: "",
    gitUserName: "",
    hostAlias: "github-",
    aliasTouched: false,
    realHost: "github.com",
    hostTouched: false,
    user: "git",
    keyMode: "new",
    keyId: "",
    keyComment: "",
    commentTouched: false,
    strictMode: false,
    pendingKeyId: "",
    pendingPub: "",
    staged: false,
    created: null,
    uploaded: false,
    auth: null,
    ownersText: "",
  };
}

function loadDraft(): Draft {
  try {
    const raw = sessionStorage.getItem(DRAFT_KEY);
    if (!raw) return emptyDraft();
    return { ...emptyDraft(), ...JSON.parse(raw) };
  } catch {
    return emptyDraft();
  }
}

function saveDraft(d: Draft) {
  sessionStorage.setItem(DRAFT_KEY, JSON.stringify(d));
}

function slug(s: string): string {
  return s
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function suggestAlias(platform: string, name: string): string {
  const p = PLATFORMS.find((x) => x.id === platform) ?? PLATFORMS[0];
  const n = slug(name);
  return n ? `${p.alias}-${n}` : `${p.alias}-`;
}

export function suggestKeyComment(email: string, name: string, gitUserName: string): string {
  const em = email.trim();
  if (em) return em;
  const n = name.trim();
  if (n) return `${n}@gam`;
  const g = gitUserName.trim();
  if (g) return `${g.split(/\s+/).join(".")}@gam`;
  return "";
}

export function NewIdentity() {
  const nav = useNavigate();
  const { status, theme, toggleTheme, writesLocked, startupNote } = useApp();
  const workspacePath = status?.workspacePath;
  const [d, setDraft] = useState<Draft>(loadDraft);
  const [identities, setIdentities] = useState<Identity[]>([]);
  const [keys, setKeys] = useState<KeyRecord[]>([]);
  const [preview, setPreview] = useState<ConfigPreview | null>(null);
  const [copied, setCopied] = useState(false);
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);
  const [patConfigured, setPatConfigured] = useState(false);
  const [patInput, setPatInput] = useState("");
  const [patLogin, setPatLogin] = useState("");

  const set = (p: Partial<Draft>) => {
    setDraft((prev) => {
      const next = { ...prev, ...p };
      saveDraft(next);
      return next;
    });
  };

  useEffect(() => {
    (async () => {
      try {
        setIdentities(await api.listIdentities());
        setKeys(await api.listKeys());
        const pat = await api.githubPatStatus();
        setPatConfigured(pat.configured);
        if (pat.configured) {
          try {
            setPatLogin(await api.testGithubPat());
          } catch {
            setPatLogin("");
          }
        }
      } catch (e) {
        setErr(errMessage(e));
      }
    })();
  }, []);

  const locked = !!d.created || d.staged;
  const plat = PLATFORMS.find((p) => p.id === d.platform) ?? PLATFORMS[0];
  const aliasClash = identities.some(
    (i) => i.hostAlias.toLowerCase() === d.hostAlias.trim().toLowerCase() && i.id !== d.created?.identity.id,
  );
  const nameClash = identities.some(
    (i) => i.name.toLowerCase() === d.name.trim().toLowerCase() && i.id !== d.created?.identity.id,
  );

  const identityFileHint = useMemo(() => {
    const root = (workspacePath || "<工作空间>").replace(/\\/g, "/");
    const stem = d.name.trim() ? `id_ed25519_${d.name.trim()}` : "id_ed25519_<备注名>";
    return `${root}/ssh-keys/${stem}${d.strictMode ? ".pub" : ""}`;
  }, [workspacePath, d.name, d.strictMode]);

  const publicKey = d.created?.publicOpenssh || d.pendingPub || keys.find((k) => k.id === d.keyId)?.publicOpenssh || "";

  function applyPlatform(id: string) {
    if (locked) return;
    const p = PLATFORMS.find((x) => x.id === id) ?? PLATFORMS[0];
    set({
      platform: id,
      hostAlias: d.aliasTouched ? d.hostAlias : suggestAlias(id, d.name),
      realHost: d.hostTouched || !p.host ? d.realHost : p.host,
    });
  }

  function onName(v: string) {
    if (locked) return;
    set({
      name: v,
      hostAlias: d.aliasTouched ? d.hostAlias : suggestAlias(d.platform, v),
      keyComment: d.commentTouched ? d.keyComment : suggestKeyComment(d.email, v, d.gitUserName),
    });
  }

  function onEmail(v: string) {
    if (locked) return;
    set({
      email: v,
      keyComment: d.commentTouched ? d.keyComment : suggestKeyComment(v, d.name, d.gitUserName),
    });
  }

  function onGitUserName(v: string) {
    if (locked) return;
    set({
      gitUserName: v,
      keyComment: d.commentTouched ? d.keyComment : suggestKeyComment(d.email, d.name, v),
    });
  }

  async function discardDraftStaging() {
    if (d.created || !d.staged) return;
    const keyId = d.pendingKeyId || d.keyId;
    try {
      await api.abortIdentityDraft({
        hostAlias: d.hostAlias.trim(),
        keyId: keyId || "",
      });
    } catch {
      /* 取消时尽力卸下临时密钥，失败不阻断退出 */
    }
    set({ staged: false, auth: null });
  }

  async function exitWizard() {
    if (!d.created) {
      await discardDraftStaging();
    } else {
      sessionStorage.removeItem(DRAFT_KEY);
    }
    nav("/");
  }

  async function loadPreview() {
    if (!d.hostAlias.trim() || !d.realHost.trim()) return;
    try {
      setPreview(
        await api.previewConfig({
          alias: d.hostAlias.trim(),
          hostName: d.realHost.trim(),
          user: d.user.trim() || "git",
          identityFile: identityFileHint,
          identitiesOnly: true,
        }),
      );
    } catch {
      /* 预览失败不阻断 */
    }
  }

  useEffect(() => {
    if (d.step === 1) loadPreview();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [d.step, d.hostAlias, d.realHost, d.user, identityFileHint]);

  function validateInfo(): string | null {
    if (!d.name.trim()) return "请填写备注名";
    if (!d.hostAlias.trim()) return "请填写 Host 别名";
    if (!/^[A-Za-z0-9._-]+$/.test(d.hostAlias.trim())) return "Host 别名只能包含字母、数字、点、下划线和连字符";
    if (!d.realHost.trim()) return "请填写真实主机名";
    if (aliasClash) return `Host 别名「${d.hostAlias}」已被占用`;
    if (nameClash) return `备注名「${d.name}」已存在`;
    return null;
  }

  async function ensurePublicKey(): Promise<{ keyId: string; pub: string }> {
    if (d.created?.publicOpenssh) {
      return { keyId: d.created.identity.keyId || d.pendingKeyId || d.keyId, pub: d.created.publicOpenssh };
    }
    if (d.pendingPub && (d.pendingKeyId || d.keyId)) {
      return { keyId: d.pendingKeyId || d.keyId, pub: d.pendingPub };
    }
    if (d.keyMode === "reuse") {
      const k = keys.find((x) => x.id === d.keyId);
      if (!k) throw new Error("请选择要复用的密钥");
      set({ pendingKeyId: k.id, pendingPub: k.publicOpenssh });
      return { keyId: k.id, pub: k.publicOpenssh };
    }
    const k = await api.generateKey(
      d.keyComment.trim() || suggestKeyComment(d.email, d.name, d.gitUserName),
      d.name.trim() || undefined,
    );
    setKeys(await api.listKeys());
    set({ pendingKeyId: k.id, pendingPub: k.publicOpenssh, keyId: k.id });
    return { keyId: k.id, pub: k.publicOpenssh };
  }

  async function ensureCreated(keyId?: string) {
    if (d.created) return d.created;
    const resolved = keyId || (d.keyMode === "reuse" ? d.keyId : d.pendingKeyId) || undefined;
    const r = await api.createIdentity({
      name: d.name.trim(),
      platform: d.platform,
      hostAlias: d.hostAlias.trim(),
      realHost: d.realHost.trim(),
      user: d.user.trim() || "git",
      email: d.email.trim() || undefined,
      gitUserName: d.gitUserName.trim() || undefined,
      strictMode: d.strictMode,
      keyId: resolved,
      keyComment: d.keyComment.trim() || suggestKeyComment(d.email, d.name, d.gitUserName) || undefined,
      owners: [],
    });
    set({ created: r, pendingKeyId: r.identity.keyId ?? resolved ?? "", pendingPub: r.publicOpenssh });
    return r;
  }

  async function goTo(target: number) {
    if (target === d.step) return;
    setErr("");
    setMsg("");
    if (target < d.step) {
      set({ step: target });
      return;
    }
    const infoErr = validateInfo();
    if (target >= 1 && infoErr) {
      set({ step: 0 });
      setErr(infoErr);
      return;
    }
    if (target >= 2) {
      if (d.keyMode === "reuse" && !d.keyId && !d.pendingKeyId) {
        set({ step: 1 });
        return setErr("请选择要复用的密钥");
      }
      setBusy(true);
      try {
        if (!d.keyComment.trim() && d.keyMode === "new") {
          set({ keyComment: suggestKeyComment(d.email, d.name, d.gitUserName) });
        }
        await ensurePublicKey();
      } catch (e) {
        set({ step: 1 });
        setErr(errMessage(e));
        setBusy(false);
        return;
      }
      setBusy(false);
    }
    set({ step: target, maxReached: Math.max(d.maxReached, target) });
  }

  async function copyPub() {
    if (!publicKey) return;
    await navigator.clipboard.writeText(publicKey.trim());
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }

  async function openKeysPage() {
    setErr("");
    if (!plat.keysUrl) return setErr("该平台请自行打开 SSH 公钥设置页");
    try {
      await api.openUrl(plat.keysUrl);
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  async function savePat() {
    const token = patInput.trim();
    if (!token) return setErr("请粘贴 GitHub PAT");
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      await api.setGithubPat(token);
      const login = await api.testGithubPat();
      setPatConfigured(true);
      setPatLogin(login);
      setPatInput("");
      setMsg(`PAT 已保存，当前 GitHub 账号 ${login}`);
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function uploadViaPat() {
    const keyId = d.created?.identity.keyId || d.pendingKeyId || d.keyId;
    if (!keyId) return;
    if (!patConfigured) return setErr("请先在下方保存 GitHub PAT，或改用复制公钥手动添加");
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      await api.uploadPublicKey(keyId, d.name.trim() || "git-account-manager");
      set({ uploaded: true });
      setMsg("公钥已通过 PAT 上传到 GitHub");
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function verify() {
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      const keyId = (await ensurePublicKey()).keyId;
      await api.stageIdentityDraft({
        name: d.name.trim(),
        hostAlias: d.hostAlias.trim(),
        realHost: d.realHost.trim(),
        user: d.user.trim() || "git",
        strictMode: d.strictMode,
        keyId,
      });
      const r = await api.testConnection(d.hostAlias.trim());
      let owners = d.ownersText;
      if (r.account) {
        const lines = owners
          .split(/\r?\n/)
          .map((s) => s.trim())
          .filter(Boolean);
        if (!lines.some((l) => l.toLowerCase() === r.account!.toLowerCase())) {
          owners = [r.account, ...lines].join("\n");
        }
      }
      set({ staged: true, pendingKeyId: keyId, auth: r, ownersText: owners });
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function importOrgs() {
    setErr("");
    setBusy(true);
    try {
      const orgs = await api.listGithubOrgs();
      const have = new Set(
        d.ownersText
          .split(/\r?\n/)
          .map((s) => s.trim().toLowerCase())
          .filter(Boolean),
      );
      const extra = orgs.filter((o) => !have.has(o.toLowerCase()));
      const cur = d.ownersText.trim();
      set({ ownersText: extra.length ? (cur ? `${cur}\n${extra.join("\n")}` : extra.join("\n")) : cur });
      setMsg(orgs.length ? `已拉取 ${orgs.length} 个组织，可再编辑` : "该账号下没有组织");
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function finish() {
    setErr("");
    setBusy(true);
    try {
      const created = await ensureCreated(d.pendingKeyId || d.keyId || undefined);
      const owners = d.ownersText
        .split(/\r?\n/)
        .map((s) => s.trim())
        .filter(Boolean);
      for (const o of owners) {
        await api.addOwner(created.identity.id, o);
      }
      sessionStorage.removeItem(DRAFT_KEY);
      nav("/");
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const nameMatch = d.auth?.account
    ? d.auth.account.toLowerCase() === d.name.trim().toLowerCase() ||
      d.auth.account.toLowerCase() === d.gitUserName.trim().toLowerCase()
    : null;

  function jumpStep(i: number) {
    if (i === d.step) return;
    if (i <= d.maxReached) goTo(i);
  }

  return (
    <div className="wizard-overlay">
      <div className="wizard">
        <div className="wizard-head">
          <div className="between">
            <div>
              <div className="title-lg">新建身份</div>
              <div className="muted">未点「完成」前可随时返回修改；填写内容会自动记住。点「开始验证」才会临时写入 Host 并把私钥加载进 agent。</div>
              {writesLocked && (
                <div className="callout warn sm" style={{ marginTop: 8 }}>
                  {startupNote || "正在同步云端数据，请稍后再创建身份。"}
                </div>
              )}
            </div>
            <div className="row" style={{ gap: 6 }}>
            <button
              type="button"
              className="btn ghost sm"
              onClick={exitWizard}
            >
              退出
            </button>
            <button
              type="button"
              className="btn ghost sm"
              style={{ display: "inline-flex", gap: 5, padding: "2px 7px" }}
              title="切换界面风格"
              onClick={toggleTheme}
            >
              {theme === "light" ? <Sun size={13} /> : theme === "dark" ? <Moon size={13} /> : <Palette size={13} />}
              <span style={{ fontSize: 11 }}>{theme === "light" ? "浅色" : theme === "dark" ? "深色" : "黛蓝"}</span>
            </button>
            </div>
          </div>
          <div className="steps">
            {STEPS.map((s, i) => (
              <div
                key={s}
                className={"step" + (i === d.step ? " active" : i < d.step || i <= d.maxReached ? " done" : "")}
                onClick={() => jumpStep(i)}
                style={i <= d.maxReached && i !== d.step ? { cursor: "pointer" } : undefined}
              >
                <span className="n">{i < d.step || (i <= d.maxReached && i !== d.step) ? "✓" : i + 1}</span>
                {s}
              </div>
            ))}
          </div>
        </div>

        <div className="wizard-body">
          {d.step === 0 && (
            <div className="stack">
              {d.created && <div className="callout warn">⚠️ 身份与 SSH 配置已写入，平台 / 备注名 / 别名 / 主机 / SSH User 不可再改。其它步骤仍可返回查看。</div>}
              {d.staged && !d.created && <div className="callout warn">已临时写入 Host 并加载密钥，平台 / 备注名 / 别名 / 主机不可再改。取消创建会撤回 Host，并从 agent 卸下这把尚未绑定的密钥。</div>}
              <div className="field">
                <FieldLabel
                  name="平台"
                  tip="决定默认主机和公钥设置页。GitHub / GitLab 会自动填 HostName；自建 Gitea 需自己填真实主机。"
                />
                <div className="choice-row">
                  {PLATFORMS.map((p) => (
                    <button
                      key={p.id}
                      type="button"
                      className={"choice" + (d.platform === p.id ? " on" : "")}
                      disabled={locked}
                      onClick={() => applyPlatform(p.id)}
                    >
                      {p.label}
                    </button>
                  ))}
                </div>
                <div className="hint">示例：个人 GitHub 选 GitHub；公司自建选 Gitea / 自建。</div>
              </div>
              <div className="grid c2">
                <div className="field">
                  <FieldLabel
                    name="备注名"
                    tip="只在本软件里区分身份，也会用作工作空间 ssh-keys/ 下的文件名 id_ed25519_<备注名>。不是 GitHub 登录名，可以和账号名不同。"
                  />
                  <input className="input" value={d.name} disabled={locked} onChange={(e) => onName(e.target.value)} placeholder="例如 nova-labs" />
                  <div className="hint">示例：nova-labs、kiln-ops、polar-box</div>
                </div>
                <div className="field">
                  <FieldLabel
                    name="提交邮箱（可选）"
                    tip="写入该仓库的 git config user.email，只影响 commit 作者邮箱，不影响 SSH 登录。克隆或切换身份时自动带上。"
                  />
                  <input
                    className="input"
                    value={d.email}
                    disabled={locked}
                    onChange={(e) => onEmail(e.target.value)}
                    placeholder="例如 nova.reyes@example.com"
                  />
                  <div className="hint">示例：nova.reyes@example.com（建议与平台账号邮箱一致）</div>
                </div>
                <div className="field">
                  <FieldLabel
                    name="提交姓名 user.name（可选）"
                    tip="写入该仓库的 git config user.name，是 commit 上显示的作者名。不是 SSH 用户，也不是 Host 别名。不填则沿用本机全局 git 配置。"
                  />
                  <input
                    className="input"
                    value={d.gitUserName}
                    disabled={locked}
                    onChange={(e) => onGitUserName(e.target.value)}
                    placeholder="例如 Nova Reyes"
                  />
                  <div className="hint">示例：Nova Reyes（commit 里看到的名字）</div>
                </div>
                <div className="field">
                  <FieldLabel
                    name="SSH User"
                    tip="写入 ~/.ssh/config 的 User。GitHub / GitLab / Gitea 必须填 git，不是你的账号名。填错会导致连不上。只有自建服务器要求系统用户名时才改。"
                  />
                  <input
                    className="input mono"
                    value={d.user}
                    disabled={locked}
                    onChange={(e) => set({ user: e.target.value })}
                    placeholder="例如 git"
                  />
                  <div className="hint">示例：git（GitHub/GitLab 保持这个即可）</div>
                </div>
                <div className="field">
                  <FieldLabel
                    name="Host 别名"
                    tip="SSH config 里的 Host 短名。以后用 git@别名:owner/repo.git 克隆，就会自动用这把密钥。多账号靠不同别名区分，不要都写成 github.com。"
                  />
                  <input
                    className="input mono"
                    value={d.hostAlias}
                    disabled={locked}
                    placeholder="例如 github-nova-labs"
                    onChange={(e) => set({ aliasTouched: true, hostAlias: e.target.value })}
                  />
                  <div className="hint">示例：github-nova-labs、gitlab-kiln</div>
                  {aliasClash && <div className="hint" style={{ color: "var(--red)" }}>此别名已被占用</div>}
                </div>
                <div className="field">
                  <FieldLabel
                    name="真实主机 HostName"
                    tip="SSH 真正连接的服务器地址。别名只是本地称呼，HostName 才是 github.com 或你的自建域名。"
                  />
                  <input
                    className="input mono"
                    value={d.realHost}
                    disabled={locked}
                    placeholder="例如 github.com"
                    onChange={(e) => set({ hostTouched: true, realHost: e.target.value })}
                  />
                  <div className="hint">示例：github.com、gitlab.com、git.example.com</div>
                </div>
              </div>
            </div>
          )}

          {d.step === 1 && (
            <div className="stack">
              {locked && <div className="callout info">密钥与 config 已写入，密钥选项已锁定。可返回查看，不会重复创建。</div>}
              <div className="field">
                <FieldLabel
                  name="密钥来源"
                  tip="新建会生成一把带随机强口令的 ed25519 并加密入库。复用则使用「密钥管理」里已导入的钥匙，适合收编现有 id_ed25519。"
                />
                <div className="choice-row">
                  <button
                    type="button"
                    className={"choice grow" + (d.keyMode === "new" ? " on" : "")}
                    disabled={locked}
                    onClick={() => set({ keyMode: "new", pendingKeyId: "", pendingPub: "" })}
                  >
                    <strong>新建 ed25519</strong>
                    <div className="muted sm">自动生成强随机口令，保存到工作空间 ssh-keys/</div>
                  </button>
                  <button
                    type="button"
                    className={"choice grow" + (d.keyMode === "reuse" ? " on" : "")}
                    disabled={locked}
                    onClick={() => set({ keyMode: "reuse", pendingKeyId: "", pendingPub: "" })}
                  >
                    <strong>复用库内密钥</strong>
                    <div className="muted sm">在「密钥管理」导入后可在此选择</div>
                  </button>
                </div>
                <div className="hint">示例：第一次用这个账号选「新建」；已有 ~/.ssh 钥匙则先导入再选「复用」。</div>
              </div>
              {d.keyMode === "new" && (
                <div className="field">
                  <FieldLabel
                    name="密钥注释 comment"
                    tip="写在公钥末尾的备注，方便在 GitHub SSH keys 列表里认出是哪把钥匙。不影响认证。"
                  />
                  <input
                    className="input"
                    value={d.keyComment}
                    disabled={locked}
                    placeholder="例如 nova.reyes@example.com"
                    onChange={(e) => set({ commentTouched: true, keyComment: e.target.value })}
                  />
                  <div className="hint">
                    默认取提交邮箱；未填邮箱则用备注名@gam，再否则用提交姓名。可手动改。
                  </div>
                </div>
              )}
              {d.keyMode === "reuse" && (
                <div className="field">
                  <FieldLabel name="选择密钥" tip="从工作空间已入库的密钥里挑一把绑定到这个身份。指纹应和你要使用的账号公钥一致。" />
                  {keys.length === 0 ? (
                    <div className="muted">库内还没有密钥，请先到「密钥管理」导入，或改选新建。</div>
                  ) : (
                    <select className="input" value={d.keyId} disabled={locked} onChange={(e) => set({ keyId: e.target.value, pendingPub: "", pendingKeyId: "" })}>
                      <option value="">请选择…</option>
                      {keys.map((k) => (
                        <option key={k.id} value={k.id}>
                          {k.name} · {k.algorithm} · {k.fingerprint.slice(0, 24)}
                        </option>
                      ))}
                    </select>
                  )}
                  <div className="hint">示例：选择指纹与 GitHub 上已登记公钥相同的那一把</div>
                </div>
              )}
              <div className="field">
                <div className="between">
                  <div>
                    <FieldLabel
                      name="严格模式"
                      tip="关闭（默认）：带口令的私钥和公钥都写到工作空间 ssh-keys/，IdentityFile 指向私钥。开启：该目录只留 .pub，私钥只在解锁后注入 ssh-agent；Git 通过 ssh 向 agent 要签名，不直接读私钥文件。未解锁或未加载 agent 时 git 会失败。"
                    />
                    <div className="hint">示例：日常用默认关闭；对高敏感账号可开启</div>
                  </div>
                  <button
                    type="button"
                    className={"switch" + (d.strictMode ? "" : " off")}
                    disabled={locked}
                    onClick={() => !locked && set({ strictMode: !d.strictMode })}
                  />
                </div>
              </div>
              <div className="callout info">IdentityFile 将指向真实路径 {identityFileHint}。点「开始验证」才会备份并写入 config。</div>
              {preview?.diff && (
                <div>
                  <div className="section-title">将写入的 diff（预览，尚未落盘）</div>
                  <pre className="code">{preview.diff}</pre>
                </div>
              )}
            </div>
          )}

          {d.step === 2 && (
            <div className="stack">
              <div className="field">
                <FieldLabel
                  name="公钥"
                  tip="把整行公钥添加到对应平台的 SSH keys。平台只需要公钥；私钥留在本地/工作空间。添加后才能通过 ssh -T 验证。"
                />
                <div className="code">{publicKey || "（尚未生成，请返回上一步）"}</div>
                <div className="hint">示例：ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA… nova.reyes@example.com</div>
              </div>
              <div className="row" style={{ flexWrap: "wrap" }}>
                <button className="btn" onClick={copyPub} disabled={!publicKey}>
                  {copied ? "已复制" : "复制公钥"}
                </button>
                {plat.keysUrl && (
                  <button className="btn" onClick={openKeysPage}>
                    打开 {plat.label} SSH 设置页
                  </button>
                )}
                {d.platform === "github" && (
                  <button className="btn primary" disabled={busy || d.uploaded || !publicKey || !patConfigured} onClick={uploadViaPat}>
                    {d.uploaded ? "已上传" : "用 PAT 自动上传"}
                  </button>
                )}
              </div>
              {d.platform === "github" && (
                <div className="field">
                  <FieldLabel
                    name="GitHub PAT"
                    tip="classic token 需要 write:public_key 或 admin:public_key，才能代你把公钥添加到 GitHub。令牌只进加密库，界面不会回显。也可跳过，改用上方「复制公钥」手动添加。"
                  />
                  {patConfigured ? (
                    <div className="callout good sm">
                      已保存 PAT{patLogin ? `（GitHub 账号 ${patLogin}）` : ""}，可直接自动上传。也可到「设置」里更换或清除。
                    </div>
                  ) : (
                    <div className="stack" style={{ gap: 8 }}>
                      <div className="muted sm">
                        还没配置 PAT。到 GitHub → Settings → Developer settings → Personal access tokens 新建 classic token，勾选 <code>write:public_key</code>（或 <code>admin:public_key</code>），粘贴到下面保存。
                      </div>
                      <input
                        className="input mono"
                        type="password"
                        autoComplete="off"
                        placeholder="ghp_… 或 github_pat_…"
                        value={patInput}
                        onChange={(e) => setPatInput(e.target.value)}
                      />
                      <div className="row" style={{ flexWrap: "wrap" }}>
                        <button type="button" className="btn primary sm" disabled={busy || !patInput.trim()} onClick={savePat}>
                          保存 PAT
                        </button>
                        <button type="button" className="btn ghost sm" onClick={() => api.openUrl("https://github.com/settings/tokens")}>
                          打开 GitHub 令牌页
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              )}
              <hr className="sep" />
              <div className="grid-sum">
                <div>
                  <div className="muted">备注名</div>
                  <div style={{ fontWeight: 600 }}>{d.name || "—"}</div>
                </div>
                <div>
                  <div className="muted">Host 别名</div>
                  <div className="mono" style={{ fontWeight: 600 }}>
                    {d.hostAlias || "—"}
                  </div>
                </div>
                <div>
                  <div className="muted">私钥策略</div>
                  <div style={{ fontWeight: 600 }}>{d.strictMode ? "严格模式 · ssh-keys 只留公钥，私钥进 agent" : "默认 · 带口令私钥保存在工作空间 ssh-keys/"}</div>
                </div>
                <div>
                  <div className="muted">config</div>
                  <div style={{ fontWeight: 600 }}>{d.created ? (d.created.configVerified ? "已写入，ssh -G 通过" : "已写入，ssh -G 未通过") : d.staged ? "已临时写入，待完成登记" : "尚未写入，点开始验证才会落盘"}</div>
                </div>
              </div>
              {d.created?.configBackup && <div className="muted sm">已备份原 config：{d.created.configBackup}</div>}
            </div>
          )}

          {d.step === 3 && (
            <div className="stack">
              <div className="field">
                <FieldLabel
                  name="连接验证"
                  tip="此时尚未登记身份。会临时写入 Host、把私钥加载进 ssh-agent，再执行 ssh -T。点「完成」才真正创建身份；取消则撤回 Host 并从 agent 卸下这把尚未绑定的密钥。"
                />
                <div className="muted">将执行 ssh -T git@{d.hostAlias || "别名"}。严格模式会先把私钥注入 agent，取消创建时再卸下。</div>
                <div className="hint">示例：成功时类似「Hi nova-labs! You've successfully authenticated」</div>
              </div>
              <div className="row">
                <button className="btn primary" disabled={busy} onClick={verify}>
                  {busy ? "验证中…" : d.auth ? "重新验证" : "开始验证"}
                </button>
              </div>
              {d.auth && (
                <div className={"callout " + (d.auth.ok ? "good" : "danger")}>
                  <div>
                    <div>{d.auth.message}</div>
                    {d.auth.account && (
                      <div className="muted sm" style={{ marginTop: 6 }}>
                        实测账号 {d.auth.account}
                        {nameMatch === false && " · 与备注名/提交姓名不一致，请确认是否连到了正确账号"}
                        {nameMatch === true && " · 与填写信息一致"}
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>
          )}

          {d.step === 4 && (
            <div className="stack">
              <div className="field">
                <FieldLabel
                  name="归属标识"
                  tip="这个身份名下的 owner / 组织 / 路径前缀。粘贴仓库地址时优先按这里查表，精确匹配置信度最高。支持通配 acme-*，以及自建多段路径 git.internal/platform。一行一条。"
                />
                <textarea
                  className="input mono"
                  placeholder={"例如：\nnova-labs\nacme-*\ngit.internal/platform"}
                  value={d.ownersText}
                  onChange={(e) => set({ ownersText: e.target.value })}
                />
                <div className="hint">示例：nova-labs（账号）、kiln-org（组织）、acme-*（通配）、git.internal/platform（多段路径）</div>
              </div>
              {d.platform === "github" && (
                <div>
                  <button className="btn" disabled={busy} onClick={importOrgs}>
                    从 GitHub 组织列表导入（需已配置 PAT）
                  </button>
                </div>
              )}
            </div>
          )}

          {msg && <div className="callout info" style={{ marginTop: 16 }}>{msg}</div>}
          {err && <div className="err-text">{err}</div>}
        </div>

        <div className="wizard-foot">
          <div className="row" style={{ gap: 8 }}>
          <button type="button" className="btn ghost" onClick={exitWizard}>
            取消并返回
          </button>
          {d.step > 0 && (
            <button type="button" className="btn ghost" onClick={() => goTo(d.step - 1)}>
              上一步
            </button>
          )}
          </div>
          {d.step < 4 && (
            <button type="button" className="btn primary" disabled={busy || writesLocked} onClick={() => goTo(d.step + 1)}>
              {busy ? "处理中…" : d.step === 2 ? "下一步验证" : d.step === 3 ? (d.auth ? "下一步" : "跳过验证") : "下一步"}
            </button>
          )}
          {d.step === 4 && (
            <button type="button" className="btn primary" disabled={busy || writesLocked} onClick={finish}>
              {busy ? "保存中…" : "完成"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
