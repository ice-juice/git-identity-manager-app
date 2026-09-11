import { useEffect, useState } from "react";

/**
 * 平台与形态判定。
 *
 * 刻意把「是不是手机」和「要不要用紧凑布局」分成两件事：
 * - `isMobilePlatform()` 决定**能力**（能不能开 ssh-agent、要不要显示窗口按钮）；
 * - `useIsCompact()` 决定**布局**（底部 Tab 还是左侧栏）。
 *
 * 这样在 Windows 上把 Vite 开发服务器窗口拉窄就能验收移动端布局，
 * 不必先装好 Android 模拟器。桌面窗口有 840px 最小宽度（tauri.conf.json），
 * 打包后的桌面端不会误触紧凑布局。
 */

const COMPACT_QUERY = "(max-width: 640px)";

function ua(): string {
  return typeof navigator === "undefined" ? "" : navigator.userAgent;
}

export function isAndroid(): boolean {
  return /Android/i.test(ua());
}

export function isIOS(): boolean {
  // iPadOS 13+ 的 Safari UA 里不再有 iPad，退化成 Macintosh + 触摸点。
  const s = ua();
  if (/iPhone|iPad|iPod/i.test(s)) return true;
  return /Macintosh/i.test(s) && typeof navigator !== "undefined" && navigator.maxTouchPoints > 1;
}

export function isMobilePlatform(): boolean {
  return isAndroid() || isIOS();
}

/** 当前视口是否为紧凑（手机竖屏）形态。手机平台恒为真。 */
export function useIsCompact(): boolean {
  const [compact, setCompact] = useState(() => {
    if (isMobilePlatform()) return true;
    if (typeof window === "undefined" || !window.matchMedia) return false;
    return window.matchMedia(COMPACT_QUERY).matches;
  });

  useEffect(() => {
    if (isMobilePlatform()) {
      setCompact(true);
      return;
    }
    if (typeof window === "undefined" || !window.matchMedia) return;
    const mq = window.matchMedia(COMPACT_QUERY);
    const onChange = (e: MediaQueryListEvent) => setCompact(e.matches);
    mq.addEventListener("change", onChange);
    setCompact(mq.matches);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  return compact;
}

/**
 * 移动端不提供的能力。用于隐藏入口，与后端 `AppError::Unsupported`
 * （code `UNSUPPORTED_PLATFORM`）互为里外两道防线。
 */
export function supportsLocalGitTools(): boolean {
  return !isMobilePlatform();
}

/** 是否自绘窗口控制按钮（仅桌面；移动端由系统管理窗口）。 */
export function supportsWindowControls(): boolean {
  return !isMobilePlatform();
}
