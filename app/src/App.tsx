import { useEffect } from "react";
import { HashRouter, Routes, Route, Navigate, Outlet } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { api } from "./lib/ipc";
import { useApp } from "./store";
import { Layout } from "./ui/Layout";
import { InitWizard } from "./pages/Init";
import { Unlock } from "./pages/Unlock";
import { Overview } from "./pages/Overview";
import { NewIdentity } from "./pages/NewIdentity";
import { Keys } from "./pages/Keys";
import { ConfigPage } from "./pages/ConfigPage";
import { AgentPage } from "./pages/Agent";
import { Repos } from "./pages/Repos";
import { ClonePage } from "./pages/Clone";
import { SyncPage } from "./pages/Sync";
import { Settings } from "./pages/Settings";
import { CloseConfirmHost } from "./ui/CloseConfirm";

function AppShell() {
  return (
    <Layout>
      <Outlet />
    </Layout>
  );
}

export default function App() {
  const { status, loading, refresh, setWritesLock } = useApp();

  useEffect(() => {
    (async () => {
      try {
        await api.vaultTryGraceUnlock();
      } catch {
        /* 无会话或已过期，走正常解锁 */
      }
      await refresh();
    })();
  }, [refresh]);

  useEffect(() => {
    let unlistenLock: (() => void) | undefined;
    let unlistenReady: (() => void) | undefined;
    listen<{ locked: boolean; note?: string | null }>("writes-lock", (ev) => {
      setWritesLock(!!ev.payload.locked, ev.payload.note);
    })
      .then((fn) => {
        unlistenLock = fn;
      })
      .catch(() => {});
    listen("startup-ready", async () => {
      setWritesLock(false, "");
      await refresh();
    })
      .then((fn) => {
        unlistenReady = fn;
      })
      .catch(() => {});
    return () => {
      unlistenLock?.();
      unlistenReady?.();
    };
  }, [refresh, setWritesLock]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen("window-restored", async () => {
      try {
        await api.vaultTryGraceUnlock();
      } catch {
        /* 免验证未开启或已过期，走解锁页 */
      }
      await refresh();
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {
        /* 非 Tauri 环境 */
      });
    return () => {
      unlisten?.();
    };
  }, [refresh]);

  if (loading) {
    return (
      <>
        <CloseConfirmHost />
        <div className="center-stage">
          <div className="muted">加载中…</div>
        </div>
      </>
    );
  }

  if (!status?.initialized) {
    return (
      <>
        <CloseConfirmHost />
        <InitWizard />
      </>
    );
  }

  if (!status.unlocked) {
    return (
      <>
        <CloseConfirmHost />
        <Unlock />
      </>
    );
  }

  return (
    <>
      <CloseConfirmHost />
      <HashRouter>
        <Routes>
          <Route path="/identities/new" element={<NewIdentity />} />
          <Route element={<AppShell />}>
            <Route path="/" element={<Overview />} />
            <Route path="/keys" element={<Keys />} />
            <Route path="/config" element={<ConfigPage />} />
            <Route path="/agent" element={<AgentPage />} />
            <Route path="/repos" element={<Repos />} />
            <Route path="/clone" element={<ClonePage />} />
            <Route path="/sync" element={<SyncPage />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Routes>
      </HashRouter>
    </>
  );
}
