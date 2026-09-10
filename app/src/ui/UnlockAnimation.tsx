import { useEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import type { UnlockAnimStyle } from "../lib/prefs";
import "./UnlockAnimation.css";

interface Props {
  style?: UnlockAnimStyle;
  onDone: () => void;
}

/** 赛博/经典模式钥匙槽位（相对 .ua-stage 宽高的百分比） */
const KEY_SLOT = [
  { x: 39.6, y: 38 },
  { x: 44.6, y: 38 },
  { x: 49.6, y: 38 },
];
const LOCK = { x: 71.8, y: 54 };

export function UnlockAnimation({ style = "cyber", onDone }: Props) {
  const [chosen] = useState(() => Math.floor(Math.random() * 3));
  const [phase, setPhase] = useState<"play" | "reveal">("play");
  const finished = useRef(false);
  const skipRef = useRef<() => void>(() => {});

  // 极简极速模式耗时更短（约 1.3s），其他模式约 2.4s
  const isMinimal = style === "minimal";
  const revealAt = isMinimal ? 1150 : 2100;
  const doneAt = isMinimal ? 1480 : 2520;
  const hardTimeout = isMinimal ? 2000 : 3200;

  useEffect(() => {
    finished.current = false;
    setPhase("play");
    const timers: ReturnType<typeof setTimeout>[] = [];
    const complete = () => {
      if (finished.current) return;
      finished.current = true;
      onDone();
    };

    timers.push(setTimeout(() => setPhase("reveal"), revealAt));
    timers.push(setTimeout(complete, doneAt));
    timers.push(setTimeout(complete, hardTimeout));

    let skipArmed = false;
    timers.push(
      setTimeout(() => {
        skipArmed = true;
      }, isMinimal ? 200 : 350),
    );

    const skip = () => {
      if (!skipArmed) return;
      setPhase("reveal");
      timers.push(setTimeout(complete, 200));
    };
    skipRef.current = skip;

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" || e.key === "Enter" || e.key === " ") skip();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      timers.forEach(clearTimeout);
      window.removeEventListener("keydown", onKey);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [style]);

  return createPortal(
    <div
      className={`ua-overlay ua-theme--${style} ${phase === "reveal" ? "ua-reveal" : ""}`}
      role="presentation"
      aria-hidden="true"
      onClick={() => skipRef.current()}
      style={{ position: "fixed", inset: 0, zIndex: 9999 }}
    >
      {style === "cyber" && (
        <CyberScene chosen={chosen} />
      )}

      {style === "classic" && (
        <ClassicScene chosen={chosen} />
      )}

      {style === "minimal" && (
        <MinimalScene />
      )}

      <div className="ua-wipe" />
      <div className="ua-streaks" />
      <div className="ua-hint">点击任意处跳过 · ESC</div>
    </div>,
    document.body,
  );
}

/** 风格一：赛博全息鉴权 */
function CyberScene({ chosen }: { chosen: number }) {
  return (
    <>
      <div className="ua-bg">
        <div className="ua-grid" />
        <div className="ua-scan" />
        <div className="ua-vignette" />
      </div>

      <div className="ua-hud-frame" />
      <div className="ua-hud-line ua-hud-line-l" />
      <div className="ua-hud-line ua-hud-line-r" />

      <div className="ua-stage">
        <div className="ua-cat-wrap">
          <svg className="ua-cat-art" viewBox="0 0 180 200" xmlns="http://www.w3.org/2000/svg">
            <defs>
              <linearGradient id="uaHolo" x1="0%" y1="0%" x2="30%" y2="100%">
                <stop offset="0%" stopColor="#67e8f9" />
                <stop offset="55%" stopColor="#818cf8" />
                <stop offset="100%" stopColor="#22d3ee" />
              </linearGradient>
            </defs>
            <g className="ua-cat">
              <path
                d="M28 78 C20 32 42 22 56 30 C70 40 78 56 86 60 C94 56 102 40 116 30 C130 22 152 32 144 78 C164 98 166 126 154 148 C140 168 114 176 86 176 C58 176 32 168 18 148 C6 126 8 98 28 78 Z"
                fill="rgba(34,211,238,0.08)"
                stroke="url(#uaHolo)"
                strokeWidth="2.4"
              />
              <path d="M42 44 C36 48 34 60 36 70 C42 62 52 50 42 44 Z" fill="none" stroke="#67e8f9" strokeWidth="1.4" />
              <path d="M130 44 C136 48 138 60 136 70 C130 62 120 50 130 44 Z" fill="none" stroke="#67e8f9" strokeWidth="1.4" />
              <ellipse cx="66" cy="100" rx="6" ry="8" fill="#22d3ee" />
              <ellipse cx="106" cy="100" rx="6" ry="8" fill="#22d3ee" />
              <path d="M82 120 Q86 125 90 120" fill="none" stroke="#a5f3fc" strokeWidth="1.5" />
              <path d="M118 152 C136 160 150 154 156 140" fill="none" stroke="url(#uaHolo)" strokeWidth="2" />
            </g>
          </svg>
          <div className="ua-cat-scan" />
        </div>

        <div className="ua-keyring" aria-hidden="true">
          <span className="ua-orbit" />
          {KEY_SLOT.map((slot, i) => {
            const isChosen = i === chosen;
            const style = {
              left: `${slot.x}%`,
              top: `${slot.y}%`,
              ["--ua-dx" as string]: String(LOCK.x - slot.x),
              ["--ua-dy" as string]: String(LOCK.y - slot.y),
            } as CSSProperties;
            return (
              <span
                key={i}
                className={"ua-key" + (isChosen ? " ua-key--chosen" : " ua-key--dim")}
                style={style}
              >
                {isChosen && <span className="ua-reticle" />}
                <svg className="ua-key-art" viewBox="0 0 28 56" xmlns="http://www.w3.org/2000/svg">
                  <defs>
                    <linearGradient id={`uaKey${i}`} x1="0%" y1="0%" x2="100%" y2="100%">
                      <stop offset="0%" stopColor="#ecfeff" />
                      <stop offset="45%" stopColor="#22d3ee" />
                      <stop offset="100%" stopColor="#6366f1" />
                    </linearGradient>
                  </defs>
                  <circle cx="14" cy="10" r="8.5" fill="none" stroke={`url(#uaKey${i})`} strokeWidth="2.4" />
                  <circle cx="14" cy="10" r="3.2" fill="none" stroke="#67e8f9" strokeWidth="1.4" />
                  <rect x="11.4" y="17" width="5.2" height="28" rx="1.6" fill={`url(#uaKey${i})`} />
                  <rect x="16.4" y="36" width="8" height="3.6" rx="1" fill={`url(#uaKey${i})`} />
                  <rect x="16.4" y="29" width="6.2" height="3.6" rx="1" fill={`url(#uaKey${i})`} />
                </svg>
              </span>
            );
          })}
        </div>

        <div className="ua-door">
          <div className="ua-vault-ring" />
          <div className="ua-vault-ring ua-vault-ring-2" />
          <div className="ua-jamb">
            <div className="ua-interior" />
            <div className="ua-leaf">
              <span className="ua-pane ua-pane-t" />
              <span className="ua-pane ua-pane-b" />
              <span className="ua-lock-glow" />
              <span className="ua-keyhole" />
              <span className="ua-knob" />
            </div>
          </div>
        </div>
      </div>

      <div className="ua-status">
        <span className="ua-status-k">SYS</span>
        <span className="ua-status-t">AUTHENTICATING</span>
        <span className="ua-status-ok">ACCESS GRANTED</span>
      </div>
    </>
  );
}

/** 风格二：经典温情金匙 */
function ClassicScene({ chosen }: { chosen: number }) {
  return (
    <>
      <div className="ua-bg">
        <div className="ua-warm-glow" />
      </div>

      <div className="ua-stage ua-stage--classic">
        <svg className="ua-cat-art ua-cat-art--warm" viewBox="0 0 180 200" xmlns="http://www.w3.org/2000/svg">
          <defs>
            <linearGradient id="uaCatWarm" x1="0%" y1="0%" x2="0%" y2="100%">
              <stop offset="0%" stopColor="#c7d2fe" />
              <stop offset="50%" stopColor="#6366f1" />
              <stop offset="100%" stopColor="#4338ca" />
            </linearGradient>
          </defs>
          <g className="ua-cat">
            <path
              d="M28 78 C20 32 42 22 56 30 C70 40 78 56 86 60 C94 56 102 40 116 30 C130 22 152 32 144 78 C164 98 166 126 154 148 C140 168 114 176 86 176 C58 176 32 168 18 148 C6 126 8 98 28 78 Z"
              fill="url(#uaCatWarm)"
              stroke="#e0e7ff"
              strokeWidth="2.5"
            />
            <path d="M42 44 C36 48 34 60 36 70 C42 62 52 50 42 44 Z" fill="#f472b6" opacity="0.9" />
            <path d="M130 44 C136 48 138 60 136 70 C130 62 120 50 130 44 Z" fill="#f472b6" opacity="0.9" />
            <ellipse cx="66" cy="100" rx="6" ry="8" fill="#38bdf8" />
            <circle cx="64" cy="97" r="2.2" fill="#fff" />
            <ellipse cx="106" cy="100" rx="6" ry="8" fill="#38bdf8" />
            <circle cx="104" cy="97" r="2.2" fill="#fff" />
            <polygon points="86,118 83,123 89,123" fill="#f472b6" />
            <path d="M82 126 Q86 130 90 126" fill="none" stroke="#c7d2fe" strokeWidth="1.5" strokeLinecap="round" />
            <path d="M118 152 C136 160 150 154 156 140 C150 172 132 184 116 174 Z" fill="url(#uaCatWarm)" stroke="#e0e7ff" strokeWidth="2" />
          </g>
        </svg>

        <div className="ua-keyring" aria-hidden="true">
          <span className="ua-orbit ua-orbit--gold" />
          {KEY_SLOT.map((slot, i) => {
            const isChosen = i === chosen;
            const style = {
              left: `${slot.x}%`,
              top: `${slot.y}%`,
              ["--ua-dx" as string]: String(LOCK.x - slot.x),
              ["--ua-dy" as string]: String(LOCK.y - slot.y),
            } as CSSProperties;
            return (
              <span
                key={i}
                className={"ua-key" + (isChosen ? " ua-key--chosen ua-key--gold" : " ua-key--dim")}
                style={style}
              >
                {isChosen && <span className="ua-key-glow--gold" />}
                <svg className="ua-key-art" viewBox="0 0 28 56" xmlns="http://www.w3.org/2000/svg">
                  <defs>
                    <linearGradient id={`uaGoldKey${i}`} x1="0%" y1="0%" x2="100%" y2="100%">
                      <stop offset="0%" stopColor="#fffbeb" />
                      <stop offset="35%" stopColor="#fde047" />
                      <stop offset="75%" stopColor="#f59e0b" />
                      <stop offset="100%" stopColor="#b45309" />
                    </linearGradient>
                  </defs>
                  <circle cx="14" cy="10" r="9" fill={`url(#uaGoldKey${i})`} />
                  <circle cx="14" cy="10" r="4" fill="#1e1b4b" />
                  <rect x="10.5" y="17" width="7" height="30" rx="3" fill={`url(#uaGoldKey${i})`} />
                  <rect x="17.5" y="38" width="9" height="5" rx="2" fill={`url(#uaGoldKey${i})`} />
                  <rect x="17.5" y="28" width="7" height="5" rx="2" fill={`url(#uaGoldKey${i})`} />
                </svg>
              </span>
            );
          })}
        </div>

        <div className="ua-door ua-door--classic">
          <div className="ua-jamb ua-jamb--classic">
            <div className="ua-interior ua-interior--warm" />
            <div className="ua-leaf ua-leaf--classic">
              <span className="ua-pane ua-pane-t" />
              <span className="ua-pane ua-pane-b" />
              <span className="ua-lock-glow ua-lock-glow--gold" />
              <span className="ua-keyhole ua-keyhole--gold" />
              <span className="ua-knob ua-knob--gold" />
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

/** 风格三：极客量子极速流 */
function MinimalScene() {
  const codeLines = [
    "0x7F4B9A10  INIT_VAULT_SESSION",
    "DECRYPTING  AES-256-GCM / ARGON2ID",
    "VERIFYING   SSH_KEY_CREDENTIALS",
    "IDENTITY    MATCHED [MASTER_KEY]",
    "STATUS      200 OK -> GRANTED",
  ];

  return (
    <div className="ua-minimal-wrap">
      <div className="ua-minimal-matrix">
        {codeLines.map((line, idx) => (
          <div key={idx} className="ua-matrix-line" style={{ animationDelay: `${idx * 0.08}s` }}>
            {line}
          </div>
        ))}
      </div>

      <div className="ua-minimal-core">
        <div className="ua-core-ring ua-core-ring-1" />
        <div className="ua-core-ring ua-core-ring-2" />
        <div className="ua-core-ring ua-core-ring-3" />
        <div className="ua-quantum-key">
          <svg viewBox="0 0 36 36" width="36" height="36">
            <circle cx="18" cy="18" r="8" fill="none" stroke="#22d3ee" strokeWidth="2.5" />
            <path d="M18 10 L18 2" stroke="#22d3ee" strokeWidth="2.5" strokeLinecap="round" />
            <path d="M18 26 L18 34" stroke="#22d3ee" strokeWidth="2.5" strokeLinecap="round" />
            <path d="M10 18 L2 18" stroke="#22d3ee" strokeWidth="2.5" strokeLinecap="round" />
            <path d="M26 18 L34 18" stroke="#22d3ee" strokeWidth="2.5" strokeLinecap="round" />
          </svg>
        </div>
      </div>

      <div className="ua-minimal-status">
        <span className="ua-minimal-badge">KEYMASTER</span>
        <span className="ua-minimal-title">VAULT UNLOCKED</span>
      </div>
    </div>
  );
}

export default UnlockAnimation;
