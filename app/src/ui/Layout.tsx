import { useEffect, useState, type ReactNode } from "react";
import { NavLink } from "react-router-dom";
import {
  LayoutDashboard,
  KeyRound,
  FileCog,
  Cpu,
  FolderGit2,
  Download,
  Cloud,
  Settings as SettingsIcon,
  Lock,
  Sun,
  Moon,
  Palette,
} from "lucide-react";
import { api } from "../lib/ipc";
import { useApp } from "../store";
import { AppLogo } from "./AppLogo";
import { UpdateToast } from "./UpdateToast";

export function Layout({ children }: { children: ReactNode }) {
  const { status, lock, theme, toggleTheme, writesLocked, startupNote } = useApp();
  const themeLabel = theme === "light" ? "浅色" : theme === "dark" ? "深色" : "黛蓝";
  const [idCount, setIdCount] = useState<number | null>(null);
  const [keyCount, setKeyCount] = useState<number | null>(null);
  const [repoCount, setRepoCount] = useState<number | null>(null);

  useEffect(() => {
    let unmounted = false;
    (async () => {
      try {
        const [ids, ks, repos] = await Promise.all([
          api.listIdentities(),
          api.listKeys(),
          api.listManagedRepos().catch(() => []),
        ]);
        if (!unmounted) {
          setIdCount(ids.length);
          setKeyCount(ks.length);
          setRepoCount(repos.length);
        }
      } catch {
        /* 忽略 */
      }
    })();
    return () => {
      unmounted = true;
    };
  }, [status?.unlocked, writesLocked]);

  // 从工作空间路径提取简洁名称，例如 "D:/gitIdentifyData" -> "gitIdentifyData"
  const workspaceDisplay = (() => {
    if (!status?.workspacePath) return "加密工作空间";
    const p = status.workspacePath.replace(/\\/g, "/").replace(/\/+$/, "");
    const parts = p.split("/");
    return parts[parts.length - 1] || "工作空间";
  })();

  const NAV = [
    { to: "/", label: "身份总览", icon: LayoutDashboard, end: true, badge: idCount },
    { to: "/keys", label: "密钥管理", icon: KeyRound, badge: keyCount },
    { to: "/config", label: "SSH 配置 / 编辑", icon: FileCog },
    { to: "/agent", label: "Agent", icon: Cpu },
    { to: "/repos", label: "仓库管理", icon: FolderGit2, badge: repoCount },
    { to: "/clone", label: "克隆仓库", icon: Download },
    { to: "/sync", label: "云端同步", icon: Cloud },
  ];

  return (
    <div className="window">
      <div className="body">
        <aside className="sidebar">
          <div className="brand">
            <AppLogo size={28} />
            <div className="name">
              账号管理器
              <small>{workspaceDisplay} 的工作空间</small>
            </div>
          </div>
          <div className="nav-group">导航菜单</div>
          {NAV.map((n) => {
            const Icon = n.icon;
            return (
              <NavLink
                key={n.to}
                to={n.to}
                end={n.end}
                className={({ isActive }) => "nav-item" + (isActive ? " active" : "")}
              >
                <div className="nav-item-left">
                  <span className="ic">
                    <Icon size={15} />
                  </span>
                  <span>{n.label}</span>
                </div>
                {typeof n.badge === "number" && n.badge > 0 && (
                  <span className="nav-badge">{n.badge}</span>
                )}
              </NavLink>
            );
          })}
          <div className="grow" />
          <div className="side-foot">
            <NavLink
              to="/settings"
              className={({ isActive }) => "nav-item" + (isActive ? " active" : "")}
              style={{ marginBottom: 2 }}
            >
              <div className="nav-item-left">
                <span className="ic">
                  <SettingsIcon size={15} />
                </span>
                <span>设置</span>
              </div>
            </NavLink>
            <button type="button" className="nav-item" onClick={() => lock()}>
              <div className="nav-item-left">
                <span className="ic">
                  <Lock size={15} />
                </span>
                <span>立即锁定</span>
              </div>
            </button>
          </div>
        </aside>

        <main className="main">
          <div className="topbar">
            <div style={{ minWidth: 0 }}>
              <div className="muted" style={{ fontSize: 10 }}>工作空间</div>
              <div className="path-clip" title={status?.workspacePath ?? ""}>
                {status?.workspacePath ?? "—"}
              </div>
            </div>
            <div className="spacer" />
            <button
              type="button"
              className="btn ghost sm"
              style={{ display: "inline-flex", gap: 5, padding: "2px 7px" }}
              title={`当前主题：${themeLabel}（点击切换风格）`}
              onClick={toggleTheme}
            >
              {theme === "light" ? <Sun size={13} /> : theme === "dark" ? <Moon size={13} /> : <Palette size={13} />}
              <span style={{ fontSize: 11 }}>{themeLabel}</span>
            </button>
            <div className="dot4">
              <span className="led g" />
              已解锁
            </div>
          </div>
          <div className="content">
            {writesLocked && (
              <div className="startup-lock-bar" role="status">
                <strong>同步中</strong>
                <span>{startupNote || "正在从云端同步，可浏览，暂不可修改身份、密钥和仓库。"}</span>
              </div>
            )}
            {children}
          </div>
        </main>
      </div>
      <UpdateToast />
    </div>
  );
}
