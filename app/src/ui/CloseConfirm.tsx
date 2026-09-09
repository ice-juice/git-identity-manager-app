import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, X } from "lucide-react";
import { api } from "../lib/ipc";

type CloseAction = "tray" | "quit" | "cancel";

export function CloseConfirmHost() {
  const [open, setOpen] = useState(false);
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen("close-requested", () => {
      setRemember(false);
      setOpen(true);
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {
        /* 非 Tauri 环境忽略 */
      });
    return () => {
      unlisten?.();
    };
  }, []);

  async function choose(action: CloseAction) {
    if (busy) return;
    setBusy(true);
    try {
      await api.applyCloseChoice(action, remember && action !== "cancel");
      setOpen(false);
    } finally {
      setBusy(false);
    }
  }

  if (!open) return null;

  return (
    <div className="close-overlay" role="dialog" aria-labelledby="close-dialog-title">
      <div className="close-dialog">
        <div className="close-dialog-titlebar">
          <span id="close-dialog-title">提示</span>
          <button type="button" className="close-dialog-x" onClick={() => choose("cancel")} disabled={busy} aria-label="关闭">
            <X size={14} />
          </button>
        </div>

        <div className="close-dialog-body">
          <div className="close-dialog-warn" aria-hidden>
            <AlertTriangle size={34} strokeWidth={2.2} />
          </div>
          <div className="close-dialog-content">
            <div className="close-dialog-q">您点击了关闭按钮，您确定:</div>
            <button type="button" className="close-dialog-opt preferred" disabled={busy} onClick={() => choose("tray")}>
              <span className="close-dialog-arrow">→</span>
              最小化到托盘
            </button>
            <button type="button" className="close-dialog-opt" disabled={busy} onClick={() => choose("quit")}>
              <span className="close-dialog-arrow">→</span>
              退出程序
            </button>
            <button type="button" className="close-dialog-opt" disabled={busy} onClick={() => choose("cancel")}>
              <span className="close-dialog-arrow">→</span>
              取消
            </button>
          </div>
        </div>

        <div className="close-dialog-footer">
          <label className="close-dialog-remember">
            <input type="checkbox" checked={remember} disabled={busy} onChange={(e) => setRemember(e.target.checked)} />
            记住我的选择
          </label>
        </div>
      </div>
    </div>
  );
}
