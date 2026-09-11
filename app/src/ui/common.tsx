import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
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

/** 设置等页面的错误提示：居中弹窗，避免顶部一行红字被忽略。 */
export function ErrorDialog({
  title = "操作未能完成",
  message,
  onClose,
}: {
  title?: string;
  message: string;
  onClose: () => void;
}) {
  useEffect(() => {
    if (!message) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [message, onClose]);

  if (!message) return null;
  return createPortal(
    <div
      className="close-overlay"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="error-dialog-title"
      onClick={onClose}
    >
      <div className="close-dialog settings-error-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="close-dialog-titlebar">
          <span id="error-dialog-title">{title}</span>
          <button type="button" className="close-dialog-x" onClick={onClose} aria-label="关闭">
            ×
          </button>
        </div>
        <div className="close-dialog-body">
          <div className="close-dialog-content">
            <div className="settings-error-text">{message}</div>
          </div>
        </div>
        <div className="close-dialog-footer settings-error-actions">
          <button type="button" className="btn primary sm" onClick={onClose} autoFocus>
            知道了
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
