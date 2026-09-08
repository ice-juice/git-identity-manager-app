import { useEffect, useState } from "react";
import { api, errMessage, type Identity, type KeyRecord } from "../lib/ipc";
import { PageHead, Card, Empty, Badge } from "../ui/common";

export function Overview() {
  const [identities, setIdentities] = useState<Identity[]>([]);
  const [keys, setKeys] = useState<KeyRecord[]>([]);
  const [err, setErr] = useState("");

  useEffect(() => {
    (async () => {
      try {
        setIdentities(await api.listIdentities());
        setKeys(await api.listKeys());
      } catch (e) {
        setErr(errMessage(e));
      }
    })();
  }, []);

  const keyName = (id: string | null) => keys.find((k) => k.id === id)?.name ?? "未绑定";

  return (
    <div className="stack-lg">
      <PageHead title="身份总览" desc="管理多个 Git 平台账号的密钥与提交身份" />
      {err && <div className="err-text">{err}</div>}

      <div className="stat-grid">
        <div className="stat">
          <div className="stat-n">{identities.length}</div>
          <div className="stat-l">身份</div>
        </div>
        <div className="stat">
          <div className="stat-n">{keys.length}</div>
          <div className="stat-l">密钥</div>
        </div>
        <div className="stat">
          <div className="stat-n">{keys.filter((k) => k.weak).length}</div>
          <div className="stat-l">弱密钥</div>
        </div>
      </div>

      <Card title="身份列表">
        {identities.length === 0 ? (
          <Empty icon="🧑‍💻" text="还没有身份。到「密钥管理」导入或新建密钥后创建身份。" />
        ) : (
          <div className="list">
            {identities.map((id) => (
              <div className="list-row" key={id.id}>
                <div className="grow">
                  <div className="row" style={{ gap: 8 }}>
                    <strong>{id.name}</strong>
                    <Badge kind="info">{id.platform}</Badge>
                    {id.strictMode && <Badge kind="warn">严格模式</Badge>}
                  </div>
                  <div className="mono muted sm">
                    {id.user}@{id.hostAlias} → {id.realHost} · 密钥 {keyName(id.keyId)}
                  </div>
                  {id.owners.length > 0 && (
                    <div className="row" style={{ gap: 6, marginTop: 6 }}>
                      {id.owners.map((o) => (
                        <Badge key={o}>{o}</Badge>
                      ))}
                    </div>
                  )}
                </div>
                <div className="muted sm">{id.email ?? "无提交邮箱"}</div>
              </div>
            ))}
          </div>
        )}
      </Card>
    </div>
  );
}
