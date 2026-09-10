import { useEffect, useState, useMemo } from "react";
import { useNavigate } from "react-router-dom";
import {
  RefreshCw,
  Plus,
  AlertTriangle,
  Copy,
  Check,
  Activity,
  Edit3,
  Trash2,
  X,
  AlertCircle,
} from "lucide-react";
import {
  api,
  errMessage,
  type Identity,
  type KeyRecord,
  type AgentStatus,
  type ConfigView,
  type AuthResult,
  type UpdateIdentityArgs,
} from "../lib/ipc";
import { PageHead, Empty, Badge } from "../ui/common";
import { writeClipboard } from "../lib/clipboard";
import { useApp } from "../store";

// 根据身份名称生成高质感头像背景色
const AVATAR_COLORS = [
  "linear-gradient(135deg, #6366f1, #4f46e5)", // 蓝紫
  "linear-gradient(135deg, #10b981, #059669)", // 翡翠绿
  "linear-gradient(135deg, #f59e0b, #d97706)", // 琥珀橙
  "linear-gradient(135deg, #0ea5e9, #0284c7)", // 天蓝
  "linear-gradient(135deg, #ec4899, #db2777)", // 玫红
  "linear-gradient(135deg, #8b5cf6, #7c3aed)", // 紫色
  "linear-gradient(135deg, #14b8a6, #0d9488)", // 青绿
];

function getAvatarBg(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = (hash << 5) - hash + name.charCodeAt(i);
    hash |= 0;
  }
  const idx = Math.abs(hash) % AVATAR_COLORS.length;
  return AVATAR_COLORS[idx];
}

interface TestResultMap {
  [hostAlias: string]: {
    loading?: boolean;
    result?: AuthResult;
    testedAt?: string;
  };
}

export function Overview() {
  const nav = useNavigate();
  const { writesLocked } = useApp();
  const [identities, setIdentities] = useState<Identity[]>([]);
  const [keys, setKeys] = useState<KeyRecord[]>([]);
  const [agentStatus, setAgentStatus] = useState<AgentStatus | null>(null);
  const [configView, setConfigView] = useState<ConfigView | null>(null);
  const [testResults, setTestResults] = useState<TestResultMap>({});
  const [loadingAll, setLoadingAll] = useState(false);
  const [err, setErr] = useState("");
  const [copiedKeyId, setCopiedKeyId] = useState<string | null>(null);

  // 编辑身份弹窗与二次确认弹窗
  const [editingIdentity, setEditingIdentity] = useState<Identity | null>(null);
  const [confirmAliasChange, setConfirmAliasChange] = useState<{
    original: Identity;
    updated: UpdateIdentityArgs;
  } | null>(null);
  const [deletingIdentity, setDeletingIdentity] = useState<Identity | null>(null);

  // 加载全量数据
  const loadData = async () => {
    try {
      setErr("");
      const [ids, ks, ag, cfg] = await Promise.all([
        api.listIdentities(),
        api.listKeys(),
        api.agentEnsure().catch(() => api.agentStatus().catch(() => null)),
        api.readSshConfig().catch(() => null),
      ]);
      setIdentities(ids);
      setKeys(ks);
      setAgentStatus(ag);
      setConfigView(cfg);
    } catch (e) {
      setErr(errMessage(e));
    }
  };

  useEffect(() => {
    loadData();
  }, [writesLocked]);

  // 复制公钥
  const handleCopyPublic = async (keyId: string | null) => {
    if (!keyId) return;
    const k = keys.find((item) => item.id === keyId);
    if (!k || !k.publicOpenssh) return;
    await writeClipboard(k.publicOpenssh.trim());
    setCopiedKeyId(keyId);
    setTimeout(() => setCopiedKeyId(null), 1800);
  };

  // 单账号 SSH 连通性测试
  const handleTestConnection = async (hostAlias: string) => {
    setTestResults((prev) => ({
      ...prev,
      [hostAlias]: { ...prev[hostAlias], loading: true },
    }));
    try {
      const res = await api.testConnection(hostAlias);
      setTestResults((prev) => ({
        ...prev,
        [hostAlias]: {
          loading: false,
          result: res,
          testedAt: new Date().toLocaleTimeString(),
        },
      }));
    } catch (e) {
      setTestResults((prev) => ({
        ...prev,
        [hostAlias]: {
          loading: false,
          result: {
            ok: false,
            account: null,
            message: errMessage(e),
            errorCode: "ERR",
          },
          testedAt: new Date().toLocaleTimeString(),
        },
      }));
    }
  };

  // 一键体检全部身份
  const handleTestAll = async () => {
    if (loadingAll || identities.length === 0) return;
    setLoadingAll(true);
    setErr("");
    try {
      // 重新确保 agent 与获取最新配置
      await api.agentEnsure().catch(() => null);
      const [ag, cfg] = await Promise.all([
        api.agentStatus().catch(() => null),
        api.readSshConfig().catch(() => null),
      ]);
      setAgentStatus(ag);
      setConfigView(cfg);

      // 依次执行所有身份的连接测试
      for (const id of identities) {
        setTestResults((prev) => ({
          ...prev,
          [id.hostAlias]: { ...prev[id.hostAlias], loading: true },
        }));
        try {
          const res = await api.testConnection(id.hostAlias);
          setTestResults((prev) => ({
            ...prev,
            [id.hostAlias]: {
              loading: false,
              result: res,
              testedAt: new Date().toLocaleTimeString(),
            },
          }));
        } catch (e) {
          setTestResults((prev) => ({
            ...prev,
            [id.hostAlias]: {
              loading: false,
              result: {
                ok: false,
                account: null,
                message: errMessage(e),
                errorCode: "ERR",
              },
              testedAt: new Date().toLocaleTimeString(),
            },
          }));
        }
      }
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setLoadingAll(false);
    }
  };

  // 一键加载进 Agent
  const handleLoadToAgent = async (identityId: string) => {
    try {
      setErr("");
      await api.agentEnsure();
      await api.agentLoadIdentity(identityId);
      const ag = await api.agentStatus();
      setAgentStatus(ag);
    } catch (e) {
      setErr(errMessage(e));
    }
  };

  // 一键修复 SSH 配置（补齐 IdentitiesOnly 等）
  const handleFixConfig = async (id: Identity) => {
    try {
      const k = keys.find((key) => key.id === id.keyId);
      const identityFile = k?.deployedPath || k?.sourcePath || "";
      await api.applyConfig({
        alias: id.hostAlias,
        hostName: id.realHost,
        user: id.user || "git",
        identityFile,
        identitiesOnly: true,
      });
      const cfg = await api.readSshConfig();
      setConfigView(cfg);
    } catch (e) {
      setErr(errMessage(e));
    }
  };

  // 统计计算
  const stats = useMemo(() => {
    let okCount = 0;
    let issuesCount = 0;

    identities.forEach((id) => {
      let hasIssue = false;

      // 1. 密钥检查
      const k = keys.find((item) => item.id === id.keyId);
      if (!k) hasIssue = true;

      // 2. 配置检查
      const block = configView?.blocks.find((b) => b.patterns.includes(id.hostAlias));
      const hasIdentitiesOnly = block?.options.some(
        ([k, v]) => k.toLowerCase() === "identitiesonly" && v.toLowerCase() === "yes"
      );
      if (!block || !hasIdentitiesOnly) hasIssue = true;

      // 3. Agent 检查
      const inAgent =
        agentStatus?.running &&
        agentStatus?.keys.some(
          (ak) => ak.identityName === id.name || (k && ak.agent.fingerprint === k.fingerprint)
        );
      if (!inAgent) hasIssue = true;

      // 4. 连通性测试
      const test = testResults[id.hostAlias];
      if (test?.result?.ok) {
        okCount++;
      } else if (test?.result && !test.result.ok) {
        hasIssue = true;
      }

      if (hasIssue) issuesCount++;
    });

    return {
      totalIdentities: identities.length,
      totalKeys: keys.length,
      okCount,
      issuesCount,
    };
  }, [identities, keys, agentStatus, configView, testResults]);

  // 保存微调后的身份（先检查 alias 变更）
  const handleSaveIdentity = async (formData: UpdateIdentityArgs) => {
    if (!editingIdentity) return;
    const isAliasChanged = editingIdentity.hostAlias !== formData.hostAlias.trim();

    if (isAliasChanged) {
      // 别名修改属于关键项变更，先弹出二次确认弹窗！
      setConfirmAliasChange({
        original: editingIdentity,
        updated: formData,
      });
      return;
    }

    // 别名未变更，直接保存
    await executeUpdateIdentity(formData);
  };

  const executeUpdateIdentity = async (formData: UpdateIdentityArgs) => {
    try {
      await api.updateIdentity(formData);
      setEditingIdentity(null);
      setConfirmAliasChange(null);
      await loadData();
    } catch (e) {
      setErr(errMessage(e));
    }
  };

  // 删除身份
  const handleDeleteIdentity = async (identityId: string) => {
    try {
      await api.deleteIdentity(identityId);
      setDeletingIdentity(null);
      setEditingIdentity(null);
      await loadData();
    } catch (e) {
      setErr(errMessage(e));
    }
  };

  return (
    <div className="stack-lg">
      <PageHead
        title="身份总览"
        desc="所有 Git 身份的密钥、配置、Agent 与连通状态"
        actions={
          <>
            <button
              type="button"
              className="btn"
              disabled={loadingAll || identities.length === 0}
              onClick={handleTestAll}
              title="重新检测配置、Agent与所有身份的SSH连通性"
            >
              <RefreshCw size={13} className={loadingAll ? "animate-spin" : ""} />
              <span>{loadingAll ? "体检中…" : "一键体检"}</span>
            </button>
            <button
              type="button"
              className="btn primary"
              disabled={writesLocked}
              title={writesLocked ? "正在同步，暂不可新建" : undefined}
              onClick={() => nav("/identities/new")}
            >
              <Plus size={14} />
              <span>新建身份</span>
            </button>
          </>
        }
      />

      {err && (
        <div className="callout danger sm" style={{ marginBottom: 4 }}>
          <AlertCircle size={14} />
          <span>{err}</span>
        </div>
      )}

      {/* 顶部 4 个统计卡片 */}
      <div className="stat-card-row">
        <div className="stat-card">
          <div className="stat-card-title">
            <span style={{ color: "var(--accent)" }}>◆</span>
            <span>身份总数</span>
          </div>
          <div className="stat-card-body">
            <div className="stat-card-val">{stats.totalIdentities}</div>
            <div className="stat-card-sub">已登记身份</div>
          </div>
        </div>

        <div className="stat-card">
          <div className="stat-card-title">
            <span style={{ color: "var(--amber)" }}>🔒</span>
            <span>已入库密钥</span>
          </div>
          <div className="stat-card-body">
            <div className="stat-card-val">{stats.totalKeys}</div>
            <div className="stat-card-sub" style={{ color: "var(--green)" }}>
              全部加密
            </div>
          </div>
        </div>

        <div className="stat-card">
          <div className="stat-card-title">
            <span style={{ color: "var(--green)" }}>🟢</span>
            <span>连通正常</span>
          </div>
          <div className="stat-card-body">
            <div className="stat-card-val">
              <span style={{ color: "var(--green)" }}>{stats.okCount}</span>
              <span style={{ fontSize: 13, color: "var(--text-mute)", fontWeight: 500 }}>
                {" "}
                / {stats.totalIdentities}
              </span>
            </div>
            <div className="stat-card-sub">
              {stats.okCount === stats.totalIdentities && stats.totalIdentities > 0
                ? "全部畅通"
                : "已体检"}
            </div>
          </div>
        </div>

        <div className="stat-card">
          <div className="stat-card-title">
            <span style={{ color: "var(--amber)" }}>▲</span>
            <span>待处理问题</span>
          </div>
          <div className="stat-card-body">
            <div
              className="stat-card-val"
              style={{ color: stats.issuesCount > 0 ? "var(--amber)" : "var(--text)" }}
            >
              {stats.issuesCount}
            </div>
            <div className="stat-card-sub">
              {stats.issuesCount > 0 ? "需关注修复" : "状态良好"}
            </div>
          </div>
        </div>
      </div>

      {/* 身份卡片 Grid（3 列紧凑网格） */}
      {identities.length === 0 ? (
        <div className="card" style={{ padding: "30px 10px" }}>
          <Empty icon="🧑‍💻" text="还没有配置 Git 身份。点击右上角「新建身份」开始添加。" />
        </div>
      ) : (
        <div className="grid c3">
          {identities.map((id, index) => {
            const keyRecord = keys.find((k) => k.id === id.keyId);

            // 1. 密钥状态点
            const hasKey = !!keyRecord;
            const keyDot = hasKey ? "g" : "r";

            // 2. 配置状态点
            const block = configView?.blocks.find((b) => b.patterns.includes(id.hostAlias));
            const hasIdentitiesOnly = block?.options.some(
              ([k, v]) => k.toLowerCase() === "identitiesonly" && v.toLowerCase() === "yes"
            );
            const configDot = !block ? "r" : !hasIdentitiesOnly ? "y" : "g";

            // 3. Agent 状态点
            const inAgent =
              agentStatus?.running &&
              agentStatus?.keys.some(
                (ak) =>
                  ak.identityName === id.name ||
                  (keyRecord && ak.agent.fingerprint === keyRecord.fingerprint)
              );
            const agentDot = inAgent ? "g" : "gray";

            // 4. 连通状态点
            const test = testResults[id.hostAlias];
            const testDot = test?.loading
              ? "y"
              : test?.result?.ok
              ? "g"
              : test?.result
              ? "r"
              : "gray";

            // 智能诊断提示条
            let calloutContent: {
              type: "warn" | "danger" | "info" | "good";
              text: string;
              actionText?: string;
              onAction?: () => void;
            } | null = null;

            if (!hasIdentitiesOnly && block) {
              calloutContent = {
                type: "warn",
                text: "▲ 配置缺少 IdentitiesOnly yes，可能串号",
                actionText: "一键修复",
                onAction: () => handleFixConfig(id),
              };
            } else if (!inAgent) {
              calloutContent = {
                type: "danger",
                text: "✕ 密钥未加载进 Agent",
                actionText: "一键加载",
                onAction: () => handleLoadToAgent(id.id),
              };
            } else if (test?.result) {
              if (test.result.ok) {
                calloutContent = {
                  type: "info",
                  text: `● 上次体检: ${test.result.account ? `Hi ${test.result.account} ✓ 账号匹配` : "连通成功"}`,
                };
              } else {
                calloutContent = {
                  type: "danger",
                  text: `✕ 连通失败: ${test.result.message || "请检查网络或公钥部署"}`,
                };
              }
            }

            const initialLetter = (id.name || "G").charAt(0).toUpperCase();
            const isDefault = index === 0;

            return (
              <div className="identity-card" key={id.id}>
                {/* 头部：彩色头像 + 名字 + 别名映射 */}
                <div className="id-card-head">
                  <div
                    className="id-avatar"
                    style={{ background: getAvatarBg(id.name) }}
                  >
                    {initialLetter}
                  </div>
                  <div className="id-head-meta">
                    <div className="id-name-row">
                      <span className="id-name" title={id.name}>
                        {id.name}
                      </span>
                      {isDefault && <Badge kind="info">默认</Badge>}
                      {id.strictMode && <Badge kind="warn">严格</Badge>}
                    </div>
                    <div className="id-sub" title={`${id.hostAlias} → ${id.realHost}`}>
                      {id.hostAlias === id.realHost
                        ? id.realHost
                        : `${id.hostAlias} → ${id.realHost}`}
                    </div>
                  </div>
                </div>

                {/* 4 状态指示灯栏 */}
                <div className="status-bar-4">
                  <div className="status-col">
                    <span className={`status-dot ${keyDot}`} />
                    <span className="status-dot-label">密钥</span>
                  </div>
                  <div className="status-col">
                    <span className={`status-dot ${configDot}`} />
                    <span className="status-dot-label">配置</span>
                  </div>
                  <div className="status-col">
                    <span className={`status-dot ${agentDot}`} />
                    <span className="status-dot-label">Agent</span>
                  </div>
                  <div className="status-col">
                    <span className={`status-dot ${testDot}`} />
                    <span className="status-dot-label">连通</span>
                  </div>
                </div>

                {/* 提示条 Callout */}
                {calloutContent && (
                  <div className={`id-callout ${calloutContent.type}`}>
                    <span
                      style={{
                        overflow: "hidden",
                        textOverflow: calloutContent.text.includes("\n") ? undefined : "ellipsis",
                        whiteSpace: calloutContent.text.includes("\n") ? "pre-wrap" : "nowrap",
                        flex: 1,
                      }}
                      title={calloutContent.text}
                    >
                      {calloutContent.text}
                    </span>
                    {calloutContent.actionText && (
                      <button
                        type="button"
                        style={{
                          fontWeight: 600,
                          textDecoration: "underline",
                          cursor: "pointer",
                          flexShrink: 0,
                          fontSize: 10.5,
                        }}
                        onClick={calloutContent.onAction}
                      >
                        {calloutContent.actionText}
                      </button>
                    )}
                  </div>
                )}

                {/* Owners 标签群 */}
                {id.owners.length > 0 && (
                  <div className="owners-wrap">
                    {id.owners.map((owner) => (
                      <span className="owner-tag" key={owner}>
                        {owner}
                      </span>
                    ))}
                  </div>
                )}

                {/* 底部按钮栏 */}
                <div
                  className="row"
                  style={{
                    marginTop: "auto",
                    paddingTop: 6,
                    borderTop: "1px solid var(--border)",
                    justifyContent: "space-between",
                  }}
                >
                  <button
                    type="button"
                    className="btn ghost sm"
                    disabled={writesLocked}
                    onClick={() => setEditingIdentity(id)}
                    title={writesLocked ? "正在同步，暂不可修改" : "修改备注、别名、邮箱、提交姓名等"}
                  >
                    <Edit3 size={12} />
                    <span>详情</span>
                  </button>

                  <div className="row" style={{ gap: 4 }}>
                    <button
                      type="button"
                      className="btn sm"
                      disabled={!id.keyId}
                      onClick={() => handleCopyPublic(id.keyId)}
                      title="复制此身份绑定的 OpenSSH 公钥"
                    >
                      {copiedKeyId === id.keyId ? (
                        <>
                          <Check size={12} style={{ color: "var(--green)" }} />
                          <span style={{ color: "var(--green)" }}>已复制</span>
                        </>
                      ) : (
                        <>
                          <Copy size={12} />
                          <span>复制公钥</span>
                        </>
                      )}
                    </button>
                    <button
                      type="button"
                      className="btn sm"
                      disabled={test?.loading}
                      onClick={() => handleTestConnection(id.hostAlias)}
                      title="测试与此身份的 SSH 连通性"
                    >
                      <Activity size={12} />
                      <span>{test?.loading ? "测试中…" : "体检"}</span>
                    </button>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* 身份编辑与微调弹窗 */}
      {editingIdentity && (
        <EditIdentityModal
          identity={editingIdentity}
          onClose={() => setEditingIdentity(null)}
          onSave={handleSaveIdentity}
          onDelete={(id) => setDeletingIdentity(id)}
        />
      )}

      {/* 修改 Host 别名时的关键项二次确认弹窗 */}
      {confirmAliasChange && (
        <ConfirmAliasModal
          original={confirmAliasChange.original}
          updated={confirmAliasChange.updated}
          onCancel={() => setConfirmAliasChange(null)}
          onConfirm={() => executeUpdateIdentity(confirmAliasChange.updated)}
        />
      )}

      {/* 删除身份确认弹窗 */}
      {deletingIdentity && (
        <DeleteIdentityModal
          identity={deletingIdentity}
          onCancel={() => setDeletingIdentity(null)}
          onConfirm={() => handleDeleteIdentity(deletingIdentity.id)}
        />
      )}
    </div>
  );
}

// --------------------------------------------------------------------------
// 身份编辑弹窗组件
// --------------------------------------------------------------------------
function EditIdentityModal({
  identity,
  onClose,
  onSave,
  onDelete,
}: {
  identity: Identity;
  onClose: () => void;
  onSave: (form: UpdateIdentityArgs) => Promise<void>;
  onDelete: (identity: Identity) => void;
}) {
  const [name, setName] = useState(identity.name);
  const [platform, setPlatform] = useState(identity.platform);
  const [hostAlias, setHostAlias] = useState(identity.hostAlias);
  const [realHost, setRealHost] = useState(identity.realHost);
  const [user, setUser] = useState(identity.user || "git");
  const [email, setEmail] = useState(identity.email || "");
  const [gitUserName, setGitUserName] = useState(identity.gitUserName || "");
  const [strictMode, setStrictMode] = useState(identity.strictMode);
  const [ownersText, setOwnersText] = useState(identity.owners.join(", "));
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  const isAliasModified = hostAlias.trim() !== identity.hostAlias;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setErr("");
    if (!name.trim()) return setErr("请填写身份备注名");
    if (!hostAlias.trim()) return setErr("请填写 Host 别名");
    if (!realHost.trim()) return setErr("请填写真实主机地址");

    // 格式化 owners
    const owners = ownersText
      .split(/[,\s]+/)
      .map((s) => s.trim().toLowerCase())
      .filter(Boolean);

    setBusy(true);
    try {
      await onSave({
        id: identity.id,
        name: name.trim(),
        platform,
        hostAlias: hostAlias.trim(),
        realHost: realHost.trim(),
        user: user.trim() || "git",
        email: email.trim() || undefined,
        gitUserName: gitUserName.trim() || undefined,
        strictMode,
        owners,
      });
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="wizard-overlay">
      <div className="card" style={{ width: 490, maxWidth: "96%", maxHeight: "90vh", display: "flex", flexDirection: "column" }}>
        <div className="card-head">
          <div className="row" style={{ gap: 6 }}>
            <Edit3 size={15} style={{ color: "var(--accent)" }} />
            <div className="card-title">微调身份配置：{identity.name}</div>
          </div>
          <button type="button" className="btn ghost sm" onClick={onClose}>
            <X size={15} />
          </button>
        </div>

        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", flex: 1, minHeight: 0 }}>
          <div className="card-body" style={{ overflow: "auto", flex: 1, display: "flex", flexDirection: "column", gap: 10 }}>
            {err && <div className="callout danger sm">{err}</div>}

            <div className="grid c2">
              <div className="field">
                <label className="field-label">身份备注名 *</label>
                <input
                  className="input"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="如 nova-labs"
                  required
                />
              </div>

              <div className="field">
                <label className="field-label">Git 平台</label>
                <select
                  className="input"
                  value={platform}
                  onChange={(e) => setPlatform(e.target.value)}
                >
                  <option value="github">GitHub</option>
                  <option value="gitlab">GitLab</option>
                  <option value="gitee">Gitee</option>
                  <option value="codeup">阿里云 Codeup</option>
                  <option value="custom">自建 / 自定义</option>
                </select>
              </div>
            </div>

            <div className="grid c2">
              <div className="field">
                <label className="field-label">Host 别名 *</label>
                <input
                  className="input mono"
                  value={hostAlias}
                  onChange={(e) => setHostAlias(e.target.value)}
                  placeholder="如 github-nova-labs"
                  required
                />
                <div className="hint">用于 git clone git@{hostAlias || "alias"}:...</div>
              </div>

              <div className="field">
                <label className="field-label">真实服务器 Host *</label>
                <input
                  className="input mono"
                  value={realHost}
                  onChange={(e) => setRealHost(e.target.value)}
                  placeholder="如 github.com"
                  required
                />
              </div>
            </div>

            {isAliasModified && (
              <div className="callout warn sm">
                ⚠️ 修改 Host 别名会同步改写 ~/.ssh/config。如果已有本地仓库使用了原别名，需同步更新 remote 地址。
              </div>
            )}

            <div className="grid c2">
              <div className="field">
                <label className="field-label">SSH 登录用户名</label>
                <input
                  className="input mono"
                  value={user}
                  onChange={(e) => setUser(e.target.value)}
                  placeholder="默认 git"
                />
              </div>

              <div className="field">
                <label className="field-label">提交邮箱 (user.email)</label>
                <input
                  className="input"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  placeholder="用于 commit 记录"
                />
              </div>
            </div>

            <div className="field">
              <label className="field-label">提交人姓名 (user.name)</label>
              <input
                className="input"
                value={gitUserName}
                onChange={(e) => setGitUserName(e.target.value)}
                placeholder="可选，留空则保持仓库默认"
              />
            </div>

            <div className="field">
              <label className="field-label">归属标识 / Organization 组织前缀</label>
              <input
                className="input mono"
                value={ownersText}
                onChange={(e) => setOwnersText(e.target.value)}
                placeholder="以逗号分隔，如 nova-labs, polar-box, acme-*"
              />
              <div className="hint">当识别包含这些组织名的仓库地址时，将自动推荐并映射该身份。</div>
            </div>

            <div className="field" style={{ marginTop: 2 }}>
              <div className="between" style={{ padding: "6px 0" }}>
                <div>
                  <div style={{ fontWeight: 600, fontSize: 11.5 }}>严格私钥安全模式</div>
                  <div className="hint">严格模式下私钥永不落盘 ~/.ssh，仅存加密库；必须依赖 ssh-agent。</div>
                </div>
                <div
                  className={`switch ${strictMode ? "" : "off"}`}
                  onClick={() => setStrictMode(!strictMode)}
                />
              </div>
            </div>
          </div>

          <div
            className="card-head"
            style={{
              justifyContent: "space-between",
              borderRadius: "0 0 var(--radius-lg) var(--radius-lg)",
              borderTop: "1px solid var(--border)",
              borderBottom: "none",
            }}
          >
            <button
              type="button"
              className="btn danger sm"
              onClick={() => onDelete(identity)}
            >
              <Trash2 size={13} />
              <span>删除身份</span>
            </button>

            <div className="row" style={{ gap: 6 }}>
              <button type="button" className="btn ghost sm" onClick={onClose}>
                取消
              </button>
              <button type="submit" className="btn primary sm" disabled={busy}>
                {busy ? "保存中…" : "保存修改"}
              </button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}

// --------------------------------------------------------------------------
// 修改别名关键项时的二次确认弹窗
// --------------------------------------------------------------------------
function ConfirmAliasModal({
  original,
  updated,
  onCancel,
  onConfirm,
}: {
  original: Identity;
  updated: UpdateIdentityArgs;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="wizard-overlay" style={{ zIndex: 60 }}>
      <div className="card" style={{ width: 440, maxWidth: "95%", border: "1px solid var(--amber)" }}>
        <div className="card-head" style={{ background: "var(--amber-soft)" }}>
          <div className="row" style={{ gap: 6, color: "var(--amber)" }}>
            <AlertTriangle size={16} />
            <div className="card-title" style={{ color: "var(--amber)", fontWeight: 700 }}>
              关键项修改确认：Host 别名变更
            </div>
          </div>
        </div>

        <div className="card-body stack" style={{ gap: 10, padding: "14px 16px" }}>
          <div style={{ fontSize: 12, lineHeight: 1.5 }}>
            你正在修改身份「<strong>{original.name}</strong>」的核心 Host 别名：
          </div>

          <div
            style={{
              background: "var(--panel-2)",
              padding: "8px 10px",
              borderRadius: 6,
              border: "1px solid var(--border)",
              fontSize: 11.5,
              fontFamily: "var(--mono)",
            }}
          >
            <div style={{ color: "var(--red)" }}>- 旧别名: {original.hostAlias}</div>
            <div style={{ color: "var(--green)", marginTop: 2 }}>+ 新别名: {updated.hostAlias}</div>
          </div>

          <div className="callout warn sm">
            ⚠️ <strong>风险提醒：</strong>
            <br />
            如果已有本地 Git 仓库绑定了原别名（其 Remote 地址形如{" "}
            <code>git@{original.hostAlias}:owner/repo.git</code>），修改后这些仓库的拉取和推送将无法找到 Host！
            <br />
            你需要在「仓库与克隆」页面或通过 git remote 命令同步更新别名地址。
          </div>
        </div>

        <div className="card-head" style={{ justifyContent: "flex-end", gap: 8, borderTop: "1px solid var(--border)", borderBottom: "none" }}>
          <button type="button" className="btn ghost sm" onClick={onCancel}>
            返回修改
          </button>
          <button type="button" className="btn primary sm" onClick={onConfirm}>
            我已知晓风险，确认修改
          </button>
        </div>
      </div>
    </div>
  );
}

// --------------------------------------------------------------------------
// 删除身份确认弹窗
// --------------------------------------------------------------------------
function DeleteIdentityModal({
  identity,
  onCancel,
  onConfirm,
}: {
  identity: Identity;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="wizard-overlay" style={{ zIndex: 60 }}>
      <div className="card" style={{ width: 400, maxWidth: "95%", border: "1px solid var(--red)" }}>
        <div className="card-head" style={{ background: "var(--red-soft)" }}>
          <div className="row" style={{ gap: 6, color: "var(--red)" }}>
            <Trash2 size={16} />
            <div className="card-title" style={{ color: "var(--red)", fontWeight: 700 }}>
              删除身份确认
            </div>
          </div>
        </div>

        <div className="card-body stack" style={{ gap: 10, padding: "14px 16px" }}>
          <div>
            确定要删除身份「<strong>{identity.name}</strong>」（别名 <code>{identity.hostAlias}</code>）吗？
          </div>
          <div className="callout danger sm">
            此操作将从加密库移除该身份配置，并自动清理 ~/.ssh/config 中的对应 Host 托管段落。关联的密钥不会被删除。
          </div>
        </div>

        <div className="card-head" style={{ justifyContent: "flex-end", gap: 8, borderTop: "1px solid var(--border)", borderBottom: "none" }}>
          <button type="button" className="btn ghost sm" onClick={onCancel}>
            取消
          </button>
          <button type="button" className="btn danger sm" onClick={onConfirm}>
            确认删除
          </button>
        </div>
      </div>
    </div>
  );
}
