import type { ReactNode } from "react";
import { NavLink } from "react-router-dom";
import {
  LayoutDashboard,
  KeyRound,
  FileCog,
  Cpu,
  FolderGit2,
  Settings as SettingsIcon,
  Lock,
} from "lucide-react";
import { useApp } from "../store";

const NAV = [
  { to: "/", label: "身份总览", icon: LayoutDashboard, end: true },
  { to: "/keys", label: "密钥管理", icon: KeyRound },
  { to: "/config", label: "SSH 配置", icon: FileCog },
  { to: "/agent", label: "Agent 管理", icon: Cpu },
  { to: "/repos", label: "仓库与地址", icon: FolderGit2 },
  { to: "/settings", label: "设置", icon: SettingsIcon },
];

export function Layout({ children }: { children: ReactNode }) {
  const { status, lock } = useApp();
  return (
    <div className="window">
      <div className="body">
        <aside className="sidebar">
          <div className="brand">
            <div className="logo">🔐</div>
            <div className="name">
              Git 多账号管理器
              <small>本地加密工作空间</small>
            </div>
          </div>
          <div className="nav-group">导航</div>
          {NAV.map((n) => {
            const Icon = n.icon;
            return (
              <NavLink
                key={n.to}
                to={n.to}
                end={n.end}
                className={({ isActive }) => "nav-item" + (isActive ? " active" : "")}
              >
                <span className="ic">
                  <Icon size={17} />
                </span>
                {n.label}
              </NavLink>
            );
          })}
          <div className="grow" />
          <div className="side-foot">
            <button className="nav-item" style={{ width: "100%" }} onClick={() => lock()}>
              <span className="ic">
                <Lock size={17} />
              </span>
              锁定工作空间
            </button>
          </div>
        </aside>
        <main className="main">
          <div className="topbar">
            <div>
              <div style={{ fontSize: 12.5, color: "var(--text-mute)" }}>工作空间</div>
              <div className="mono" style={{ fontSize: 12 }}>
                {status?.workspacePath ?? "—"}
              </div>
            </div>
            <div className="spacer" />
            <div className="dot4">
              <span className="led g" />
              已解锁
            </div>
          </div>
          <div className="content">{children}</div>
        </main>
      </div>
    </div>
  );
}
