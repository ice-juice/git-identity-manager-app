import type { ReactNode } from "react";

export function PageHead({ title, desc, actions }: { title: string; desc?: string; actions?: ReactNode }) {
  return (
    <div className="page-head">
      <div>
        <div className="title-lg">{title}</div>
        {desc && <div className="muted">{desc}</div>}
      </div>
      {actions && <div className="row">{actions}</div>}
    </div>
  );
}

export function Card({ title, actions, children }: { title?: string; actions?: ReactNode; children: ReactNode }) {
  return (
    <div className="card">
      {(title || actions) && (
        <div className="card-head">
          <div className="card-title">{title}</div>
          {actions && <div className="row">{actions}</div>}
        </div>
      )}
      <div className="card-body">{children}</div>
    </div>
  );
}

export function Empty({ icon = "📭", text }: { icon?: string; text: string }) {
  return (
    <div className="empty">
      <div className="empty-ic">{icon}</div>
      <div>{text}</div>
    </div>
  );
}

export function Badge({ kind = "muted", children }: { kind?: string; children: ReactNode }) {
  return <span className={"badge " + kind}>{children}</span>;
}
