import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { ChevronDown, Copy, Eye, EyeOff, Plus, ChevronsDown, ChevronsUp, Clock, ExternalLink, ShieldCheck, Check } from "lucide-react";
import {
  api,
  errMessage,
  type AccountEntry,
  type BuiltinIconInfo,
  type GroupMeta,
  type HistoryMeta,
  type TotpEntry,
} from "../lib/ipc";
import { copyWithClear, isNeedReauth, tryBiometricReauth } from "../lib/secretsUi";
import { PageHead, Empty, Badge, FieldLabel } from "../ui/common";
import { detectAccountSource } from "../lib/accountInput";
import { ReauthDialog } from "../ui/ReauthDialog";
import { IconMark } from "../ui/IconMark";
import { CountdownRing } from "../ui/CountdownRing";
import { GroupDialog } from "../ui/GroupDialog";
import { GroupPicker, appendGroupIfNew, resolveGroupName } from "../ui/GroupPicker";
import { useApp } from "../store";

export function AccountsPage() {
  const { writesLocked } = useApp();
  const [entries, setEntries] = useState<AccountEntry[]>([]);
  const [groups, setGroups] = useState<GroupMeta[]>([]);
  const [totps, setTotps] = useState<TotpEntry[]>([]);
  const [builtins, setBuiltins] = useState<BuiltinIconInfo[]>([]);
  const [q, setQ] = useState("");
  const [group, setGroup] = useState("全部");
  const [err, setErr] = useState("");
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [reauth, setReauth] = useState<null | ((pw: string) => Promise<void>)>(null);
  const reauthCancel = useRef<(() => void) | null>(null);
  const [revealCfg, setRevealCfg] = useState({ grace: 5, clip: 20 });
  const [editor, setEditor] = useState<null | (Partial<AccountEntry> & { password?: string })>(null);
  const [historyFor, setHistoryFor] = useState<null | { id: string; items: HistoryMeta[]; shown?: Record<number, string> }>(null);
  const [pwShown, setPwShown] = useState<Record<string, string>>({});
  const [totpShown, setTotpShown] = useState<Record<string, { code: string; remain: number; period: number }>>({});
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [groupDlg, setGroupDlg] = useState(false);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    const [acc, totp, icons] = await Promise.all([api.accountList(), api.totpList(), api.iconListBuiltin()]);
    setEntries(acc.entries);
    setGroups(acc.groups);
    setTotps(totp.entries);
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
      setTotpShown((prev) => {
        let changed = false;
        const next = { ...prev };
        for (const id of Object.keys(next)) {
          const item = next[id];
          if (item.remain <= 1) {
            api.totpGenerateCode(id).then((c) => {
              setTotpShown((cur) => ({ ...cur, [id]: { code: c.code, remain: c.remainingSeconds, period: c.period } }));
            }).catch(() => {
              setTotpShown((cur) => {
                const { [id]: _, ...rest } = cur;
                return rest;
              });
            });
          } else {
            next[id] = { ...item, remain: item.remain - 1 };
            changed = true;
          }
        }
        return changed ? next : prev;
      });
    }, 1000);
    return () => window.clearInterval(t);
  }, []);

  function triggerCopied(key: string) {
    setCopiedKey(key);
    window.setTimeout(() => {
      setCopiedKey((prev) => (prev === key ? null : prev));
    }, 1500);
  }

  async function copyUsername(id: string, username: string) {
    await copyWithClear(username, undefined, false);
    triggerCopied(`user-${id}`);
  }

  async function withAuth<T>(fn: (pw?: string) => Promise<T>): Promise<T | undefined> {
    try {
      return await fn();
    } catch (e) {
      if (!isNeedReauth(e)) {
        setErr(errMessage(e));
        return;
      }
      if (await tryBiometricReauth()) {
        try {
          return await fn();
        } catch (err) {
          if (!isNeedReauth(err)) {
            setErr(errMessage(err));
            return;
          }
        }
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

  const filtered = useMemo(() => {
    const words = q.trim().toLowerCase().split(/\s+/).filter(Boolean);
    return entries.filter((e) => {
      if (group !== "全部" && (e.group || "未分组") !== group) return false;
      if (!words.length) return true;
      const blob = [e.platform, e.username, e.displayName, e.note, e.url, ...(e.tags || [])].filter(Boolean).join(" ").toLowerCase();
      return words.every((w) => blob.includes(w));
    });
  }, [entries, q, group]);

  const platforms = useMemo(() => {
    const map = new Map<string, AccountEntry[]>();
    for (const e of filtered) {
      const list = map.get(e.platform) || [];
      list.push(e);
      map.set(e.platform, list);
    }
    for (const list of map.values()) {
      list.sort((a, b) => {
        if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
        const ta = a.lastUsedAt || "";
        const tb = b.lastUsedAt || "";
        if (ta !== tb) return tb.localeCompare(ta);
        if (a.sortOrder !== b.sortOrder) return a.sortOrder - b.sortOrder;
        return a.username.localeCompare(b.username);
      });
    }
    return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [filtered]);

  async function revealPw(id: string) {
    const pw = await withAuth((p) => api.accountRevealPassword(id, p));
    if (pw) {
      setPwShown((m) => ({ ...m, [id]: pw }));
      await api.accountTouch(id);
    }
  }

  async function copyPw(id: string) {
    let pw = pwShown[id];
    if (!pw) {
      const got = await withAuth((p) => api.accountRevealPassword(id, p));
      if (!got) return;
      pw = got;
      setPwShown((m) => ({ ...m, [id]: pw }));
    }
    await copyWithClear(pw);
    await api.accountTouch(id);
    triggerCopied(`pw-${id}`);
  }

  async function revealLinkedTotp(totpId: string) {
    const c = await withAuth((p) => api.totpGenerateCode(totpId, p));
    if (c) setTotpShown((m) => ({ ...m, [totpId]: { code: c.code, remain: c.remainingSeconds, period: c.period } }));
  }

  async function copyLinkedTotp(totpId: string) {
    let item = totpShown[totpId];
    if (!item) {
      const c = await withAuth((p) => api.totpGenerateCode(totpId, p));
      if (!c) return;
      item = { code: c.code, remain: c.remainingSeconds, period: c.period };
      setTotpShown((m) => ({ ...m, [totpId]: item }));
    }
    await copyWithClear(item.code.replace(/\s/g, ""));
    triggerCopied(`totp-${totpId}`);
  }

  async function saveEditor() {
    if (!editor) return;
    setBusy(true);
    try {
      const group = resolveGroupName(groups, editor.group);
      const created = appendGroupIfNew(groups, group);
      if (created) {
        await api.accountSaveGroups(created);
        setGroups(created);
      }
      const platform = (editor.platform || "").trim();
      const username = (editor.username || "").trim();
      const dup = entries.some(
        (e) =>
          e.id !== editor.id &&
          e.platform.trim().toLowerCase() === platform.toLowerCase() &&
          e.username.trim().toLowerCase() === username.toLowerCase(),
      );
      if (dup && !window.confirm(`已存在 ${platform} / ${username}，仍要保存吗？`)) {
        return;
      }
      if (editor.id && editor.hasPassword === false && !editor.password?.trim()) {
        setErr("这条账号的密码已丢失，请重新填入密码。");
        return;
      }
      const args = {
        id: editor.id,
        platform,
        username,
        password: editor.password,
        displayName: editor.displayName || undefined,
        url: editor.url || undefined,
        note: editor.note || undefined,
        group,
        tags: editor.tags,
        icon: editor.icon || undefined,
        pinned: editor.pinned,
        totpRef: editor.totpRef || undefined,
      };
      if (editor.id) await api.accountUpdate(args);
      else await api.accountAdd(args);
      setEditor(null);
      await load();
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function saveGroup(name: string, color: string | null) {
    const next = [...groups, { name, color, sortOrder: groups.length }];
    await api.accountSaveGroups(next);
    setGroups(next);
    setGroup(name);
    setGroupDlg(false);
  }

  const tabs = ["全部", ...groups.map((g) => g.name), "未分组"];

  return (
    <div className="stack-lg">
      <PageHead
        title="隐私账号"
        desc="同平台多账号聚合。点击平台栏折叠；密码与关联 2FA 默认掩码。"
        actions={
          <>
            <button type="button" className="btn sm" onClick={() => setCollapsed({})}>
              <ChevronsDown size={13} /> 全部展开
            </button>
            <button
              type="button"
              className="btn sm"
              onClick={() => setCollapsed(Object.fromEntries(platforms.map(([p]) => [p, true])))}
            >
              <ChevronsUp size={13} /> 全部折叠
            </button>
            <button type="button" className="btn primary sm" disabled={writesLocked} onClick={() => setEditor({})}>
              <Plus size={13} /> 添加账号
            </button>
          </>
        }
      />

      <div className="security-banner">
        <div className="security-banner-text">
          <ShieldCheck size={16} style={{ color: "var(--accent)", flexShrink: 0 }} />
          <span>
            <strong>默认掩码安全模式：</strong>
            密码与关联 2FA 默认隐藏防窥；复制后写入剪贴板
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
          {tabs.map((g) => {
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
        <input className="input" style={{ maxWidth: 280 }} placeholder="检索平台 / 账号 / 标签 (多词 AND)" value={q} onChange={(e) => setQ(e.target.value)} />
      </div>
      {err && <div className="callout danger sm">{err}</div>}

      {platforms.length === 0 ? (
        <Empty text="还没有隐私账号。" />
      ) : (
        platforms.map(([platform, list]) => {
          const folded = !!collapsed[platform];
          return (
            <div key={platform} className={"platform-group" + (folded ? " collapsed" : "")}>
              <button
                type="button"
                className="platform-header"
                onClick={() => setCollapsed((m) => ({ ...m, [platform]: !m[platform] }))}
              >
                <div className="platform-title">
                  <ChevronDown size={15} className={"chevron" + (folded ? " rot" : "")} />
                  <IconMark icon={list[0]?.icon} builtins={builtins} label={platform} size={28} />
                  <span>{platform}</span>
                  {list[0]?.url && (
                    <span
                      role="button"
                      tabIndex={0}
                      className="btn ghost sm"
                      style={{ padding: "2px 5px", display: "inline-flex", alignItems: "center" }}
                      title="打开官方网站"
                      onClick={(ev) => {
                        ev.stopPropagation();
                        api.openUrl(list[0].url!);
                      }}
                    >
                      <ExternalLink size={12} />
                    </span>
                  )}
                  <Badge kind="info">{list.length} 个账号</Badge>
                </div>
                <span
                  className="btn sm"
                  onClick={(ev) => {
                    ev.stopPropagation();
                    setEditor({ platform, url: list[0]?.url });
                  }}
                >
                  <Plus size={12} /> 添加
                </span>
              </button>
              {!folded && (
                <div className="platform-body">
                  {list.map((e) => {
                    const linked = e.totpRef ? totpShown[e.totpRef] : undefined;
                    const isCopiedUser = copiedKey === `user-${e.id}`;
                    const isCopiedPw = copiedKey === `pw-${e.id}`;
                    const isCopiedTotp = e.totpRef ? copiedKey === `totp-${e.totpRef}` : false;
                    const pwMissing = e.hasPassword === false;

                    return (
                      <div key={e.id} className="account-item">
                        <div className="account-user-info">
                          <div className="account-user">
                            <span>{e.username}</span>
                            {e.pinned && <Badge kind="warn">置顶</Badge>}
                            {e.tags?.map((t) => (
                              <Badge key={t}>{t}</Badge>
                            ))}
                          </div>
                          <div className="account-meta">
                            {e.displayName && <span>{e.displayName}</span>}
                            {e.displayName && e.note && <span>·</span>}
                            {e.note && <span className="muted">{e.note}</span>}
                            {(e.displayName || e.note) && e.lastUsedAt && <span>·</span>}
                            {e.lastUsedAt && (
                              <span className="muted" title="最近使用时间">
                                使用: {e.lastUsedAt.slice(0, 16)}
                              </span>
                            )}
                          </div>
                        </div>

                        <div className="account-mid">
                          {e.totpRef && (
                            <div className="linked-totp" title="关联 2FA 动态令牌">
                              <span className="linked-totp-badge">2FA</span>
                              {linked ? (
                                <div
                                  className="row"
                                  style={{ gap: 6, cursor: "pointer" }}
                                  onClick={() => copyLinkedTotp(e.totpRef!)}
                                  title="点击快速复制 2FA 验证码"
                                >
                                  <span className="mono" style={{ fontWeight: 700, color: "var(--accent)" }}>
                                    {linked.code}
                                  </span>
                                  <CountdownRing remain={linked.remain} period={linked.period} size={22} />
                                </div>
                              ) : (
                                <>
                                  <span className="muted">••••••</span>
                                  <button
                                    type="button"
                                    className="btn ghost sm"
                                    style={{ padding: "1px 4px" }}
                                    title="显示 2FA 验证码"
                                    onClick={() => revealLinkedTotp(e.totpRef!)}
                                  >
                                    <Eye size={12} />
                                  </button>
                                </>
                              )}
                              {linked && (
                                <button
                                  type="button"
                                  className={"btn ghost sm " + (isCopiedTotp ? "good" : "")}
                                  style={{ padding: "1px 4px" }}
                                  title="复制验证码"
                                  onClick={() => copyLinkedTotp(e.totpRef!)}
                                >
                                  {isCopiedTotp ? <Check size={11} /> : <Copy size={11} />}
                                </button>
                              )}
                            </div>
                          )}

                          {pwMissing && (
                            <div className="callout danger sm">密码已丢失，请编辑并重新填入。</div>
                          )}
                          <div className="pwd-box">
                            <span className="mono">{pwShown[e.id] || "••••••••"}</span>
                            {pwShown[e.id] ? (
                              <button
                                type="button"
                                className="btn ghost sm"
                                style={{ padding: "1px 4px" }}
                                title="隐藏密码"
                                onClick={() =>
                                  setPwShown((m) => {
                                    const n = { ...m };
                                    delete n[e.id];
                                    return n;
                                  })
                                }
                              >
                                <EyeOff size={12} />
                              </button>
                            ) : (
                              <button
                                type="button"
                                className="btn ghost sm"
                                style={{ padding: "1px 4px" }}
                                title={pwMissing ? "密码已丢失" : "显示明文"}
                                disabled={pwMissing}
                                onClick={() => revealPw(e.id)}
                              >
                                <Eye size={12} />
                              </button>
                            )}
                            <button
                              type="button"
                              className={"btn sm " + (isCopiedPw ? "good" : "primary")}
                              style={{ padding: "2px 8px" }}
                              title={pwMissing ? "密码已丢失" : "复制密码"}
                              disabled={pwMissing}
                              onClick={() => copyPw(e.id)}
                            >
                              {isCopiedPw ? <Check size={12} /> : <Copy size={12} />}
                            </button>
                          </div>
                        </div>

                        <div className="row" style={{ gap: 5 }}>
                          <button
                            type="button"
                            className={"btn sm " + (isCopiedUser ? "good" : "")}
                            title="复制用户名"
                            onClick={() => copyUsername(e.id, e.username)}
                          >
                            {isCopiedUser ? <Check size={12} /> : <Copy size={12} />} 账号
                          </button>
                          <button
                            type="button"
                            className="btn ghost sm"
                            title="查看密码历史版本"
                            onClick={async () => {
                              const items = await api.accountHistoryList(e.id);
                              setHistoryFor({ id: e.id, items });
                            }}
                          >
                            <Clock size={13} />
                          </button>
                          <button
                            type="button"
                            className="btn sm"
                            disabled={writesLocked}
                            title="编辑账号"
                            onClick={() => setEditor({ ...e })}
                          >
                            编辑
                          </button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          );
        })
      )}

      {reauth && <ReauthDialog onCancel={() => reauthCancel.current?.()} onConfirm={(pw) => reauth(pw)} />}

      {groupDlg && (
        <GroupDialog
          existing={groups.map((g) => g.name)}
          onCancel={() => setGroupDlg(false)}
          onConfirm={saveGroup}
        />
      )}

      {editor && (
        <AccountEditor
          value={editor}
          builtins={builtins}
          groups={groups}
          platforms={entries.map((e) => e.platform)}
          totps={totps}
          busy={busy}
          onChange={setEditor}
          onClose={() => setEditor(null)}
          onSave={saveEditor}
          onDelete={
            editor.id
              ? async () => {
                  if (!window.confirm("删除此账号？")) return;
                  await api.accountDelete(editor.id!);
                  setEditor(null);
                  await load();
                }
              : undefined
          }
        />
      )}

      {historyFor && (
        <div className="wizard-overlay">
          <div className="card" style={{ width: 460, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()}>
            <div className="card-head">
              <div className="card-title">密码历史版本记录</div>
              <button type="button" className="btn ghost sm" onClick={() => setHistoryFor(null)}>
                关闭
              </button>
            </div>
            <div className="card-body stack">
              {historyFor.items.length === 0 ? (
                <div className="muted" style={{ padding: "16px 0", textAlign: "center" }}>
                  暂无历史版本记录。
                </div>
              ) : (
                <div className="timeline">
                  {historyFor.items.map((h, idx) => {
                    const isLatest = idx === 0;
                    const shownPw = historyFor.shown?.[h.index];
                    return (
                      <div key={h.index} className="timeline-item">
                        <div className={"timeline-dot" + (isLatest ? " current" : "")} />
                        <div className="timeline-content">
                          <div style={{ minWidth: 0, flex: 1 }}>
                            <div className="row" style={{ gap: 6, marginBottom: 4 }}>
                              <span className="mono" style={{ fontWeight: 600, fontSize: "13px" }}>
                                {shownPw || "••••••••"}
                              </span>
                              {isLatest && <Badge kind="good">最新历史</Badge>}
                            </div>
                            <div className="muted" style={{ fontSize: "11px" }}>
                              替换时间: {h.replacedAt}
                            </div>
                          </div>
                          <div className="row" style={{ gap: 5 }}>
                            {!shownPw ? (
                              <button
                                type="button"
                                className="btn sm"
                                onClick={async () => {
                                  const pw = await withAuth((p) =>
                                    api.accountRevealHistory(historyFor.id, h.index, p)
                                  );
                                  if (pw)
                                    setHistoryFor({
                                      ...historyFor,
                                      shown: { ...historyFor.shown, [h.index]: pw },
                                    });
                                }}
                              >
                                查看
                              </button>
                            ) : (
                              <button
                                type="button"
                                className="btn sm"
                                title="复制此密码"
                                onClick={() => copyWithClear(shownPw)}
                              >
                                <Copy size={12} /> 复制
                              </button>
                            )}
                            <button
                              type="button"
                              className="btn sm"
                              disabled={writesLocked}
                              title="将密码回滚到此版本"
                              onClick={async () => {
                                if (!window.confirm(`确认将密码回滚到此历史版本（${h.replacedAt}）？`))
                                  return;
                                await api.accountRollbackHistory(historyFor.id, h.index);
                                setHistoryFor(null);
                                await load();
                              }}
                            >
                              回滚
                            </button>
                          </div>
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
              {historyFor.items.length > 0 && (
                <div className="row" style={{ justifyContent: "flex-end", marginTop: 8 }}>
                  <button
                    type="button"
                    className="btn danger sm"
                    disabled={writesLocked}
                    onClick={async () => {
                      if (!window.confirm("确定要清空该账号的所有历史版本记录吗？此操作不可逆。"))
                        return;
                      await api.accountClearHistory(historyFor.id);
                      setHistoryFor({ id: historyFor.id, items: [] });
                    }}
                  >
                    清空历史
                  </button>
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function AccountEditor({
  value,
  builtins,
  groups,
  platforms,
  totps,
  busy,
  onChange,
  onClose,
  onSave,
  onDelete,
}: {
  value: Partial<AccountEntry> & { password?: string };
  builtins: BuiltinIconInfo[];
  groups: GroupMeta[];
  platforms: string[];
  totps: TotpEntry[];
  busy: boolean;
  onChange: (v: Partial<AccountEntry> & { password?: string }) => void;
  onClose: () => void;
  onSave: () => void;
  onDelete?: () => void;
}) {
  const [showPw, setShowPw] = useState(false);
  const hasExtra = !!(value.displayName || value.url || value.note || value.group || value.totpRef || value.pinned || (value.tags && value.tags.length));
  const [showMore, setShowMore] = useState(hasExtra);
  const [platformMsg, setPlatformMsg] = useState("");
  const uniquePlatforms = [...new Set(platforms.filter(Boolean))].sort();

  function applyPlatform(raw: string) {
    const d = detectAccountSource(raw, builtins);
    setPlatformMsg(d.message);
    onChange({
      ...value,
      platform: d.platform || raw,
      url: d.url || value.url,
      icon: value.icon?.startsWith("custom:") ? value.icon : d.icon || value.icon,
    });
    if (d.kind === "url") setShowMore(true);
  }

  function applyUrl(raw: string) {
    const next: Partial<AccountEntry> & { password?: string } = { ...value, url: raw };
    if (!value.platform?.trim() && raw.trim()) {
      const d = detectAccountSource(raw, builtins);
      if (d.kind === "url") {
        next.platform = d.platform;
        next.url = d.url || raw;
        if (!value.icon?.startsWith("custom:")) next.icon = d.icon;
        setPlatformMsg(d.message);
      }
    }
    onChange(next);
  }

  async function pickIcon() {
    const path = await open({ filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "webp", "ico", "bmp"] }] });
    if (typeof path !== "string") return;
    const info = await api.iconUploadCustom(path);
    onChange({ ...value, icon: info.iconRef });
  }

  return (
    <div className="wizard-overlay">
      <div className="card dialog-card">
        <div className="card-head"><div className="card-title">{value.id ? "编辑账号" : "添加账号"}</div></div>
        <div className="card-body stack">
          <div className="grid-sum">
            <div className="field">
              <FieldLabel
                name="平台"
                tip="这个账号属于哪个网站。也可以直接粘贴登录页网址，会自动拆出平台名和链接。"
              />
              <input
                className="input"
                list="account-platforms"
                autoFocus={!value.platform}
                placeholder="例如 GitHub，或 https://github.com/login"
                value={value.platform || ""}
                onChange={(e) => applyPlatform(e.target.value)}
              />
              <datalist id="account-platforms">
                {uniquePlatforms.map((p) => (
                  <option key={p} value={p} />
                ))}
              </datalist>
              {platformMsg && <div className="hint">{platformMsg}</div>}
            </div>
            <div className="field">
              <FieldLabel name="用户名" tip="登录用的邮箱、手机号或用户名。列表里可直接复制这一项。" />
              <input
                className="input"
                placeholder="例如 you@mail.com"
                value={value.username || ""}
                onChange={(e) => onChange({ ...value, username: e.target.value })}
              />
            </div>
          </div>

          <div className="field">
            <FieldLabel
              name={value.id ? (value.hasPassword === false ? "重新填入密码（必填）" : "密码（留空则不改）") : "密码"}
              tip="改密会自动留下旧密码，可在卡片上的时钟入口查看或回滚。"
            />
            <div className="row">
              <input
                className="input"
                type={showPw ? "text" : "password"}
                autoComplete="new-password"
                placeholder={value.id ? (value.hasPassword === false ? "请重新填入密码" : "不改请留空") : "登录密码"}
                value={value.password || ""}
                onChange={(e) => onChange({ ...value, password: e.target.value })}
              />
              <button type="button" className="btn sm" onClick={() => setShowPw((v) => !v)}>
                {showPw ? "隐藏" : "显示"}
              </button>
            </div>
          </div>

          <div className="field">
            <label className="field-label">图标</label>
            <div className="row">
              <IconMark icon={value.icon} builtins={builtins} label={value.platform} size={36} />
              <select
                className="input"
                value={value.icon?.startsWith("builtin:") ? value.icon : ""}
                onChange={(e) => onChange({ ...value, icon: e.target.value || undefined })}
              >
                <option value="">按平台名自动匹配</option>
                {builtins.map((b) => (
                  <option key={b.id} value={`builtin:${b.id}`}>{b.name}</option>
                ))}
              </select>
              <button type="button" className="btn sm" onClick={pickIcon}>上传</button>
            </div>
            <div className="hint">可不选。填 GitHub、Google 等常见平台会自动套对应图标。</div>
          </div>

          <button type="button" className="btn ghost sm" onClick={() => setShowMore((v) => !v)}>
            {showMore ? "收起更多选项" : "更多选项（显示名、分组、网址、2FA…）"}
          </button>

          {showMore && (
            <div className="totp-advanced stack">
              <div className="field">
                <FieldLabel name="显示名" tip="同一平台有多个号时用来区分，例如「公司号」「个人号」。不填则列表只显示用户名。" />
                <input
                  className="input"
                  placeholder="例如 公司号"
                  value={value.displayName || ""}
                  onChange={(e) => onChange({ ...value, displayName: e.target.value })}
                />
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
                <FieldLabel name="登录网址" tip="可选。填了之后能从卡片打开该网站。若还没填平台，粘贴网址也会自动识别。" />
                <input
                  className="input"
                  placeholder="https://github.com/login"
                  value={value.url || ""}
                  onChange={(e) => applyUrl(e.target.value)}
                />
              </div>
              <div className="field">
                <FieldLabel name="标签" tip="可选，多个标签用空格分开，方便搜索。" />
                <input
                  className="input"
                  placeholder="例如 work personal"
                  value={(value.tags || []).join(" ")}
                  onChange={(e) => onChange({ ...value, tags: e.target.value.split(/\s+/).filter(Boolean) })}
                />
              </div>
              <div className="field">
                <label className="field-label">备注</label>
                <input
                  className="input"
                  placeholder="可选，只有你自己能看到"
                  value={value.note || ""}
                  onChange={(e) => onChange({ ...value, note: e.target.value })}
                />
              </div>
              <div className="field">
                <FieldLabel
                  name="联动 2FA"
                  tip="如果这个账号的验证码已经存在「2FA / TOTP」里，选中后卡片上就能一起看。"
                />
                <select
                  className="input"
                  value={value.totpRef || ""}
                  onChange={(e) => onChange({ ...value, totpRef: e.target.value || undefined })}
                >
                  <option value="">不联动</option>
                  {totps.map((t) => (
                    <option key={t.id} value={t.id}>{t.issuer} / {t.account}</option>
                  ))}
                </select>
                {totps.length === 0 && <div className="hint">还没有 TOTP 条目。可先到「2FA / TOTP」添加，再回来关联。</div>}
              </div>
              <label className="row" style={{ gap: 8 }}>
                <input type="checkbox" checked={!!value.pinned} onChange={(e) => onChange({ ...value, pinned: e.target.checked })} />
                <span>置顶到该平台最前面</span>
              </label>
            </div>
          )}

        </div>
        <div className="card-foot">
          {onDelete ? (
            <button type="button" className="btn danger sm" onClick={onDelete}>删除</button>
          ) : (
            <span />
          )}
          <div className="row">
            <button type="button" className="btn ghost sm" onClick={onClose}>取消</button>
            <button type="button" className="btn primary sm" disabled={busy} onClick={onSave}>保存</button>
          </div>
        </div>
      </div>
    </div>
  );
}
