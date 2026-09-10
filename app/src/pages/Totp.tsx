import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Eye, EyeOff, LayoutGrid, List, Plus, Scan, Copy, KeyRound, Trash2, Link2, ShieldCheck, Check } from "lucide-react";
import {
  api,
  errMessage,
  type BuiltinIconInfo,
  type GroupMeta,
  type ParsedTotpPreview,
  type ScreenHit,
  type TotpEntry,
} from "../lib/ipc";
import { copyWithClear, isNeedReauth } from "../lib/secretsUi";
import { PageHead, Empty, Badge, FieldLabel } from "../ui/common";
import { detectTotpInput } from "../lib/totpInput";
import { ReauthDialog } from "../ui/ReauthDialog";
import { IconMark } from "../ui/IconMark";
import { CountdownRing } from "../ui/CountdownRing";
import { GroupDialog } from "../ui/GroupDialog";
import { GroupPicker, appendGroupIfNew, resolveGroupName } from "../ui/GroupPicker";
import { useApp } from "../store";

const VIEW_KEY = "gam.totp.view";
const NOTE_MAX = 32;

function clipNote(note?: string | null) {
  const text = (note || "").trim();
  if (!text) return "";
  return text.length > NOTE_MAX ? `${text.slice(0, NOTE_MAX)}…` : text;
}

export function TotpPage() {
  const { writesLocked } = useApp();
  const [entries, setEntries] = useState<TotpEntry[]>([]);
  const [groups, setGroups] = useState<GroupMeta[]>([]);
  const [builtins, setBuiltins] = useState<BuiltinIconInfo[]>([]);
  const [q, setQ] = useState("");
  const [group, setGroup] = useState("全部");
  const [view, setView] = useState<"grid" | "list">(() => (localStorage.getItem(VIEW_KEY) === "list" ? "list" : "grid"));
  const [err, setErr] = useState("");
  const [codes, setCodes] = useState<Record<string, { code: string; remain: number; period: number }>>({});
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [reauth, setReauth] = useState<null | ((pw: string) => Promise<void>)>(null);
  const reauthCancel = useRef<(() => void) | null>(null);
  const [revealCfg, setRevealCfg] = useState({ grace: 5, clip: 20 });
  const [editor, setEditor] = useState<null | Partial<TotpEntry> & { secret?: string }>(null);
  const [secretDlg, setSecretDlg] = useState<null | { id: string; secret?: string; uri?: string; qr?: string }>(null);
  const [scanHits, setScanHits] = useState<ScreenHit[] | null>(null);
  const [groupDlg, setGroupDlg] = useState(false);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    const [list, icons] = await Promise.all([api.totpList(), api.iconListBuiltin()]);
    setEntries(list.entries);
    setGroups(list.groups);
    setBuiltins(icons);
  }, []);

  useEffect(() => {
    load().catch((e) => setErr(errMessage(e)));
  }, [load]);

  useEffect(() => {
    api.getRevealSettings()
      .then((s) => setRevealCfg({ grace: s.revealGraceMinutes, clip: s.clipboardClearSeconds }))
      .catch(() => {});
  }, []);

  useEffect(() => {
    const t = window.setInterval(() => {
      setCodes((prev) => {
        const next = { ...prev };
        for (const id of Object.keys(next)) {
          const item = next[id];
          if (item.remain <= 1) {
            api.totpGenerateCode(id).then((c) => {
              setCodes((cur) => ({ ...cur, [id]: { code: c.code, remain: c.remainingSeconds, period: c.period } }));
            }).catch(() => {
              setCodes((cur) => {
                const { [id]: _, ...rest } = cur;
                return rest;
              });
            });
          } else {
            next[id] = { ...item, remain: item.remain - 1 };
          }
        }
        return next;
      });
    }, 1000);
    return () => window.clearInterval(t);
  }, []);

  const filtered = useMemo(() => {
    const words = q.trim().toLowerCase().split(/\s+/).filter(Boolean);
    return entries
      .filter((e) => group === "全部" || (e.group || "未分组") === group)
      .filter((e) => {
        if (!words.length) return true;
        const blob = [e.issuer, e.account, e.note, e.group, e.url].filter(Boolean).join(" ").toLowerCase();
        return words.every((w) => blob.includes(w));
      })
      .sort((a, b) => a.sortOrder - b.sortOrder || a.issuer.localeCompare(b.issuer));
  }, [entries, q, group]);

  function setViewMode(mode: "grid" | "list") {
    setView(mode);
    localStorage.setItem(VIEW_KEY, mode);
  }

  async function withAuth<T>(fn: (pw?: string) => Promise<T>): Promise<T | undefined> {
    try {
      return await fn();
    } catch (e) {
      if (!isNeedReauth(e)) {
        setErr(errMessage(e));
        return;
      }
      return await new Promise<T | undefined>((resolve) => {
        reauthCancel.current = () => {
          reauthCancel.current = null;
          setReauth(null);
          resolve(undefined);
        };
        setReauth(() => async (pw: string) => {
          try {
            const r = await fn(pw);
            reauthCancel.current = null;
            setReauth(null);
            resolve(r);
          } catch (err) {
            throw new Error(errMessage(err));
          }
        });
      });
    }
  }

  async function reveal(id: string) {
    setErr("");
    const c = await withAuth((pw) => api.totpGenerateCode(id, pw));
    if (c) setCodes((m) => ({ ...m, [id]: { code: c.code, remain: c.remainingSeconds, period: c.period } }));
  }

  async function copyCode(id: string) {
    let cur = codes[id];
    if (!cur) {
      const c = await withAuth((pw) => api.totpGenerateCode(id, pw));
      if (!c) return;
      cur = { code: c.code, remain: c.remainingSeconds, period: c.period };
      setCodes((m) => ({ ...m, [id]: cur }));
    }
    await copyWithClear(cur.code.replace(/\s/g, ""));
    setCopiedId(id);
    window.setTimeout(() => {
      setCopiedId((prev) => (prev === id ? null : prev));
    }, 1500);
  }

  async function openSecret(id: string) {
    setSecretDlg({ id });
  }

  async function confirmSecret(pw: string) {
    if (!secretDlg) return;
    const sec = await api.totpRevealSecret(secretDlg.id, pw);
    setSecretDlg({
      id: secretDlg.id,
      secret: sec.secretBase32,
      uri: sec.otpauthUri,
      qr: sec.qrPngBase64,
    });
  }

  async function saveEditor() {
    if (!editor) return;
    if (editor.id && editor.hasSeed === false && !editor.secret?.trim()) {
      setErr("这条记录的种子已丢失，请重新填入密钥或 otpauth 链接。");
      return;
    }
    setBusy(true);
    try {
      const group = resolveGroupName(groups, editor.group);
      const created = appendGroupIfNew(groups, group);
      if (created) {
        await api.totpSaveGroups(created);
        setGroups(created);
      }
      const args = {
        id: editor.id,
        issuer: editor.issuer || "",
        account: editor.account || "",
        secret: editor.secret,
        note: clipNote(editor.note) || undefined,
        url: editor.url || undefined,
        group,
        algorithm: editor.algorithm || "SHA1",
        digits: editor.digits || 6,
        period: editor.period || 30,
        icon: editor.icon || undefined,
      };
      if (editor.id) await api.totpUpdate(args);
      else await api.totpAdd(args);
      setEditor(null);
      await load();
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function applyPreview(p: ParsedTotpPreview, secretFromUri?: string) {
    setEditor({
      issuer: p.issuer,
      account: p.account,
      algorithm: p.algorithm,
      digits: p.digits,
      period: p.period,
      icon: p.suggestedIcon || undefined,
      secret: secretFromUri || p.secret,
    });
  }

  async function importImage() {
    const path = await open({ filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "webp", "bmp"] }] });
    if (typeof path !== "string") return;
    try {
      const p = await api.totpImportFromImage(path);
      applyPreview(p);
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  async function scanScreen() {
    setBusy(true);
    try {
      const hits = await api.totpScanScreen();
      if (hits.length === 0) setErr("屏幕中未识别到 otpauth 二维码");
      else if (hits.length === 1) applyPreview({ ...hits[0].parsed, suggestedIcon: null, secret: detectTotpInput(hits[0].uri).secret });
      else setScanHits(hits);
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function saveGroup(name: string, color: string | null) {
    const next = [...groups, { name, color, sortOrder: groups.length }];
    await api.totpSaveGroups(next);
    setGroups(next);
    setGroup(name);
    setGroupDlg(false);
  }

  const groupTabs = ["全部", ...groups.map((g) => g.name), ...entries.map((e) => e.group || "未分组").filter((n, i, a) => n !== "未分组" && !groups.some((g) => g.name === n) && a.indexOf(n) === i), "未分组"];

  return (
    <div className="stack-lg">
      <PageHead
        title="2FA / TOTP"
        desc="默认掩码保护，点击眼睛查看验证码。支持卡片/列表切换。"
        actions={
          <>
            <div className="view-switcher">
              <button type="button" className={"view-btn" + (view === "grid" ? " on" : "")} onClick={() => setViewMode("grid")}>
                <LayoutGrid size={13} /> 卡片
              </button>
              <button type="button" className={"view-btn" + (view === "list" ? " on" : "")} onClick={() => setViewMode("list")}>
                <List size={13} /> 列表
              </button>
            </div>
            <button type="button" className="btn sm" disabled={busy} onClick={scanScreen}>
              <Scan size={13} /> 扫描屏幕
            </button>
            <button type="button" className="btn sm" onClick={importImage}>
              导入图片
            </button>
            <button type="button" className="btn primary sm" disabled={writesLocked} onClick={() => setEditor({})}>
              <Plus size={13} /> 添加
            </button>
          </>
        }
      />

      <div className="security-banner">
        <div className="security-banner-text">
          <ShieldCheck size={16} style={{ color: "var(--accent)", flexShrink: 0 }} />
          <span>
            <strong>默认掩码安全模式：</strong>
            验证码默认隐藏防窥；复制后写入剪贴板
            {revealCfg.clip > 0 ? `，${revealCfg.clip} 秒后自动清空` : ""}。
          </span>
        </div>
        <div className="security-banner-pills">
          {revealCfg.grace > 0 && <span className="sec-pill cyan">免密 {revealCfg.grace} 分钟</span>}
          <span className="sec-pill purple">端到端加密</span>
        </div>
      </div>

      <div className="row between">
        <div className="group-tabs">
          {groupTabs.map((g) => {
            const count = g === "全部" ? entries.length : entries.filter((e) => (e.group || "未分组") === g).length;
            const color = groups.find((item) => item.name === g)?.color;
            return (
              <button key={g} type="button" className={"group-tab" + (group === g ? " on" : "")} onClick={() => setGroup(g)}>
                {color && <span className="group-tab-dot" style={{ background: color }} />}
                <span>{g}</span>
                <span className="group-tab-count">{count}</span>
              </button>
            );
          })}
          <button
            type="button"
            className="group-tab dashed"
            disabled={writesLocked}
            onClick={() => setGroupDlg(true)}
          >
            + 新建分组
          </button>
        </div>
        <input className="input" style={{ maxWidth: 260 }} placeholder="搜索平台 / 账号 / 备注" value={q} onChange={(e) => setQ(e.target.value)} />
      </div>

      {err && <div className="callout danger sm">{err}</div>}

      {filtered.length === 0 ? (
        <Empty text="还没有 TOTP 条目。可以手动添加，或扫描屏幕二维码导入。" />
      ) : view === "grid" ? (
        <div className="totp-grid">
          {filtered.map((e) => (
            <TotpCard
              key={e.id}
              e={e}
              builtins={builtins}
              shown={codes[e.id]}
              copied={copiedId === e.id}
              onReveal={() => reveal(e.id)}
              onHide={() => setCodes((m) => { const n = { ...m }; delete n[e.id]; return n; })}
              onCopy={() => copyCode(e.id)}
              onSecret={() => openSecret(e.id)}
              onEdit={() => setEditor({ ...e })}
              onDelete={async () => {
                if (!window.confirm(`删除 ${e.issuer} / ${e.account}？`)) return;
                await api.totpDelete(e.id);
                await load();
              }}
              locked={writesLocked}
            />
          ))}
        </div>
      ) : (
        <div className="totp-list-card">
          {filtered.map((e) => {
            const shown = codes[e.id];
            const seedMissing = e.hasSeed === false;
            const isCopied = copiedId === e.id;
            return (
              <div key={e.id} className="totp-list-row">
                <div className="totp-list-identity">
                  <IconMark icon={e.icon} builtins={builtins} label={e.issuer} size={32} />
                  <div style={{ minWidth: 0, overflow: "hidden" }}>
                    <div className="row" style={{ gap: 6, flexWrap: "wrap" }}>
                      <span className="totp-card-issuer">{e.issuer}</span>
                      {e.group && <Badge kind="info">{e.group}</Badge>}
                      {e.url && (
                        <button
                          type="button"
                          className="btn ghost sm"
                          style={{ padding: "0 2px" }}
                          title="访问登录页面"
                          onClick={() => api.openUrl(e.url!)}
                        >
                          <Link2 size={12} />
                        </button>
                      )}
                    </div>
                    <div className="totp-card-account">{e.account}</div>
                    {e.note?.trim() && (
                      <div className="totp-card-note" title={e.note.trim()}>
                        {clipNote(e.note)}
                      </div>
                    )}
                  </div>
                </div>

                <div className="totp-list-code">
                  {shown ? (
                    <div
                      className="row"
                      style={{
                        background: "var(--gray-soft)",
                        padding: "4px 10px",
                        borderRadius: "8px",
                        gap: 10,
                        cursor: "pointer",
                      }}
                      title="点击快速复制验证码"
                      onClick={() => copyCode(e.id)}
                    >
                      <span className="totp-code" style={{ fontSize: "18px" }}>
                        {formatCode(shown.code)}
                      </span>
                      <CountdownRing
                        remain={shown.remain}
                        period={shown.period || e.period || 30}
                        size={26}
                      />
                    </div>
                  ) : (
                    <button type="button" className="btn sm" onClick={() => reveal(e.id)}>
                      <Eye size={12} /> 查看验证码
                    </button>
                  )}
                </div>

                <div className="totp-list-actions">
                  <button
                    type="button"
                    className={"btn sm " + (isCopied ? "good" : "primary")}
                    title={seedMissing ? "种子已丢失" : "复制验证码"}
                    disabled={seedMissing}
                    onClick={() => copyCode(e.id)}
                  >
                    {isCopied ? <Check size={12} /> : <Copy size={12} />}
                  </button>
                  {shown && (
                    <button
                      type="button"
                      className="btn sm"
                      title="隐藏验证码"
                      onClick={() =>
                        setCodes((m) => {
                          const n = { ...m };
                          delete n[e.id];
                          return n;
                        })
                      }
                    >
                      <EyeOff size={12} />
                    </button>
                  )}
                  <button
                    type="button"
                    className="btn sm"
                    title={seedMissing ? "种子已丢失，无法取回" : "密钥 / 二维码"}
                    disabled={seedMissing}
                    onClick={() => openSecret(e.id)}
                  >
                    <KeyRound size={12} />
                  </button>
                  <button
                    type="button"
                    className="btn sm"
                    disabled={writesLocked}
                    title="编辑条目"
                    onClick={() => setEditor({ ...e })}
                  >
                    编辑
                  </button>
                  <button
                    type="button"
                    className="btn sm danger"
                    disabled={writesLocked}
                    title="删除条目"
                    onClick={async () => {
                      if (!window.confirm(`删除 ${e.issuer} / ${e.account}？`)) return;
                      await api.totpDelete(e.id);
                      await load();
                    }}
                  >
                    <Trash2 size={12} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {reauth && (
        <ReauthDialog
          onCancel={() => reauthCancel.current?.()}
          onConfirm={async (pw) => {
            await reauth(pw);
          }}
        />
      )}

      {groupDlg && (
        <GroupDialog
          existing={groups.map((g) => g.name)}
          onCancel={() => setGroupDlg(false)}
          onConfirm={saveGroup}
        />
      )}

      {editor && (
        <Editor
          key={editor.id || editor.secret || "new"}
          value={editor}
          builtins={builtins}
          groups={groups}
          busy={busy}
          onChange={setEditor}
          onClose={() => setEditor(null)}
          onSave={saveEditor}
        />
      )}

      {secretDlg && (
        <div className="wizard-overlay">
          <div className="card" style={{ width: 420 }}>
            <div className="card-head"><div className="card-title">取回原始密钥</div></div>
            <div className="card-body stack">
              {!secretDlg.secret ? (
                <ReauthInner hint="导出种子不走免密时效，必须重新输入访问密码。" onConfirm={confirmSecret} onCancel={() => setSecretDlg(null)} />
              ) : (
                <>
                  {secretDlg.qr && <img alt="otpauth qr" src={`data:image/png;base64,${secretDlg.qr}`} style={{ width: 180, height: 180, margin: "0 auto", display: "block" }} />}
                  <div className="field">
                    <label className="field-label">Base32 密钥</label>
                    <div className="row">
                      <input className="input mono" readOnly value={secretDlg.secret} />
                      <button type="button" className="btn sm" onClick={() => copyWithClear(secretDlg.secret!)}>复制</button>
                    </div>
                  </div>
                  <div className="field">
                    <label className="field-label">otpauth 链接</label>
                    <div className="row">
                      <input className="input mono" readOnly value={secretDlg.uri} />
                      <button type="button" className="btn sm" onClick={() => copyWithClear(secretDlg.uri!)}>复制</button>
                    </div>
                  </div>
                  <button type="button" className="btn primary sm" onClick={() => setSecretDlg(null)}>关闭</button>
                </>
              )}
            </div>
          </div>
        </div>
      )}

      {scanHits && (
        <div className="wizard-overlay">
          <div className="card" style={{ width: 480 }}>
            <div className="card-head"><div className="card-title">屏幕识别结果</div></div>
            <div className="card-body stack">
              {scanHits.map((h, i) => (
                <button key={i} type="button" className="choice" onClick={() => { applyPreview({ ...h.parsed, suggestedIcon: null, secret: detectTotpInput(h.uri).secret }); setScanHits(null); }}>
                  <b>{h.parsed.issuer}</b> · {h.parsed.account}
                  <div className="muted">{h.display} · {h.parsed.algorithm} {h.parsed.digits}位</div>
                </button>
              ))}
              <div className="row" style={{ justifyContent: "flex-end" }}>
                <button type="button" className="btn ghost sm" onClick={() => setScanHits(null)}>取消</button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function formatCode(code: string) {
  if (code.length === 6) return `${code.slice(0, 3)} ${code.slice(3)}`;
  if (code.length === 8) return `${code.slice(0, 4)} ${code.slice(4)}`;
  return code;
}

function TotpCard({
  e,
  builtins,
  shown,
  copied,
  onReveal,
  onHide,
  onCopy,
  onSecret,
  onEdit,
  onDelete,
  locked,
}: {
  e: TotpEntry;
  builtins: BuiltinIconInfo[];
  shown?: { code: string; remain: number; period: number };
  copied?: boolean;
  onReveal: () => void;
  onHide: () => void;
  onCopy: () => void;
  onSecret: () => void;
  onEdit: () => void;
  onDelete: () => void;
  locked: boolean;
}) {
  const seedMissing = e.hasSeed === false;
  return (
    <div className="totp-card">
      <div className="totp-card-head">
        <div className="row" style={{ minWidth: 0, gap: 10 }}>
          <IconMark icon={e.icon} builtins={builtins} label={e.issuer} size={36} />
          <div style={{ minWidth: 0, overflow: "hidden" }}>
            <div className="totp-card-issuer" title={e.issuer}>
              {e.issuer}
            </div>
            <div className="totp-card-account" title={e.account}>
              {e.account}
            </div>
            {e.note?.trim() && (
              <div className="totp-card-note" title={e.note.trim()}>
                {clipNote(e.note)}
              </div>
            )}
          </div>
        </div>
        <div className="row" style={{ gap: 4 }}>
          {e.group && <Badge kind="info">{e.group}</Badge>}
          {e.url && (
            <button
              type="button"
              className="btn ghost sm"
              title="访问登录页面"
              onClick={() => api.openUrl(e.url!)}
            >
              <Link2 size={13} />
            </button>
          )}
        </div>
      </div>

      {seedMissing && (
        <div className="callout danger sm">种子已丢失，请编辑并重新填入密钥，或删除后重新导入。</div>
      )}

      <div
        className={"totp-code-box" + (shown ? " revealed" : "")}
        title={shown ? "点击快捷复制验证码" : undefined}
        onClick={shown ? onCopy : undefined}
      >
        {shown ? (
          <>
            <span className="totp-code">{formatCode(shown.code)}</span>
            <CountdownRing remain={shown.remain} period={shown.period || e.period || 30} />
          </>
        ) : (
          <>
            <span className="totp-code masked">••••••</span>
            <button
              type="button"
              className="btn sm primary"
              disabled={seedMissing}
              onClick={(ev) => {
                ev.stopPropagation();
                onReveal();
              }}
            >
              <Eye size={13} /> 查看
            </button>
          </>
        )}
      </div>

      <div className="totp-card-actions">
        <button
          type="button"
          className={"btn sm " + (copied ? "good" : "primary")}
          style={{ flex: 1 }}
          disabled={seedMissing}
          onClick={onCopy}
        >
          {copied ? (
            <>
              <Check size={13} /> 已复制
            </>
          ) : (
            <>
              <Copy size={13} /> 复制验证码
            </>
          )}
        </button>
        {shown && (
          <button
            type="button"
            className="btn sm"
            title="隐藏验证码"
            onClick={onHide}
          >
            <EyeOff size={13} />
          </button>
        )}
        <button
          type="button"
          className="btn sm"
          title={seedMissing ? "种子已丢失，无法取回" : "取回原始密钥或导出二维码"}
          disabled={seedMissing || locked}
          onClick={onSecret}
        >
          <KeyRound size={13} />
        </button>
        <button
          type="button"
          className="btn sm"
          disabled={locked}
          title="编辑条目"
          onClick={onEdit}
        >
          编辑
        </button>
        <button
          type="button"
          className="btn sm danger"
          disabled={locked}
          title="删除条目"
          onClick={onDelete}
        >
          <Trash2 size={13} />
        </button>
      </div>
    </div>
  );
}

function ReauthInner({ hint, onConfirm, onCancel }: { hint: string; onConfirm: (pw: string) => Promise<void>; onCancel: () => void }) {
  const [pw, setPw] = useState("");
  const [err, setErr] = useState("");
  return (
    <div className="stack">
      <div className="muted">{hint}</div>
      <input className="input" type="password" placeholder="访问密码" value={pw} onChange={(e) => setPw(e.target.value)} />
      {err && <div className="callout danger sm">{err}</div>}
      <div className="row">
        <button type="button" className="btn ghost sm" onClick={onCancel}>取消</button>
        <button
          type="button"
          className="btn primary sm"
          onClick={async () => {
            try {
              await onConfirm(pw);
            } catch (e) {
              setErr(errMessage(e));
            }
          }}
        >
          验证并显示
        </button>
      </div>
    </div>
  );
}

function Editor({
  value, builtins, groups, busy, onChange, onClose, onSave,
}: {
  value: Partial<TotpEntry> & { secret?: string };
  builtins: BuiltinIconInfo[];
  groups: GroupMeta[];
  busy: boolean;
  onChange: (v: Partial<TotpEntry> & { secret?: string }) => void;
  onClose: () => void;
  onSave: () => void;
}) {
  const [secretDraft, setSecretDraft] = useState(value.secret || "");
  const detected = detectTotpInput(secretDraft);
  const nonDefaultAlgo =
    (value.algorithm && value.algorithm !== "SHA1") ||
    (value.digits && value.digits !== 6) ||
    (value.period && value.period !== 30);
  const [showAdvanced, setShowAdvanced] = useState(!!nonDefaultAlgo);

  function applySecretDraft(next: string) {
    setSecretDraft(next);
    const d = detectTotpInput(next);
    if (d.kind === "otpauth") {
      onChange({
        ...value,
        secret: d.secret,
        issuer: d.issuer || value.issuer,
        account: d.account || value.account,
        algorithm: d.algorithm || value.algorithm || "SHA1",
        digits: d.digits || value.digits || 6,
        period: d.period || value.period || 30,
        icon: value.icon,
      });
      if ((d.algorithm && d.algorithm !== "SHA1") || d.digits !== 6 || d.period !== 30) {
        setShowAdvanced(true);
      }
    } else if (d.kind === "base32") {
      onChange({ ...value, secret: d.secret });
    } else {
      onChange({ ...value, secret: next.trim() || undefined });
    }
  }

  async function pickIcon() {
    const path = await open({ filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "webp", "ico", "bmp"] }] });
    if (typeof path !== "string") return;
    const info = await api.iconUploadCustom(path);
    onChange({ ...value, icon: info.iconRef });
  }

  const detectKind = detected.kind === "otpauth" ? "good" : detected.kind === "base32" ? "info" : detected.kind === "unknown" ? "warn" : "";

  return (
    <div className="wizard-overlay">
      <div className="card totp-editor dialog-card">
        <div className="card-head"><div className="card-title">{value.id ? "编辑 TOTP" : "添加 TOTP"}</div></div>
        <div className="card-body stack">
          <div className="field">
            <FieldLabel
              name={value.id ? (value.hasSeed === false ? "重新填入密钥（必填）" : "更换密钥（可选）") : "密钥"}
              tip="otpauth 链接和 Base32 密钥只需填一种。粘贴后会自动识别：链接会顺带填好平台、账号和算法；密钥则只需再补平台和账号。"
            />
            <textarea
              className="input mono totp-secret-box"
              rows={3}
              autoFocus={!value.id}
              value={secretDraft}
              placeholder={value.id ? "留空则不改原密钥" : "在此粘贴 otpauth 链接或 Base32 密钥"}
              onChange={(e) => applySecretDraft(e.target.value)}
            />
            <div className={"hint" + (detectKind ? "" : "")}>{detected.message}</div>
            {detectKind && (
              <div className={"callout sm " + detectKind} style={{ marginTop: 6 }}>
                {detected.kind === "otpauth" && "二选一 · 已按链接识别，不必再单独填 Base32"}
                {detected.kind === "base32" && "二选一 · 已按密钥识别，不必再贴 otpauth 链接"}
                {detected.kind === "unknown" && "请改贴完整链接或密钥"}
              </div>
            )}
            <div className="field-example">
              示例密钥：<code>JBSW Y3DP EHPK 3PXP</code>
              <br />
              示例链接：<code>otpauth://totp/GitHub:you@mail.com?secret=JBSWY3DPEHPK3PXP</code>
            </div>
          </div>

          <div className="grid-sum">
            <div className="field">
              <FieldLabel name="平台" tip="这个验证码属于哪个网站或服务，会出现在卡片标题上。" />
              <input
                className="input"
                value={value.issuer || ""}
                placeholder="例如 GitHub、Google、Cloudflare"
                onChange={(e) => onChange({ ...value, issuer: e.target.value })}
              />
            </div>
            <div className="field">
              <FieldLabel name="账号" tip="用来区分同一平台的多个号，可以填邮箱、用户名或备注名。" />
              <input
                className="input"
                value={value.account || ""}
                placeholder="例如 you@mail.com"
                onChange={(e) => onChange({ ...value, account: e.target.value })}
              />
            </div>
          </div>

          <div className="field">
            <label className="field-label">图标</label>
            <div className="row">
              <IconMark icon={value.icon} builtins={builtins} label={value.issuer} size={36} />
              <select className="input" value={value.icon?.startsWith("builtin:") ? value.icon : ""} onChange={(e) => onChange({ ...value, icon: e.target.value || undefined })}>
                <option value="">按平台名自动匹配</option>
                {builtins.map((b) => (
                  <option key={b.id} value={`builtin:${b.id}`}>{b.name}</option>
                ))}
              </select>
              <button type="button" className="btn sm" onClick={pickIcon}>上传</button>
            </div>
            <div className="hint">可不选。保存时会按平台名自动猜一个；也可上传自己的图标。</div>
          </div>

          <div className="field">
            <FieldLabel name="分组" tip="点选已有分组，或直接输入新名称。留空表示不分组。" />
            <GroupPicker
              groups={groups}
              value={value.group}
              onChange={(group) => onChange({ ...value, group })}
            />
          </div>
          <div className="field">
            <FieldLabel name="登录网址" tip="可选。填了之后卡片上能一键打开该网站。" />
            <input className="input" placeholder="https://github.com/login" value={value.url || ""} onChange={(e) => onChange({ ...value, url: e.target.value })} />
          </div>
          <div className="field">
            <FieldLabel name="备注" tip="会显示在主界面卡片和列表上。为保持版面整洁，最多 32 个字。" />
            <input
              className="input"
              maxLength={NOTE_MAX}
              placeholder="例如 公司号、备用邮箱"
              value={value.note || ""}
              onChange={(e) => onChange({ ...value, note: e.target.value.slice(0, NOTE_MAX) })}
            />
            <div className="hint">{(value.note || "").length}/{NOTE_MAX}</div>
          </div>

          <button type="button" className="btn ghost sm" onClick={() => setShowAdvanced((v) => !v)}>
            {showAdvanced ? "收起高级选项" : "高级选项（一般不用改）"}
          </button>
          {showAdvanced && (
            <div className="totp-advanced">
              <div className="hint" style={{ marginBottom: 6 }}>绝大多数网站都是 SHA1、6 位、30 秒，链接里带了这些参数时会自动填好。</div>
              <div className="row">
                <div className="field" style={{ flex: 1 }}>
                  <label className="field-label">算法</label>
                  <select className="input" value={value.algorithm || "SHA1"} onChange={(e) => onChange({ ...value, algorithm: e.target.value })}>
                    <option>SHA1</option><option>SHA256</option><option>SHA512</option>
                  </select>
                </div>
                <div className="field" style={{ flex: 1 }}>
                  <label className="field-label">位数</label>
                  <select className="input" value={value.digits || 6} onChange={(e) => onChange({ ...value, digits: Number(e.target.value) })}>
                    <option value={6}>6 位</option><option value={8}>8 位</option>
                  </select>
                </div>
                <div className="field" style={{ flex: 1 }}>
                  <label className="field-label">周期（秒）</label>
                  <input className="input" type="number" min={1} value={value.period || 30} onChange={(e) => onChange({ ...value, period: Number(e.target.value) })} />
                </div>
              </div>
            </div>
          )}

        </div>
        <div className="card-foot">
          <span />
          <div className="row">
            <button type="button" className="btn ghost sm" onClick={onClose}>取消</button>
            <button type="button" className="btn primary sm" disabled={busy} onClick={onSave}>保存</button>
          </div>
        </div>
      </div>
    </div>
  );
}
