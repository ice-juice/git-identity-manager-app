import { useEffect, type ReactNode } from "react";
import { HashRouter, Routes, Route, Navigate, Outlet } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { api } from "./lib/ipc";
import { useApp } from "./store";
import { Layout } from "./ui/Layout";
import { TitleBar } from "./ui/TitleBar";
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
import { TotpPage } from "./pages/Totp";
import { AccountsPage } from "./pages/Accounts";
import { CloseConfirmHost } from "./ui/CloseConfirm";
import UnlockAnimation from "./ui/UnlockAnimation";

function AppShell() {
  return (
    <Layout>
      <Outlet />
    </Layout>
  );
}

export default function App() {
  const {
    status,
    loading,
    refresh,
    setWritesLock,
    playUnlockAnim,
    animPreviewStyle,
    unlockAnimStyle,
    animPlayId,
    endUnlockAnim,
  } = useApp();

  const unlockOverlay = playUnlockAnim ? (
    <UnlockAnimation
      key={animPlayId}
      style={animPreviewStyle || unlockAnimStyle}
      onDone={endUnlockAnim}
    />
  ) : null;

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

  let screen: ReactNode;
  if (loading) {
    screen = (
      <div className="center-stage">
        <div className="muted">加载中…</div>
      </div>
    );
  } else if (!status?.initialized) {
    screen = <InitWizard />;
  } else if (!status.unlocked) {
    screen = <Unlock />;
  } else {
    screen = (
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
            <Route path="/totp" element={<TotpPage />} />
            <Route path="/accounts" element={<AccountsPage />} />
            <Route path="/sync" element={<SyncPage />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Routes>
      </HashRouter>
    );
  }

  return (
    <>
      <CloseConfirmHost />
      <div className="app-shell">
        <TitleBar />
        <div className="app-view">{screen}</div>
      </div>
      {unlockOverlay}
    </>
  );
}
