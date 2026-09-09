import { AlertTriangle, X } from "lucide-react";

const CHANGE_ITEMS = [
  "git 全局配置 core.sshCommand，改为 Git 自带 ssh",
  "当前用户环境变量 GIT_SSH、SSH_AUTH_SOCK、SSH_AGENT_PID",
  "PowerShell 启动脚本（$PROFILE）",
  "Git Bash 启动脚本（~/.bashrc）",
  "~/.ssh 下的本软件 agent 环境脚本",
];

export function EnvUnifyDialog({
  open,
  busy,
  error,
  ssh,
  sock,
  onCancel,
  onConfirm,
}: {
  open: boolean;
  busy?: boolean;
  error?: string;
  ssh?: string | null;
  sock?: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  if (!open) return null;

  return (
    <div className="close-overlay" role="dialog" aria-labelledby="env-unify-title" aria-modal="true">
      <div className="close-dialog env-unify-dialog">
        <div className="close-dialog-titlebar">
          <span id="env-unify-title">确认应用到本机环境</span>
          <button type="button" className="close-dialog-x" onClick={onCancel} disabled={busy} aria-label="关闭">
            <X size={14} />
          </button>
        </div>
        <div className="close-dialog-body">
          <div className="close-dialog-warn" aria-hidden>
            <AlertTriangle size={34} strokeWidth={2.2} />
          </div>
          <div className="close-dialog-content">
            <div className="close-dialog-q">将把当前 Git ssh-agent 应用到本机，让终端和 git 使用同一套环境。需你确认后才会执行：</div>
            <ul className="env-unify-list">
              {CHANGE_ITEMS.map((item) => (
                <li key={item}>{item}</li>
              ))}
            </ul>
            {(ssh || sock) && (
              <div className="mono muted sm env-unify-meta">
                {ssh && (
                  <>
                    ssh：{ssh}
                    <br />
                  </>
                )}
                {sock && <>sock：{sock}</>}
              </div>
            )}
            <div className="muted sm" style={{ marginTop: 8 }}>
              取消则不做任何修改。确认后，已打开的终端需新开窗口才会读到新变量。
            </div>
            {error && <div className="err-text" style={{ marginTop: 8 }}>{error}</div>}
          </div>
        </div>
        <div className="close-dialog-footer env-unify-actions">
          <button type="button" className="btn" disabled={busy} onClick={onCancel}>
            取消
          </button>
          <button type="button" className="btn primary" disabled={busy} onClick={onConfirm}>
            {busy ? "正在应用…" : "确认应用"}
          </button>
        </div>
      </div>
    </div>
  );
}
