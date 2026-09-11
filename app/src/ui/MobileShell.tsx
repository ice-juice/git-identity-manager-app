import { type ReactNode } from "react";
import { NavLink } from "react-router-dom";
import {
  LayoutDashboard,
  Cloud,
  Settings as SettingsIcon,
  Lock,
  Timer,
  UserRound,
  Sun,
  Moon,
  Palette,
} from "lucide-react";
import { AppLogo } from "./AppLogo";
import { useApp } from "../store";
import { APP_NAME } from "../lib/config";

/**
 * 移动端外壳：顶部精简标题 + 内容区 + 底部 Tab。
 *
 * 与桌面 `Layout` 的差别不只是"侧栏挪到底部"：
 * - 不显示工作空间路径（移动端路径固定在沙箱内，用户无从选择也无需知道）；
 * - 不显示窗口控制按钮；
 * - SSH 配置 / Agent / 仓库 / 克隆四个桌面专属页面不进导航。
 */

const TABS = [
  { to: "/", label: "总览", icon: LayoutDashboard, end: true },
  { to: "/totp", label: "验证码", icon: Timer },
  { to: "/accounts", label: "账号", icon: UserRound },
  { to: "/sync", label: "同步", icon: Cloud },
  { to: "/settings", label: "设置", icon: SettingsIcon },
];

export function MobileShell({ children }: { children: ReactNode }) {
  const { status, lock, writesLocked, startupNote, theme, toggleTheme } = useApp();
  const unlocked = !!status?.unlocked;
  const ThemeIcon = theme === "light" ? Sun : theme === "dark" ? Moon : Palette;

  return (
    <div className="m-shell">
      <header className="m-topbar">
        <AppLogo size={22} />
        <div className="m-topbar-title">{APP_NAME}</div>
        <div className="m-topbar-spacer" />
        <div className={"m-status" + (unlocked ? " on" : "")}>
          <span className="led" />
          {unlocked ? "已解锁" : "已锁定"}
        </div>
        <button
          type="button"
          className="m-icon-btn"
          aria-label="切换主题"
          onClick={toggleTheme}
        >
          <ThemeIcon size={17} />
        </button>
        <button
          type="button"
          className="m-icon-btn"
          aria-label="立即锁定"
          onClick={() => lock()}
        >
          <Lock size={17} />
        </button>
      </header>

      <main className="m-content">
        {writesLocked && (
          <div className="startup-lock-bar" role="status">
            <strong>同步中</strong>
            <span>{startupNote || "正在从云端同步，可浏览，暂不可修改。"}</span>
          </div>
        )}
        {children}
      </main>

      <nav className="m-tabbar">
        {TABS.map((t) => {
          const Icon = t.icon;
          return (
            <NavLink
              key={t.to}
              to={t.to}
              end={t.end}
              className={({ isActive }) => "m-tab" + (isActive ? " active" : "")}
            >
              <Icon size={20} />
              <span>{t.label}</span>
            </NavLink>
          );
        })}
      </nav>
    </div>
  );
}
