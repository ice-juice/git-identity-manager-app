import { useEffect, useState } from "react";
import type { BuiltinIconInfo } from "../lib/ipc";
import { customIconUrl } from "../lib/secretsUi";

const FALLBACK: BuiltinIconInfo = { id: "generic", name: "通用", color: "#6366f1", glyph: "•" };

export function IconMark({
  icon,
  builtins,
  label,
  size = 28,
}: {
  icon?: string | null;
  builtins: BuiltinIconInfo[];
  label?: string;
  size?: number;
}) {
  const [custom, setCustom] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    if (icon?.startsWith("custom:")) {
      customIconUrl(icon)
        .then((u) => {
          if (live) setCustom(u);
        })
        .catch(() => {
          if (live) setCustom(null);
        });
    } else {
      setCustom(null);
    }
    return () => {
      live = false;
    };
  }, [icon]);

  if (custom) {
    return (
      <img
        src={custom}
        alt=""
        width={size}
        height={size}
        style={{ width: size, height: size, borderRadius: 8, objectFit: "cover", flex: `0 0 ${size}px` }}
      />
    );
  }

  const id = icon?.startsWith("builtin:") ? icon.slice("builtin:".length) : "";
  const info = builtins.find((b) => b.id === id) || FALLBACK;
  const letter = (label || info.glyph || "?").slice(0, 2);
  return (
    <div
      className="icon-mark"
      style={{
        width: size,
        height: size,
        borderRadius: 8,
        background: info.color,
        color: "#fff",
        fontSize: size < 26 ? 9 : 11,
        fontWeight: 700,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        flex: `0 0 ${size}px`,
      }}
    >
      {info.id === "generic" && label ? letter : info.glyph}
    </div>
  );
}
