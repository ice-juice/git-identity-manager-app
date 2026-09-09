import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Pencil, Trash2, X, Plus } from "lucide-react";
import { api, errMessage, type Identity, type ManagedRepoView } from "../lib/ipc";
import { PageHead, Card, Empty, Badge } from "../ui/common";
import { useApp } from "../store";

const SOURCE_LABEL: Record<string, string> = {
  scan: "扫描",
  clone: "克隆",
  init: "初始化",
  addRemote: "补远程",
  manual: "手动",
};

export function Repos() {
  const nav = useNavigate();
  const { writesLocked } = useApp();
  const [repos, setRepos] = useState<ManagedRepoView[]>([]);
  const [identities, setIdentities] = useState<Identity[]>([]);
  const [root, setRoot] = useState("");
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);
  const [removingBusy, setRemovingBusy] = useState(false);
  const [editing, setEditing] = useState<ManagedRepoView | null>(null);
  const [removing, setRemoving] = useState<ManagedRepoView | null>(null);

  async function load() {
    try {
      const [list, ids] = await Promise.all([api.listManagedRepos(), api.listIdentities()]);
      setRepos(list);
      setIdentities(ids);
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  useEffect(() => {
    load();
  }, []);

  async function pickRoot() {
    setErr("");
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "选择要扫描的根目录",
        defaultPath: root.trim() || undefined,
      });
      if (typeof selected !== "string" || !selected) return;
      setRoot(selected);
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  async function scanImport() {
    setErr("");
    setMsg("");
    if (!root.trim()) return setErr("请选择或填写扫描根目录");
    setBusy(true);
    try {
      const result = await api.scanAndImportRepos(root, 4);
      setRepos(result.repos);
      setMsg(
        `扫描完成：新增 ${result.imported} 个，更新 ${result.updated} 个；跳过无 remote ${result.skippedNoRemote} 个。`,
      );
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function openDir(path: string) {
    setErr("");
    try {
      await api.openRepoDir(path);
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  async function confirmRemove() {
    if (!removing || removingBusy) return;
    const target = removing;
    setErr("");
    setRemovingBusy(true);
    setRepos((prev) => prev.filter((r) => r.id !== target.id));
    setRemoving(null);
    try {
      await api.removeManagedRepo(target.id);
    } catch (e) {
      setErr(errMessage(e));
      await load();
    } finally {
      setRemovingBusy(false);
    }
  }

  return (
    <div className="stack-lg">
      <PageHead
        title="仓库管理"
        desc="登记带 remote 的本地仓库，统一改地址、切换身份、打开目录"
        actions={
          <button type="button" className="btn primary" disabled={writesLocked} onClick={() => nav("/clone")}>
            <Plus size={13} />
            <span>去克隆</span>
          </button>
        }
      />
      {err && <div className="err-text">{err}</div>}
      {msg && <div className="callout good">{msg}</div>}

      <Card title="扫描并入库">
        <div className="stack">
          <div className="path-pick">
            <input
              className="input mono"
              placeholder="扫描根目录，例如 D:/work"
              value={root}
              onChange={(e) => setRoot(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && scanImport()}
            />
            <button type="button" className="btn" onClick={pickRoot}>
              浏览…
            </button>
            <button type="button" className="btn primary" disabled={busy || writesLocked} onClick={scanImport}>
              {busy ? "扫描中…" : "扫描入库"}
            </button>
          </div>
          <div className="muted sm">只把已配置 origin / remote 的仓库加入管理列表；无 remote 的本地目录会被跳过。</div>
        </div>
      </Card>

      <Card title={`已登记仓库（${repos.length}）`}>
        {repos.length === 0 ? (
          <Empty icon="📦" text="还没有登记仓库。扫描带 remote 的目录，或先到「克隆」页落地一个仓库。" />
        ) : (
          <div className="list">
            {repos.map((r) => (
              <div className="list-row" key={r.id} style={{ alignItems: "flex-start" }}>
                <div className="grow">
                  <div className="row" style={{ gap: 6 }}>
                    <strong>{r.name}</strong>
                    <Badge>{SOURCE_LABEL[r.source] ?? r.source}</Badge>
                    {!r.exists && <Badge kind="danger">目录不存在</Badge>}
                    {r.needsAliasFix && <Badge kind="warn">建议改别名</Badge>}
                    {r.identityName && <Badge kind="info">{r.identityName}</Badge>}
                  </div>
                  <div className="mono muted sm">{r.path}</div>
                  <div className="mono sm" style={{ marginTop: 2 }}>
                    {r.remoteUrl ?? "无 remote"}
                  </div>
                </div>
                <div className="row" style={{ gap: 4 }}>
                  <button
                    type="button"
                    className="btn sm"
                    disabled={!r.exists}
                    title="在资源管理器中打开"
                    onClick={() => openDir(r.path)}
                  >
                    <FolderOpen size={12} />
                    <span>打开</span>
                  </button>
                  <button
                    type="button"
                    className="btn sm"
                    disabled={!r.exists || writesLocked}
                    onClick={() => setEditing(r)}
                    title={writesLocked ? "正在同步，暂不可修改" : "更换 remote URL 或绑定身份"}
                  >
                    <Pencil size={12} />
                    <span>改地址</span>
                  </button>
                  <button type="button" className="btn ghost sm" disabled={writesLocked} onClick={() => setRemoving(r)} title="仅从列表移除">
                    <Trash2 size={12} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </Card>

      {editing && (
        <RemoteEditModal
          repo={editing}
          identities={identities}
          onClose={() => setEditing(null)}
          onSaved={async () => {
            setEditing(null);
            setMsg("已更新 remote");
            await load();
          }}
        />
      )}

      {removing && (
        <div className="wizard-overlay" style={{ zIndex: 60 }}>
          <div className="card" style={{ width: 400, maxWidth: "95%" }}>
            <div className="card-head">
              <div className="card-title">从列表移除</div>
              <button type="button" className="btn ghost sm" onClick={() => setRemoving(null)}>
                <X size={15} />
              </button>
            </div>
            <div className="card-body stack" style={{ gap: 8 }}>
              <div>
                确定把「<strong>{removing.name}</strong>」移出管理列表？
              </div>
              <div className="callout info sm">只取消登记，不会删除磁盘上的仓库目录。</div>
              <div className="mono muted sm">{removing.path}</div>
            </div>
            <div
              className="card-head"
              style={{ justifyContent: "flex-end", gap: 8, borderTop: "1px solid var(--border)", borderBottom: "none" }}
            >
              <button type="button" className="btn ghost sm" onClick={() => setRemoving(null)}>
                取消
              </button>
              <button type="button" className="btn danger sm" disabled={removingBusy} onClick={confirmRemove}>
                {removingBusy ? "移除中…" : "移除"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function RemoteEditModal({
  repo,
  identities,
  onClose,
  onSaved,
}: {
  repo: ManagedRepoView;
  identities: Identity[];
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { writesLocked } = useApp();
  const [remoteUrl, setRemoteUrl] = useState(repo.remoteUrl ?? "");
  const [identityId, setIdentityId] = useState(repo.identityId ?? "");
  const [rewrite, setRewrite] = useState(!!repo.identityId);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  async function save() {
    setErr("");
    if (!remoteUrl.trim()) return setErr("请填写 remote URL");
    setBusy(true);
    try {
      await api.setRepoRemote({
        repoId: repo.id,
        remoteUrl: remoteUrl.trim(),
        identityId: rewrite && identityId ? identityId : undefined,
      });
      await onSaved();
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="wizard-overlay" style={{ zIndex: 60 }}>
      <div className="card" style={{ width: 480, maxWidth: "95%" }}>
        <div className="card-head">
          <div className="card-title">更换 remote：{repo.name}</div>
          <button type="button" className="btn ghost sm" onClick={onClose}>
            <X size={15} />
          </button>
        </div>
        <div className="card-body stack" style={{ gap: 10 }}>
          {err && <div className="callout danger sm">{err}</div>}
          <div className="field">
            <label className="field-label">origin URL</label>
            <input
              className="input mono"
              value={remoteUrl}
              onChange={(e) => setRemoteUrl(e.target.value)}
              placeholder="git@github-alias:owner/repo.git"
            />
          </div>
          <div className="field">
            <label className="field-label">绑定身份（可选）</label>
            <select className="input" value={identityId} onChange={(e) => setIdentityId(e.target.value)}>
              <option value="">不切换身份，按填写的 URL 写入</option>
              {identities.map((id) => (
                <option key={id.id} value={id.id}>
                  {id.name}（{id.hostAlias}）
                </option>
              ))}
            </select>
          </div>
          <div className="between" style={{ padding: "4px 0" }}>
            <div>
              <div style={{ fontWeight: 600, fontSize: 11.5 }}>同时改写为身份别名地址</div>
              <div className="hint">开启后按所选身份把 URL 改写成 git@别名:owner/repo.git，并写入提交身份。</div>
            </div>
            <div
              className={`switch ${rewrite && identityId ? "" : "off"}`}
              onClick={() => identityId && setRewrite(!rewrite)}
            />
          </div>
        </div>
        <div
          className="card-head"
          style={{ justifyContent: "flex-end", gap: 8, borderTop: "1px solid var(--border)", borderBottom: "none" }}
        >
          <button type="button" className="btn ghost sm" onClick={onClose}>
            取消
          </button>
          <button type="button" className="btn primary sm" disabled={busy || writesLocked} onClick={save}>
            {busy ? "保存中…" : "保存"}
          </button>
        </div>
      </div>
    </div>
  );
}
