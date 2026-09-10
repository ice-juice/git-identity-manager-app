import { useEffect, useRef, useState, type CSSProperties } from "react";
import { prefersReducedMotion } from "../lib/prefs";
import "./UnlockAnimation.css";

interface Props {
  /** 动画结束（或被跳过）时调用，用于卸载 overlay、露出主界面。 */
  onDone: () => void;
}

// 三把钥匙悬挂点的横坐标（SVG 局部坐标，viewBox 0 0 400 260）
const KEY_SLOT_X = [132, 156, 180];
const KEY_SLOT_Y = 116;
// 门锁孔坐标
const LOCK_X = 326;
const LOCK_Y = 152;

// 时间轴（毫秒）
const FADE_AT = 1950; // 开始整层淡出
const DONE_AT = 2380; // 通知父级卸载
const HARD_TIMEOUT = 2800; // 兜底：任何异常都不阻塞进入主界面

/**
 * 解锁开门过场动画：吉祥猫从三把钥匙里随机抽一把，飞向门锁孔旋转开门，
 * 门开后整层淡出，露出背后已挂载的主界面（揭幕式）。
 *
 * 性能：无第三方库，纯内联 SVG + CSS 关键帧（仅 transform/opacity/filter），
 * 结束即卸载；支持点击/Esc 跳过与 prefers-reduced-motion。
 */
export function UnlockAnimation({ onDone }: Props) {
  // 一次性随机抽签（惰性初始化，避免重渲染改变结果）
  const [chosen] = useState(() => Math.floor(Math.random() * 3));
  const [done, setDone] = useState(false);
  const finished = useRef(false);
  const skipRef = useRef<() => void>(() => {});

  useEffect(() => {
    // 系统减少动效：直接结束
    if (prefersReducedMotion()) {
      onDone();
      return;
    }

    const timers: ReturnType<typeof setTimeout>[] = [];
    const complete = () => {
      if (finished.current) return;
      finished.current = true;
      onDone();
    };

    timers.push(setTimeout(() => setDone(true), FADE_AT));
    timers.push(setTimeout(complete, DONE_AT));
    timers.push(setTimeout(complete, HARD_TIMEOUT));

    const skip = () => {
      setDone(true);
      // 让淡出有一帧过渡再卸载
      timers.push(setTimeout(complete, 320));
    };
    skipRef.current = skip;

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" || e.key === "Enter") skip();
    };
    window.addEventListener("keydown", onKey);

    return () => {
      timers.forEach(clearTimeout);
      window.removeEventListener("keydown", onKey);
    };
    // onDone 为 zustand 稳定引用，仅需在挂载时启动一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const slotX = KEY_SLOT_X[chosen];
  const chosenStyle = {
    // 从抽中钥匙的悬挂点飞到锁孔的位移
    ["--ua-tx" as string]: `${LOCK_X - slotX}px`,
    ["--ua-ty" as string]: `${LOCK_Y - KEY_SLOT_Y}px`,
  } as CSSProperties;

  return (
    <div
      className={"ua-overlay" + (done ? " ua-done" : "")}
      role="presentation"
      aria-hidden="true"
      onClick={() => skipRef.current()}
    >
      <svg className="ua-scene" viewBox="0 0 400 260" xmlns="http://www.w3.org/2000/svg">
        <defs>
          <linearGradient id="uaGold" x1="0%" y1="0%" x2="100%" y2="100%">
            <stop offset="0%" stopColor="#fffbeb" />
            <stop offset="35%" stopColor="#fde047" />
            <stop offset="75%" stopColor="#f59e0b" />
            <stop offset="100%" stopColor="#b45309" />
          </linearGradient>
          <linearGradient id="uaCat" x1="0%" y1="0%" x2="0%" y2="100%">
            <stop offset="0%" stopColor="#a5b4fc" />
            <stop offset="55%" stopColor="#6366f1" />
            <stop offset="100%" stopColor="#4338ca" />
          </linearGradient>
          <linearGradient id="uaDoor" x1="0%" y1="0%" x2="100%" y2="0%">
            <stop offset="0%" stopColor="#312e81" />
            <stop offset="100%" stopColor="#1e1b4b" />
          </linearGradient>
          <radialGradient id="uaLight" cx="50%" cy="50%" r="50%">
            <stop offset="0%" stopColor="#fef9c3" stopOpacity="0.95" />
            <stop offset="55%" stopColor="#fde047" stopOpacity="0.55" />
            <stop offset="100%" stopColor="#f59e0b" stopOpacity="0" />
          </radialGradient>
          <radialGradient id="uaKeyGlow" cx="50%" cy="50%" r="50%">
            <stop offset="0%" stopColor="#fde047" stopOpacity="0.9" />
            <stop offset="100%" stopColor="#fde047" stopOpacity="0" />
          </radialGradient>
        </defs>

        {/* ==== 门（右侧） ==== */}
        <g>
          {/* 门框 + 门内光 */}
          <rect x="276" y="40" width="104" height="188" rx="12" fill="#0b1024" stroke="#3730a3" strokeWidth="2" />
          <ellipse className="ua-doorlight" cx="322" cy="134" rx="52" ry="96" fill="url(#uaLight)" />
          {/* 门扇（以右侧为轴开启） */}
          <g className="ua-door-panel">
            <rect x="282" y="46" width="92" height="176" rx="9" fill="url(#uaDoor)" stroke="#4f46e5" strokeWidth="2" />
            <rect x="292" y="58" width="72" height="70" rx="6" fill="none" stroke="#6366f1" strokeWidth="1.5" opacity="0.6" />
            <rect x="292" y="140" width="72" height="70" rx="6" fill="none" stroke="#6366f1" strokeWidth="1.5" opacity="0.6" />
            {/* 锁孔 + 点亮 */}
            <circle className="ua-lock-glow" cx={LOCK_X} cy={LOCK_Y} r="16" fill="url(#uaKeyGlow)" />
            <circle cx={LOCK_X} cy={LOCK_Y} r="9" fill="#0b1024" stroke="#fde047" strokeWidth="1.5" />
            <rect x={LOCK_X - 2} y={LOCK_Y} width="4" height="12" rx="2" fill="#0b1024" stroke="#fde047" strokeWidth="1" />
            {/* 门把手 */}
            <circle cx="296" cy="140" r="4" fill="#fde047" />
          </g>
        </g>

        {/* ==== 吉祥猫（左侧，握着钥匙环） ==== */}
        <g className="ua-cat">
          {/* 头 */}
          <path
            d="M40 96 C30 52 50 42 62 48 C76 56 84 72 92 76 C100 72 108 56 122 48 C134 42 154 52 144 96 C162 116 164 142 154 162 C142 180 118 188 92 188 C66 188 42 180 30 162 C20 142 22 116 40 96 Z"
            fill="url(#uaCat)"
            stroke="#e0e7ff"
            strokeWidth="2.5"
          />
          {/* 内耳 */}
          <path d="M54 62 C49 66 46 76 48 85 C53 79 61 67 54 62 Z" fill="#f472b6" opacity="0.85" />
          <path d="M130 62 C135 66 138 76 136 85 C131 79 123 67 130 62 Z" fill="#f472b6" opacity="0.85" />
          {/* 眼睛 */}
          <ellipse cx="72" cy="118" rx="6" ry="8" fill="#38bdf8" />
          <circle cx="70" cy="115" r="2.2" fill="#fff" />
          <ellipse cx="112" cy="118" rx="6" ry="8" fill="#38bdf8" />
          <circle cx="110" cy="115" r="2.2" fill="#fff" />
          {/* 鼻 + 嘴 */}
          <polygon points="92,128 89,132 95,132" fill="#f472b6" />
          <path d="M89 134 Q92 137 95 134" fill="none" stroke="#c7d2fe" strokeWidth="1.4" strokeLinecap="round" />
          {/* 触手抓环 */}
          <path d="M120 168 C136 176 150 172 156 160 C150 178 134 190 120 182 Z" fill="url(#uaCat)" stroke="#e0e7ff" strokeWidth="2" />
        </g>

        {/* ==== 钥匙环 + 三把钥匙 ==== */}
        <g>
          <circle cx="156" cy="112" r="16" fill="none" stroke="url(#uaGold)" strokeWidth="5" />
          {[0, 1, 2].map((i) => {
            const isChosen = i === chosen;
            const x = KEY_SLOT_X[i];
            return (
              <g
                key={i}
                className={isChosen ? "ua-key--chosen" : "ua-key--dim"}
                style={isChosen ? chosenStyle : undefined}
                transform={`translate(${x - 156}, 0)`}
              >
                {isChosen && (
                  <circle className="ua-glow" cx="156" cy={KEY_SLOT_Y + 18} r="26" fill="url(#uaKeyGlow)" />
                )}
                {/* 钥匙：环-柄-齿 */}
                <circle cx="156" cy={KEY_SLOT_Y} r="9" fill="url(#uaGold)" />
                <circle cx="156" cy={KEY_SLOT_Y} r="4" fill="#1e1b4b" />
                <rect x="152.5" y={KEY_SLOT_Y + 8} width="7" height="40" rx="3" fill="url(#uaGold)" />
                <rect x="159" y={KEY_SLOT_Y + 40} width="9" height="5" rx="2" fill="url(#uaGold)" />
                <rect x="159" y={KEY_SLOT_Y + 30} width="7" height="5" rx="2" fill="url(#uaGold)" />
              </g>
            );
          })}
        </g>
      </svg>

      <div className="ua-hint">点击任意处跳过</div>
    </div>
  );
}

export default UnlockAnimation;
