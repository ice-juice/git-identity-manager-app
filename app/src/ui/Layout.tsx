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
  Timer,
  UserRound,
} from "lucide-react";
import { api } from "../lib/ipc";
import { useApp } from "../store";
import { UpdateToast } from "./UpdateToast";

export function Layout({ children }: { children: ReactNode }) {
  const { status, lock, writesLocked, startupNote } = useApp();
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

  const NAV = [
    { to: "/", label: "身份总览", icon: LayoutDashboard, end: true, badge: idCount },
    { to: "/keys", label: "密钥管理", icon: KeyRound, badge: keyCount },
    { to: "/config", label: "SSH 配置 / 编辑", icon: FileCog },
    { to: "/agent", label: "Agent", icon: Cpu },
    { to: "/repos", label: "仓库管理", icon: FolderGit2, badge: repoCount },
    { to: "/clone", label: "克隆仓库", icon: Download },
    { to: "/totp", label: "2FA / TOTP", icon: Timer },
    { to: "/accounts", label: "隐私账号", icon: UserRound },
    { to: "/sync", label: "云端同步", icon: Cloud },
  ];

  return (
    <div className="window">
      <div className="body">
        <aside className="sidebar">
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
