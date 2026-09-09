import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { CircleHelp } from "lucide-react";

function placeTooltip(icon: HTMLElement, pop: HTMLElement) {
  const r = icon.getBoundingClientRect();
  const pw = pop.offsetWidth;
  const ph = pop.offsetHeight;
  const gap = 8;
  const pad = 10;
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  let left = r.left;
  let top = r.bottom + gap;

  if (left + pw > vw - pad) left = r.right - pw;
  if (left < pad) left = pad;
  if (left + pw > vw - pad) left = Math.max(pad, vw - pad - pw);

  if (top + ph > vh - pad) top = r.top - gap - ph;
  if (top < pad) top = pad;
  if (top + ph > vh - pad) top = Math.max(pad, vh - pad - ph);

  pop.style.top = `${Math.round(top)}px`;
  pop.style.left = `${Math.round(left)}px`;
}

export function FieldLabel({ name, tip }: { name: string; tip: string }) {
  const [open, setOpen] = useState(false);
  const iconRef = useRef<HTMLSpanElement>(null);
  const popRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!open) return;
    const icon = iconRef.current;
    const pop = popRef.current;
    if (!icon || !pop) return;
    placeTooltip(icon, pop);
    pop.dataset.visible = "1";

    const onMove = () => {
      if (iconRef.current && popRef.current) placeTooltip(iconRef.current, popRef.current);
    };
    window.addEventListener("resize", onMove);
    window.addEventListener("scroll", onMove, true);
    return () => {
      window.removeEventListener("resize", onMove);
      window.removeEventListener("scroll", onMove, true);
    };
  }, [open, tip]);

  return (
    <label className="field-label">
      <span>{name}</span>
      <span
        ref={iconRef}
        className="field-help"
        tabIndex={0}
        aria-label={tip}
        onMouseEnter={() => setOpen(true)}
        onMouseLeave={() => setOpen(false)}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
      >
        <CircleHelp size={14} strokeWidth={2} />
      </span>
      {open &&
        createPortal(
          <div ref={popRef} className="field-help-pop" role="tooltip">
            {tip}
          </div>,
          document.body,
        )}
    </label>
  );
}

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
