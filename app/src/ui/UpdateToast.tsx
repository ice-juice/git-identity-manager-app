import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useNavigate } from "react-router-dom";
import { api, errMessage, type UpdateCheckResult, type UpdateProgress } from "../lib/ipc";

export function UpdateToast() {
  const navigate = useNavigate();
  const [notice, setNotice] = useState<UpdateCheckResult | null>(null);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  useEffect(() => {
    let unavail: (() => void) | undefined;
    let unprog: (() => void) | undefined;
    listen<UpdateCheckResult>("update-available", (ev) => {
      if (ev.payload?.available) {
        setNotice(ev.payload);
        setErr("");
      }
    })
      .then((fn) => {
        unavail = fn;
      })
      .catch(() => {});
    listen<UpdateProgress>("update-progress", (ev) => {
      setProgress(ev.payload);
    })
      .then((fn) => {
        unprog = fn;
      })
      .catch(() => {});
    return () => {
      unavail?.();
      unprog?.();
    };
  }, []);

  async function install() {
    setBusy(true);
    setErr("");
    try {
      await api.downloadAndInstallUpdate();
    } catch (e) {
      setErr(errMessage(e));
      setBusy(false);
    }
  }

  async function skip() {
    const ver = notice?.latestVersion;
    if (!ver) return;
    setBusy(true);
    try {
      await api.skipUpdateVersion(ver);
      setNotice(null);
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const downloading = progress && progress.phase !== "finished";
  const percent =
    progress && progress.total && progress.total > 0
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : null;

  if (!notice && !progress) return null;

  return (
    <div className="update-toast" role="status">
      {progress?.phase === "finished" ? (
        <div className="update-toast-title">更新已就绪，即将重启…</div>
      ) : downloading ? (
        <>
          <div className="update-toast-title">正在下载更新…</div>
          <div className="update-progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent ?? 0}>
            <span style={{ width: `${percent ?? 15}%` }} />
          </div>
          <div className="muted" style={{ fontSize: 11, marginTop: 6 }}>
            {percent != null
              ? `${percent}%`
              : `${Math.round(progress.downloaded / 1024)} KB`}
          </div>
        </>
      ) : notice ? (
        <>
          <div className="update-toast-title">发现新版本 v{notice.latestVersion}</div>
          <div className="muted" style={{ fontSize: 12, marginTop: 4 }}>
            当前 {notice.currentVersion}
            {notice.selfUpdateSupported ? "，可直接下载安装。" : "，当前安装方式请手动下载。"}
          </div>
          {err && <div className="err-text">{err}</div>}
          <div className="row" style={{ flexWrap: "wrap", marginTop: 10 }}>
            {notice.selfUpdateSupported && (
              <button type="button" className="btn primary sm" disabled={busy} onClick={install}>
                下载并安装
              </button>
            )}
            {notice.downloadUrl && (
              <button type="button" className="btn sm" onClick={() => api.openUrl(notice.downloadUrl!)}>
                手动下载
              </button>
            )}
            <button
              type="button"
              className="btn ghost sm"
              onClick={() => {
                setNotice(null);
                navigate("/settings");
              }}
            >
              查看说明
            </button>
            <button type="button" className="btn ghost sm" disabled={busy} onClick={skip}>
              跳过
            </button>
          </div>
        </>
      ) : null}
    </div>
  );
}
