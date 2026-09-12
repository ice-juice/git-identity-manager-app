import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Copy, Square, X, Sun, Moon, Palette } from "lucide-react";
import { AppLogo } from "./AppLogo";
import { useApp } from "../store";
import { APP_NAME } from "../lib/config";

const appWindow = (() => {
  try {
    return getCurrentWindow();
  } catch {
    return null;
  }
})();

/**
 * 自绘标题栏：横贯整个窗口顶部，与侧边栏、主区无缝融合。
 * 左段承接侧边栏（品牌），右段承接主区（工作空间路径 / 主题 / 状态 / 窗口按钮）。
 * 非按钮区域标记为拖拽域，可拖动窗口、双击最大化。
 */
export function TitleBar() {
  const { status, theme, toggleTheme } = useApp();
  const themeLabel = theme === "light" ? "浅色" : theme === "dark" ? "深色" : "黛蓝";
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!appWindow) return;
    let unlisten: (() => void) | undefined;
    appWindow.isMaximized().then(setMaximized).catch(() => {});
    appWindow
      .onResized(() => {
        appWindow.isMaximized().then(setMaximized).catch(() => {});
      })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => unlisten?.();
  }, []);

  const unlocked = !!status?.unlocked;

  return (
    <div className="titlebar">
      {/* 左段：品牌，宽度对齐侧边栏 */}
      <div className="tb-brand" data-tauri-drag-region>
        <AppLogo size={26} />
        {/* 只渲染编译期 APP_NAME。不要去读运行时产品名，否则中文包会变成英文品牌。 */}
        <div className="tb-title">{APP_NAME}</div>
      </div>

      {/* 右段：路径 + 操作 + 窗口控制 */}
      <div className="tb-main" data-tauri-drag-region>
        <div className="tb-ws" data-tauri-drag-region>
          <span className="tb-ws-label">工作空间</span>
          <span className="tb-ws-path" title={status?.workspacePath ?? ""}>
            {status?.workspacePath ?? "—"}
          </span>
        </div>

        <div className="tb-spacer" data-tauri-drag-region />

        <button
          type="button"
          className="tb-chip"
          title={`当前主题：${themeLabel}（点击切换风格）`}
          onClick={toggleTheme}
        >
          {theme === "light" ? <Sun size={13} /> : theme === "dark" ? <Moon size={13} /> : <Palette size={13} />}
          <span>{themeLabel}</span>
        </button>

        {status?.initialized && (
          <div className={"tb-status" + (unlocked ? " on" : "")} data-tauri-drag-region>
            <span className="led" />
            {unlocked ? "已解锁" : "已锁定"}
          </div>
        )}

        {/* 窗口控制按钮 */}
        <div className="tb-winbtns">
          <button
            type="button"
            className="tb-winbtn"
            aria-label="最小化"
            title="最小化"
            onClick={() => appWindow?.minimize()}
          >
            <Minus size={15} />
          </button>
          <button
            type="button"
            className="tb-winbtn"
            aria-label={maximized ? "还原" : "最大化"}
            title={maximized ? "还原" : "最大化"}
            onClick={() => appWindow?.toggleMaximize()}
          >
            {maximized ? <Copy size={12} /> : <Square size={12} />}
          </button>
          <button
            type="button"
            className="tb-winbtn danger"
            aria-label="关闭"
            title="关闭"
            onClick={() => appWindow?.close()}
          >
            <X size={16} />
          </button>
        </div>
      </div>
    </div>
  );
}
