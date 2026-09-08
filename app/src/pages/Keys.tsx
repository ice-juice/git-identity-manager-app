import { useEffect, useState } from "react";
import { api, errMessage, type KeyRecord, type ScannedKey } from "../lib/ipc";
import { PageHead, Card, Empty, Badge } from "../ui/common";

export function Keys() {
  const [tab, setTab] = useState<"vault" | "scan">("vault");
  const [keys, setKeys] = useState<KeyRecord[]>([]);
  const [scanned, setScanned] = useState<ScannedKey[]>([]);
  const [err, setErr] = useState("");
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);
  const [genComment, setGenComment] = useState("");
  const [genName, setGenName] = useState("");
  const [showGen, setShowGen] = useState(false);

  async function loadVault() {
    try {
      setKeys(await api.listKeys());
    } catch (e) {
      setErr(errMessage(e));
    }
  }
  async function loadScan() {
    setErr("");
    try {
      setScanned(await api.scanKeys());
    } catch (e) {
      setErr(errMessage(e));
    }
  }

  useEffect(() => {
    loadVault();
  }, []);

  async function doGenerate() {
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      const k = await api.generateKey(genComment || "git-account-manager", genName || undefined);
      setMsg(`已生成密钥「${k.name}」，指纹 ${k.fingerprint}`);
      setShowGen(false);
      setGenComment("");
      setGenName("");
      await loadVault();
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function importPath(p: string) {
    setErr("");
    setMsg("");
    setBusy(true);
    try {
      const k = await api.importKeyFromPath(p);
      setMsg(`已导入「${k.name}」`);
      await loadVault();
      await loadScan();
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="stack-lg">
      <PageHead
        title="密钥管理"
        desc="库内密钥全部加密存储；可扫描系统 ~/.ssh 并导入"
        actions={
          <>
            <button className="btn primary" onClick={() => setShowGen((v) => !v)}>
              生成新密钥
            </button>
          </>
        }
      />
      {err && <div className="err-text">{err}</div>}
      {msg && <div className="callout info">{msg}</div>}

      {showGen && (
        <Card title="生成 ed25519 密钥（自动强随机 passphrase 加密入库）">
          <div className="stack">
            <div className="field">
              <label>名称（可选）</label>
              <input className="input" value={genName} onChange={(e) => setGenName(e.target.value)} />
            </div>
            <div className="field">
              <label>注释 / comment</label>
              <input
                className="input"
                placeholder="you@example.com"
                value={genComment}
                onChange={(e) => setGenComment(e.target.value)}
              />
            </div>
            <div className="row">
              <button className="btn primary" disabled={busy} onClick={doGenerate}>
                {busy ? "生成中…" : "生成并入库"}
              </button>
              <button className="btn ghost" onClick={() => setShowGen(false)}>
                取消
              </button>
            </div>
          </div>
        </Card>
      )}

      <div className="tabs">
        <button className={"tab" + (tab === "vault" ? " active" : "")} onClick={() => setTab("vault")}>
          库内密钥
        </button>
        <button
          className={"tab" + (tab === "scan" ? " active" : "")}
          onClick={() => {
            setTab("scan");
            loadScan();
          }}
        >
          系统扫描
        </button>
      </div>

      {tab === "vault" && (
        <Card>
          {keys.length === 0 ? (
            <Empty icon="🔑" text="库内还没有密钥。生成新密钥或从系统扫描导入。" />
          ) : (
            <div className="list">
              {keys.map((k) => (
                <div className="list-row" key={k.id}>
                  <div className="grow">
                    <div className="row" style={{ gap: 8 }}>
                      <strong>{k.name}</strong>
                      <Badge kind="info">{k.algorithm}</Badge>
                      {k.hasPassphrase && <Badge kind="good">已加密</Badge>}
                      {k.weak && <Badge kind="danger">弱密钥</Badge>}
                    </div>
                    <div className="mono muted sm">{k.fingerprint}</div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </Card>
      )}

      {tab === "scan" && (
        <Card actions={<button className="btn ghost sm" onClick={loadScan}>重新扫描</button>}>
          {scanned.length === 0 ? (
            <Empty icon="📁" text="未在 ~/.ssh 发现密钥，或点击「重新扫描」。" />
          ) : (
            <div className="list">
              {scanned.map((s) => (
                <div className="list-row" key={s.path}>
                  <div className="grow">
                    <div className="row" style={{ gap: 8 }}>
                      <strong className="mono sm">{s.path}</strong>
                      <Badge kind="info">{s.info.algorithm}</Badge>
                      {s.inVault && <Badge kind="good">已入库</Badge>}
                    </div>
                    <div className="mono muted sm">{s.info.fingerprint}</div>
                  </div>
                  {!s.inVault && (
                    <button className="btn sm" disabled={busy} onClick={() => importPath(s.path)}>
                      导入
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}
        </Card>
      )}
    </div>
  );
}
