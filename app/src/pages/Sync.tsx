import { useEffect, useState } from "react";
import {
  Cloud,
  CloudUpload,
  CloudDownload,
  RefreshCw,
  Shield,
  FileArchive,
  Download,
  Upload,
  Eye,
  EyeOff,
  Zap,
  History,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  api,
  errMessage,
  type S3Config,
  type CloudSyncStatus,
  type SyncResult,
  type BackupSummary,
  type AutoSyncSettings,
  type CloudSnapshot,
} from "../lib/ipc";
import { PageHead, Card, Badge } from "../ui/common";
import { useApp } from "../store";

const AUTO_PRESETS: { label: string; minutes: number }[] = [
  { label: "关闭", minutes: 0 },
  { label: "15 分钟", minutes: 15 },
  { label: "30 分钟 (推荐)", minutes: 30 },
  { label: "1 小时", minutes: 60 },
  { label: "2 小时", minutes: 120 },
];

export function SyncPage() {
  const { writesLocked } = useApp();
  const [activeTab, setActiveTab] = useState<"cloud" | "backup">("cloud");

  // 云端同步配置与状态
  const [s3Config, setS3Config] = useState<S3Config>({
    endpoint: "",
    bucket: "",
    region: "auto",
    accessKeyId: "",
    secretAccessKey: "",
    prefix: "gam-sync/",
  });
  const [showSecret, setShowSecret] = useState(false);
  const [cloudStatus, setCloudStatus] = useState<CloudSyncStatus | null>(null);
  const [loadingStatus, setLoadingStatus] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; msg: string } | null>(null);
  const [syncing, setSyncing] = useState<"push" | "pull" | null>(null);
  const [syncNotice, setSyncNotice] = useState<string | null>(null);
  const [configSaved, setConfigSaved] = useState(false);
  const [autoSync, setAutoSync] = useState<AutoSyncSettings | null>(null);
  const [savingAuto, setSavingAuto] = useState(false);
  const [snapshots, setSnapshots] = useState<CloudSnapshot[]>([]);
  const [loadingSnaps, setLoadingSnaps] = useState(false);
  const [restoringId, setRestoringId] = useState<string | null>(null);

  // 本地离线备份导出
  const [exportPw, setExportPw] = useState("");
  const [exportPw2, setExportPw2] = useState("");
  const [exporting, setExporting] = useState(false);
  const [exportResult, setExportResult] = useState<BackupSummary | null>(null);
  const [exportErr, setExportErr] = useState<string | null>(null);

  // 本地离线备份导入
  const [importPath, setImportPath] = useState("");
  const [importPw, setImportPw] = useState("");
  const [inspecting, setInspecting] = useState(false);
  const [importSummary, setImportSummary] = useState<BackupSummary | null>(null);
  const [importing, setImporting] = useState(false);
  const [importSuccess, setImportSuccess] = useState<BackupSummary | null>(null);
  const [importErr, setImportErr] = useState<string | null>(null);

  // 加载已保存配置和状态
  async function loadInitial() {
    setLoadingStatus(true);
    try {
      const cfg = await api.getCloudSyncConfig();
      if (cfg) {
        setS3Config(cfg);
      }
      const st = await api.getCloudSyncStatus();
      setCloudStatus(st);
      const auto = await api.getAutoSyncSettings();
      setAutoSync(auto);
      try {
        setSnapshots(await api.listCloudSnapshots());
      } catch {
        setSnapshots([]);
      }
    } catch (e) {
      console.error(e);
    } finally {
      setLoadingStatus(false);
    }
  }

  async function loadSnapshots() {
    setLoadingSnaps(true);
    try {
      setSnapshots(await api.listCloudSnapshots());
    } catch {
      setSnapshots([]);
    } finally {
      setLoadingSnaps(false);
    }
  }

  useEffect(() => {
    loadInitial();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<{ ok: boolean; message: string; syncedAt?: string }>("cloud-auto-sync", (ev) => {
      const p = ev.payload;
      setSyncNotice(`${p.ok ? "✅" : "❌"} ${p.message}`);
      void loadInitial();
      void loadSnapshots();
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {
        /* 非 Tauri */
      });
    return () => {
      unlisten?.();
    };
  }, []);

  // 快捷预设
  function applyPreset(type: "r2" | "s3" | "minio") {
    if (type === "r2") {
      setS3Config((prev) => ({
        ...prev,
        endpoint: "https://<account_id>.r2.cloudflarestorage.com",
        region: "auto",
        prefix: "gam-sync/",
      }));
    } else if (type === "s3") {
      setS3Config((prev) => ({
        ...prev,
        endpoint: "https://s3.us-east-1.amazonaws.com",
        region: "us-east-1",
        prefix: "gam-sync/",
      }));
    } else {
      setS3Config((prev) => ({
        ...prev,
        endpoint: "http://127.0.0.1:9000",
        region: "us-east-1",
        prefix: "gam-sync/",
      }));
    }
  }

  // 保存云存储配置
  async function saveConfig() {
    try {
      await api.saveCloudSyncConfig(s3Config);
      setConfigSaved(true);
      setTimeout(() => setConfigSaved(false), 3000);
      refreshStatus();
    } catch (e) {
      alert("保存失败: " + errMessage(e));
    }
  }

  async function exportS3File() {
    try {
      const selected = await save({
        defaultPath: `gam-s3-${s3Config.bucket || "config"}.json`,
        filters: [{ name: "GAM S3/R2 配置", extensions: ["json"] }],
      });
      if (!selected) return;
      await api.exportS3Config(selected, s3Config);
      setSyncNotice("✅ 已导出 S3/R2 配置文件");
    } catch (e) {
      setSyncNotice(`❌ 导出配置失败: ${errMessage(e)}`);
    }
  }

  async function importS3File() {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "GAM S3/R2 配置", extensions: ["json"] }],
      });
      if (!selected || typeof selected !== "string") return;
      const cfg = await api.importS3Config(selected);
      setS3Config(cfg);
      setSyncNotice("✅ 已导入 S3/R2 配置，请确认后保存");
    } catch (e) {
      setSyncNotice(`❌ 导入配置失败: ${errMessage(e)}`);
    }
  }

  // 测试云端连通性
  async function testConnection() {
    if (!s3Config.endpoint || !s3Config.bucket || !s3Config.accessKeyId || !s3Config.secretAccessKey) {
      setTestResult({ ok: false, msg: "请先填写完整的 Endpoint、Bucket、AK 与 SK" });
      return;
    }
    setTesting(true);
    setTestResult(null);
    try {
      const ms = await api.testCloudSyncConfig(s3Config);
      setTestResult({ ok: true, msg: `连接正常，读写探测延迟 ${ms} ms` });
    } catch (e) {
      setTestResult({ ok: false, msg: errMessage(e) });
    } finally {
      setTesting(false);
    }
  }

  // 刷新云同步状态
  async function refreshStatus() {
    setLoadingStatus(true);
    try {
      const st = await api.getCloudSyncStatus();
      setCloudStatus(st);
    } catch (e) {
      console.error(e);
    } finally {
      setLoadingStatus(false);
    }
  }

  // 推送到云端 (Push)
  async function handlePush() {
    if (!cloudStatus || cloudStatus.status === "unconfigured") {
      alert("请先填写并保存云存储配置");
      return;
    }
    if (!confirm("确定要将当前本地工作空间加密推送到云端存储吗？")) return;
    setSyncing("push");
    setSyncNotice(null);
    try {
      const res: SyncResult = await api.cloudSyncPush();
      setSyncNotice(`✅ ${res.message}`);
      await refreshStatus();
      await loadSnapshots();
    } catch (e) {
      setSyncNotice(`❌ 推送失败: ${errMessage(e)}`);
    } finally {
      setSyncing(null);
    }
  }

  // 从云端拉取 (Pull)
  async function handlePull() {
    if (!cloudStatus || cloudStatus.status === "unconfigured") {
      alert("请先填写并保存云存储配置");
      return;
    }
    if (!confirm("确定要从云端存储拉取最新加密数据并合并到本地吗？")) return;
    setSyncing("pull");
    setSyncNotice(null);
    try {
      const res: SyncResult = await api.cloudSyncPull();
      setSyncNotice(`✅ ${res.message}（${res.identityCount} 个身份，${res.keyCount} 把密钥）`);
      await refreshStatus();
    } catch (e) {
      setSyncNotice(`❌ 拉取失败: ${errMessage(e)}`);
    } finally {
      setSyncing(null);
    }
  }

  // 触发离线备份导出
  async function handleExportBackup() {
    setExportErr(null);
    setExportResult(null);
    if (!exportPw || exportPw.length < 6) {
      setExportErr("备份加密密码不能少于 6 位");
      return;
    }
    if (exportPw !== exportPw2) {
      setExportErr("两次输入的备份密码不一致");
      return;
    }

    const defaultName = `gam-backup-${new Date().toISOString().slice(0, 10)}.gambackup`;
    const selected = await save({
      defaultPath: defaultName,
      filters: [{ name: "GAM 加密备份包", extensions: ["gambackup"] }],
    });

    if (!selected) return;

    setExporting(true);
    try {
      const res = await api.exportVaultBackup(selected, exportPw);
      setExportResult(res);
      setExportPw("");
      setExportPw2("");
    } catch (e) {
      setExportErr(errMessage(e));
    } finally {
      setExporting(false);
    }
  }

  // 选择待导入备份文件
  async function pickImportFile() {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "GAM 加密备份包", extensions: ["gambackup"] }],
    });
    if (selected && typeof selected === "string") {
      setImportPath(selected);
      setImportSummary(null);
      setImportErr(null);
      setImportSuccess(null);
    }
  }

  // 查看备份包内容
  async function handleInspectBackup() {
    if (!importPath) {
      setImportErr("请先选择备份文件");
      return;
    }
    if (!importPw) {
      setImportErr("请输入备份加密密码");
      return;
    }
    setInspecting(true);
    setImportErr(null);
    try {
      const res = await api.inspectVaultBackup(importPath, importPw);
      setImportSummary(res);
    } catch (e) {
      setImportErr(errMessage(e));
    } finally {
      setInspecting(false);
    }
  }

  // 确认导入合并
  async function handleConfirmImport() {
    if (!importPath || !importPw) return;
    setImporting(true);
    setImportErr(null);
    try {
      const res = await api.importVaultBackup(importPath, importPw, true);
      setImportSuccess(res);
      setImportSummary(null);
      setImportPath("");
      setImportPw("");
    } catch (e) {
      setImportErr(errMessage(e));
    } finally {
      setImporting(false);
    }
  }

  const statusLabel = (() => {
    if (!cloudStatus) return { text: "检测中", cls: "muted" };
    switch (cloudStatus.status) {
      case "synced":
        return { text: "已同步", cls: "ok" };
      case "local_ahead":
        return { text: "本地有待推送变更", cls: "warn" };
      case "remote_ahead":
        return { text: "云端有待拉取更新", cls: "info" };
      case "different_workspace":
        return { text: "与当前空间不一致", cls: "danger" };
      case "not_synced":
        return { text: "云端暂无备份", cls: "warn" };
      default:
        return { text: "未配置云存储", cls: "muted" };
    }
  })();

  return (
    <div className="stack-lg">
      <PageHead
        title="云端同步与备份"
        desc="多端零知识加密同步与本地离线归档备份"
        actions={
          <div className="flex gap-2">
            <button
              type="button"
              className={`btn sm ${activeTab === "cloud" ? "primary" : "ghost"}`}
              onClick={() => setActiveTab("cloud")}
            >
              <Cloud size={13} style={{ marginRight: 4 }} />
              云端同步 (E2EE)
            </button>
            <button
              type="button"
              className={`btn sm ${activeTab === "backup" ? "primary" : "ghost"}`}
              onClick={() => setActiveTab("backup")}
            >
              <FileArchive size={13} style={{ marginRight: 4 }} />
              离线备份包
            </button>
          </div>
        }
      />

      {activeTab === "cloud" && (
        <>
          {/* 板块 1: 云同步状态与快速操作 */}
          <Card
            title="云端同步状态"
            actions={
              <div className="flex items-center gap-2">
                <Badge kind={statusLabel.cls}>{statusLabel.text}</Badge>
                <button
                  type="button"
                  className="btn ghost sm"
                  disabled={loadingStatus}
                  onClick={refreshStatus}
                  style={{ display: "inline-flex", alignItems: "center", gap: 4 }}
                >
                  <RefreshCw size={12} className={loadingStatus ? "animate-spin" : ""} />
                  刷新
                </button>
              </div>
            }
          >
            <div className="stat-card-row mb-3">
              <div className="stat-card">
                <div className="stat-card-title">本地资产</div>
                <div className="stat-card-body">
                  <div className="stat-card-val" style={{ fontSize: 18 }}>
                    {cloudStatus?.localIdentityCount ?? 0}
                  </div>
                  <div className="stat-card-sub">
                    {cloudStatus?.localKeyCount ?? 0} 密钥 · {cloudStatus?.localRepoCount ?? 0} 仓库
                  </div>
                </div>
              </div>
              <div className="stat-card">
                <div className="stat-card-title">云端状态</div>
                <div className="stat-card-body">
                  <div
                    className="stat-card-val"
                    style={{
                      fontSize: 16,
                      color: cloudStatus?.remoteExists ? "var(--green)" : "var(--text-soft)",
                    }}
                  >
                    {cloudStatus?.remoteExists ? "已就绪" : "待初次推送"}
                  </div>
                  <div className="stat-card-sub">
                    {cloudStatus?.headerReady ? "换机恢复头部正常" : "无云端快照"}
                  </div>
                </div>
              </div>
              <div className="stat-card">
                <div className="stat-card-title">最近同步</div>
                <div className="stat-card-body">
                  <div className="stat-card-val" style={{ fontSize: 13, lineHeight: 1.4 }}>
                    {cloudStatus?.remoteUpdatedAt ? new Date(cloudStatus.remoteUpdatedAt).toLocaleTimeString() : "—"}
                  </div>
                  <div className="stat-card-sub">
                    {cloudStatus?.remoteUpdatedAt ? new Date(cloudStatus.remoteUpdatedAt).toLocaleDateString() : "尚未同步"}
                  </div>
                </div>
              </div>
              <div className="stat-card">
                <div className="stat-card-title">安全算法</div>
                <div className="stat-card-body">
                  <div className="stat-card-val" style={{ fontSize: 15, color: "var(--accent)" }}>
                    E2EE
                  </div>
                  <div className="stat-card-sub">
                    XChaCha20-Poly1305
                  </div>
                </div>
              </div>
            </div>

            {syncNotice && (
              <div
                style={{
                  padding: "7px 10px",
                  borderRadius: 6,
                  fontSize: 11.5,
                  marginBottom: 10,
                  background: syncNotice.startsWith("✅") ? "rgba(16, 185, 129, 0.1)" : "rgba(239, 68, 68, 0.1)",
                  color: syncNotice.startsWith("✅") ? "var(--green)" : "var(--red)",
                  border: "1px solid",
                  borderColor: syncNotice.startsWith("✅") ? "rgba(16, 185, 129, 0.3)" : "rgba(239, 68, 68, 0.3)",
                }}
              >
                {syncNotice}
              </div>
            )}

            <div className="sync-action-bar">
              <div className="muted" style={{ fontSize: 11.5, lineHeight: 1.5, flex: 1, minWidth: 180 }}>
                数据出机前全量加密，新设备可在初始化时凭恢复密钥直接还原。
              </div>
              <div className="sync-action-btns">
                <button
                  type="button"
                  className="btn primary"
                  disabled={writesLocked || syncing !== null || cloudStatus?.status === "unconfigured"}
                  onClick={handlePush}
                >
                  <CloudUpload size={14} />
                  {syncing === "push" ? "正在加密推送…" : "推送到云端 (Push)"}
                </button>
                <button
                  type="button"
                  className="btn"
                  disabled={syncing !== null || cloudStatus?.status === "unconfigured" || !cloudStatus?.remoteExists}
                  onClick={handlePull}
                >
                  <CloudDownload size={14} />
                  {syncing === "pull" ? "正在同步…" : "从云端拉取 (Pull)"}
                </button>
              </div>
            </div>
          </Card>

          {/* 板块 2: 精简后的定时同步 */}
          <Card
            title="定时自动同步"
            actions={
              <button
                type="button"
                className="btn ghost sm"
                disabled={syncing !== null || cloudStatus?.status === "unconfigured"}
                onClick={async () => {
                  setSyncing("pull");
                  setSyncNotice(null);
                  try {
                    const res = await api.runAutoSyncNow();
                    setSyncNotice(res ? `✅ ${res.message}` : "当前已是最新状态");
                    await refreshStatus();
                    await loadSnapshots();
                  } catch (e) {
                    setSyncNotice(`❌ ${errMessage(e)}`);
                  } finally {
                    setSyncing(null);
                  }
                }}
                style={{ display: "inline-flex", alignItems: "center", gap: 4 }}
              >
                <RefreshCw size={12} className={syncing === "pull" ? "animate-spin" : ""} />
                立即同步一次
              </button>
            }
          >
            <div className="stack">
              <div className="between" style={{ flexWrap: "wrap", gap: 8 }}>
                <div>
                  <div style={{ fontSize: 11.5, fontWeight: 600 }}>同步周期</div>
                  <div className="muted" style={{ fontSize: 11, marginTop: 1 }}>
                    编辑身份或密钥后会自动推送；程序打开或唤醒时自动拉取。
                  </div>
                </div>
                <div className="choice-row" style={{ margin: 0 }}>
                  {AUTO_PRESETS.map((p) => {
                    const on = (autoSync?.minutes ?? 30) === p.minutes;
                    return (
                      <button
                        key={p.minutes}
                        type="button"
                        className={"choice" + (on ? " on" : "")}
                        disabled={savingAuto}
                        style={{ padding: "4px 9px", fontSize: 11 }}
                        onClick={async () => {
                          setSavingAuto(true);
                          try {
                            const minutes = await api.setAutoSyncMinutes(p.minutes);
                            setAutoSync((prev) =>
                              prev ? { ...prev, minutes } : { minutes, defaultMinutes: 30, lastAutoSyncAt: null, lastAutoSyncMessage: null },
                            );
                          } catch (e) {
                            setSyncNotice(`❌ ${errMessage(e)}`);
                          } finally {
                            setSavingAuto(false);
                          }
                        }}
                      >
                        {p.label}
                      </button>
                    );
                  })}
                </div>
              </div>
              {autoSync?.lastAutoSyncAt && (
                <div className="muted sm" style={{ borderTop: "1px dashed var(--border)", paddingTop: 5, marginTop: 2 }}>
                  最近自动同步：{new Date(autoSync.lastAutoSyncAt).toLocaleString()}
                  {autoSync.lastAutoSyncMessage ? ` · ${autoSync.lastAutoSyncMessage}` : ""}
                </div>
              )}
            </div>
          </Card>

          {/* 板块 3: S3 / Cloudflare R2 存储配置 (上移到历史快照之前) */}
          <Card
            title="S3 / Cloudflare R2 存储配置"
            actions={
              <div className="flex gap-1">
                <span className="muted" style={{ fontSize: 11, alignSelf: "center", marginRight: 4 }}>快速预设:</span>
                <button type="button" className="btn ghost sm" onClick={() => applyPreset("r2")}>Cloudflare R2</button>
                <button type="button" className="btn ghost sm" onClick={() => applyPreset("s3")}>AWS S3</button>
                <button type="button" className="btn ghost sm" onClick={() => applyPreset("minio")}>MinIO</button>
              </div>
            }
          >
            <div className="grid grid-cols-2 gap-3 mb-3">
              <div>
                <label className="field-label">存储端点 (Endpoint)</label>
                <input
                  className="input mono"
                  placeholder="https://<account_id>.r2.cloudflarestorage.com"
                  value={s3Config.endpoint}
                  onChange={(e) => setS3Config({ ...s3Config, endpoint: e.target.value })}
                />
              </div>
              <div>
                <label className="field-label">存储桶名称 (Bucket)</label>
                <input
                  className="input mono"
                  placeholder="my-git-vault-backup"
                  value={s3Config.bucket}
                  onChange={(e) => setS3Config({ ...s3Config, bucket: e.target.value })}
                />
              </div>
              <div>
                <label className="field-label">区域 (Region)</label>
                <input
                  className="input mono"
                  placeholder="auto 或 us-east-1"
                  value={s3Config.region}
                  onChange={(e) => setS3Config({ ...s3Config, region: e.target.value })}
                />
              </div>
              <div>
                <label className="field-label">路径前缀 (Prefix)</label>
                <input
                  className="input mono"
                  placeholder="gam-sync/"
                  value={s3Config.prefix}
                  onChange={(e) => setS3Config({ ...s3Config, prefix: e.target.value })}
                />
              </div>
              <div>
                <label className="field-label">Access Key ID</label>
                <input
                  className="input mono"
                  placeholder="AKIA..."
                  value={s3Config.accessKeyId}
                  onChange={(e) => setS3Config({ ...s3Config, accessKeyId: e.target.value })}
                />
              </div>
              <div>
                <label className="field-label">Secret Access Key</label>
                <div style={{ position: "relative" }}>
                  <input
                    type={showSecret ? "text" : "password"}
                    className="input mono"
                    placeholder="Secret Key"
                    value={s3Config.secretAccessKey}
                    onChange={(e) => setS3Config({ ...s3Config, secretAccessKey: e.target.value })}
                    style={{ paddingRight: 32 }}
                  />
                  <button
                    type="button"
                    className="btn ghost sm"
                    onClick={() => setShowSecret(!showSecret)}
                    style={{ position: "absolute", right: 4, top: 4, padding: "2px 6px" }}
                  >
                    {showSecret ? <EyeOff size={13} /> : <Eye size={13} />}
                  </button>
                </div>
              </div>
            </div>

            {testResult && (
              <div
                style={{
                  padding: "7px 10px",
                  borderRadius: 6,
                  fontSize: 11.5,
                  marginBottom: 10,
                  background: testResult.ok ? "rgba(16, 185, 129, 0.1)" : "rgba(239, 68, 68, 0.1)",
                  color: testResult.ok ? "var(--green)" : "var(--red)",
                  border: "1px solid",
                  borderColor: testResult.ok ? "rgba(16, 185, 129, 0.3)" : "rgba(239, 68, 68, 0.3)",
                }}
              >
                {testResult.ok ? `✅ ${testResult.msg}` : `❌ ${testResult.msg}`}
              </div>
            )}

            <div className="flex gap-2 items-center">
              <button
                type="button"
                className="btn primary sm"
                onClick={saveConfig}
              >
                {configSaved ? "已保存配置 ✓" : "保存配置"}
              </button>
              <button
                type="button"
                className="btn ghost sm"
                disabled={testing}
                onClick={testConnection}
                style={{ display: "inline-flex", alignItems: "center", gap: 5 }}
              >
                <Zap size={13} />
                {testing ? "正在测试…" : "测试连通性"}
              </button>
              <button type="button" className="btn ghost sm" onClick={exportS3File}>
                <Download size={13} />
                导出配置
              </button>
              <button type="button" className="btn ghost sm" onClick={importS3File}>
                <Upload size={13} />
                导入配置
              </button>
            </div>
          </Card>

          {/* 板块 4: 云端历史快照 (下移一个板块至存储配置下方) */}
          <Card
            title="云端历史快照"
            actions={
              <button
                type="button"
                className="btn ghost sm"
                disabled={loadingSnaps || cloudStatus?.status === "unconfigured"}
                onClick={loadSnapshots}
                style={{ display: "inline-flex", alignItems: "center", gap: 4 }}
              >
                <History size={12} />
                {loadingSnaps ? "读取中…" : "刷新快照"}
              </button>
            }
          >
            <div className="muted" style={{ fontSize: 11.5, marginBottom: 8 }}>
              自动保留最近 10 份快照与近 14 天每日版本，支持一键按需回滚。
            </div>
            {snapshots.length === 0 ? (
              <div className="muted sm">暂无历史快照，完成初次推送后将在此显示。</div>
            ) : (
              <div className="list">
                {snapshots.map((s, i) => {
                  const day = new Date(s.createdAt).toLocaleDateString();
                  const prevDay = i > 0 ? new Date(snapshots[i - 1].createdAt).toLocaleDateString() : "";
                  const showDay = day !== prevDay;
                  return (
                    <div className="list-row" key={s.id} style={{ flexDirection: "column", alignItems: "stretch", gap: 5 }}>
                      {showDay && (
                        <div className="muted sm" style={{ fontWeight: 600 }}>{day}</div>
                      )}
                      <div className="row" style={{ justifyContent: "space-between", gap: 8 }}>
                        <div className="grow">
                          <div className="row" style={{ gap: 6, flexWrap: "wrap" }}>
                            <span>{new Date(s.createdAt).toLocaleString()}</span>
                            {s.isDailyFirst && <Badge kind="info">当日首份</Badge>}
                            {s.isRecent && <Badge kind="ok">最近</Badge>}
                          </div>
                          <div className="muted sm" style={{ marginTop: 2 }}>
                            {s.identityCount} 个身份 · {s.keyCount} 把密钥 · {s.repoCount} 个仓库
                            {s.clientName ? ` · ${s.clientName}` : ""}
                          </div>
                        </div>
                        <button
                          type="button"
                          className="btn ghost sm"
                          disabled={writesLocked || restoringId !== null}
                          onClick={async () => {
                            if (!confirm(`确定恢复到 ${new Date(s.createdAt).toLocaleString()} 的快照吗？将覆盖当前身份、密钥与配置。`)) return;
                            setRestoringId(s.id);
                            try {
                              const res = await api.restoreCloudSnapshot(s.id);
                              setSyncNotice(`✅ ${res.message}`);
                              await refreshStatus();
                            } catch (e) {
                              setSyncNotice(`❌ 恢复失败: ${errMessage(e)}`);
                            } finally {
                              setRestoringId(null);
                            }
                          }}
                        >
                          {restoringId === s.id ? "恢复中…" : "恢复此版本"}
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </Card>

          {/* 底部轻量安全提示 */}
          <div
            style={{
              padding: "9px 12px",
              borderRadius: "var(--radius)",
              background: "rgba(99, 102, 241, 0.05)",
              border: "1px solid rgba(99, 102, 241, 0.18)",
              fontSize: 11.5,
              lineHeight: 1.5,
              color: "var(--text-soft)",
              display: "flex",
              alignItems: "center",
              gap: 8,
            }}
          >
            <Shield size={15} style={{ color: "var(--accent)", flexShrink: 0 }} />
            <div>
              <strong>零知识端到端加密：</strong>所有资产均在本地通过 XChaCha20-Poly1305 加密后上传，云端文件名经 HMAC 散列混淆，服务商及任何第三方均无法解密。
            </div>
          </div>
        </>
      )}

      {activeTab === "backup" && (
        <div className="stack-lg">
          {/* M6 本地离线加密导出 */}
          <Card title="导出离线加密备份 (.gambackup)">
            <p className="muted" style={{ fontSize: 11.5, marginBottom: 10 }}>
              将工作空间身份、密钥与仓库私钥打包为单一离线加密文件，适合本地冷备份或换机导入。
            </p>

            <div className="grid grid-cols-2 gap-3 mb-3">
              <div>
                <label className="field-label">设置备份加密密码 (至少 6 位)</label>
                <input
                  type="password"
                  className="input"
                  placeholder="输入密码"
                  value={exportPw}
                  onChange={(e) => setExportPw(e.target.value)}
                />
              </div>
              <div>
                <label className="field-label">再次确认密码</label>
                <input
                  type="password"
                  className="input"
                  placeholder="重复输入密码"
                  value={exportPw2}
                  onChange={(e) => setExportPw2(e.target.value)}
                />
              </div>
            </div>

            {exportErr && (
              <div className="err-text mb-3">❌ {exportErr}</div>
            )}

            {exportResult && (
              <div
                style={{
                  padding: "7px 10px",
                  borderRadius: 6,
                  fontSize: 11.5,
                  marginBottom: 10,
                  background: "rgba(16, 185, 129, 0.1)",
                  color: "var(--green)",
                  border: "1px solid rgba(16, 185, 129, 0.3)",
                }}
              >
                ✅ 备份导出成功：包含 {exportResult.identityCount} 个身份，{exportResult.keyCount} 把密钥，{exportResult.repoCount} 个仓库。
              </div>
            )}

            <button
              type="button"
              className="btn primary sm"
              disabled={exporting}
              onClick={handleExportBackup}
              style={{ display: "inline-flex", alignItems: "center", gap: 5 }}
            >
              <Download size={13} />
              {exporting ? "正在打包并加密…" : "导出离线备份"}
            </button>
          </Card>

          {/* M6 本地离线备份导入 */}
          <Card title="导入与还原离线备份">
            <p className="muted" style={{ fontSize: 11.5, marginBottom: 10 }}>
              从已导出的 <code>.gambackup</code> 文件中恢复数据，可先解密预览清单后再确认合并。
            </p>

            <div className="grid grid-cols-2 gap-3 mb-3">
              <div>
                <label className="field-label">选择备份文件 (.gambackup)</label>
                <div className="flex gap-2">
                  <input
                    className="input grow"
                    readOnly
                    placeholder="未选择备份文件"
                    value={importPath}
                  />
                  <button type="button" className="btn ghost sm" onClick={pickImportFile}>
                    浏览…
                  </button>
                </div>
              </div>
              <div>
                <label className="field-label">备份加密密码</label>
                <input
                  type="password"
                  className="input"
                  placeholder="输入导出时的密码"
                  value={importPw}
                  onChange={(e) => setImportPw(e.target.value)}
                />
              </div>
            </div>

            {importErr && (
              <div className="err-text mb-3">❌ {importErr}</div>
            )}

            {importSuccess && (
              <div
                style={{
                  padding: "7px 10px",
                  borderRadius: 6,
                  fontSize: 11.5,
                  marginBottom: 10,
                  background: "rgba(16, 185, 129, 0.1)",
                  color: "var(--green)",
                  border: "1px solid rgba(16, 185, 129, 0.3)",
                }}
              >
                ✅ 备份导入成功：当前工作空间已合并更新（{importSuccess.identityCount} 个身份，{importSuccess.keyCount} 把密钥）。
              </div>
            )}

            {importSummary && (
              <div
                style={{
                  padding: "9px 12px",
                  borderRadius: 6,
                  fontSize: 11.5,
                  background: "rgba(99, 102, 241, 0.08)",
                  border: "1px solid rgba(99, 102, 241, 0.25)",
                  marginBottom: 10,
                }}
              >
                <div style={{ fontWeight: 600, color: "var(--accent)", marginBottom: 3 }}>
                  📦 备份包解密成功，资产清单：
                </div>
                <div style={{ color: "var(--text)" }}>
                  • 备份时间：{new Date(importSummary.createdAt).toLocaleString()} · 身份：<b>{importSummary.identityCount}</b> 个 · 密钥：<b>{importSummary.keyCount}</b> 把 · 仓库：<b>{importSummary.repoCount}</b> 个
                </div>
              </div>
            )}

            <div className="flex gap-2">
              <button
                type="button"
                className="btn ghost sm"
                disabled={inspecting || !importPath}
                onClick={handleInspectBackup}
                style={{ display: "inline-flex", alignItems: "center", gap: 5 }}
              >
                <Eye size={13} />
                {inspecting ? "正在验证解密…" : "解密并预览"}
              </button>
              {importSummary && (
                <button
                  type="button"
                  className="btn primary sm"
                  disabled={importing}
                  onClick={handleConfirmImport}
                  style={{ display: "inline-flex", alignItems: "center", gap: 5 }}
                >
                  <Upload size={13} />
                  {importing ? "正在导入合并…" : "确认合并到当前空间"}
                </button>
              )}
            </div>
          </Card>
        </div>
      )}
    </div>
  );
}
