import { useEffect } from "react";
import { HashRouter, Routes, Route, Navigate } from "react-router-dom";
import { useApp } from "./store";
import { Layout } from "./ui/Layout";
import { InitWizard } from "./pages/Init";
import { Unlock } from "./pages/Unlock";
import { Overview } from "./pages/Overview";
import { Keys } from "./pages/Keys";
import { ConfigPage } from "./pages/ConfigPage";
import { AgentPage } from "./pages/Agent";
import { Repos } from "./pages/Repos";
import { Settings } from "./pages/Settings";

export default function App() {
  const { status, loading, refresh } = useApp();

  useEffect(() => {
    refresh();
  }, [refresh]);

  if (loading) {
    return (
      <div className="center-stage">
        <div style={{ color: "#cbd5e1" }}>加载中…</div>
      </div>
    );
  }

  if (!status?.initialized) {
    return <InitWizard />;
  }

  if (!status.unlocked) {
    return <Unlock />;
  }

  return (
    <HashRouter>
      <Layout>
        <Routes>
          <Route path="/" element={<Overview />} />
          <Route path="/keys" element={<Keys />} />
          <Route path="/config" element={<ConfigPage />} />
          <Route path="/agent" element={<AgentPage />} />
          <Route path="/repos" element={<Repos />} />
          <Route path="/settings" element={<Settings />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </Layout>
    </HashRouter>
  );
}
