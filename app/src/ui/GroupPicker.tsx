import type { GroupMeta } from "../lib/ipc";
import { GROUP_COLORS } from "./GroupDialog";

export function resolveGroupName(groups: GroupMeta[], raw?: string | null): string | undefined {
  const trimmed = (raw || "").trim();
  if (!trimmed) return undefined;
  return groups.find((g) => g.name.toLowerCase() === trimmed.toLowerCase())?.name || trimmed;
}

export function appendGroupIfNew(groups: GroupMeta[], raw?: string | null): GroupMeta[] | null {
  const name = resolveGroupName(groups, raw);
  if (!name) return null;
  if (groups.some((g) => g.name.toLowerCase() === name.toLowerCase())) return null;
  return [
    ...groups,
    {
      name,
      color: GROUP_COLORS[groups.length % GROUP_COLORS.length],
      sortOrder: groups.length,
    },
  ];
}

export function GroupPicker({
  groups,
  value,
  onChange,
}: {
  groups: GroupMeta[];
  value?: string | null;
  onChange: (name: string) => void;
}) {
  const current = (value || "").trim();
  const matched = groups.find((g) => g.name.toLowerCase() === current.toLowerCase());
  const isNew = current.length > 0 && !matched;

  return (
    <div className="group-picker">
      <div className="choice-row">
        <button
          type="button"
          className={"choice" + (!current ? " on" : "")}
          onClick={() => onChange("")}
        >
          不分组
        </button>
        {groups.map((g) => (
          <button
            key={g.name}
            type="button"
            className={"choice" + (matched?.name === g.name ? " on" : "")}
            onClick={() => onChange(g.name)}
          >
            {g.color && <span className="group-tab-dot" style={{ background: g.color }} />}
            {g.name}
          </button>
        ))}
      </div>
      <input
        className="input"
        placeholder={groups.length ? "或输入新分组名称" : "输入新分组名称，例如 工作"}
        value={value || ""}
        onChange={(e) => onChange(e.target.value)}
      />
      {isNew && <div className="hint">保存时将新建分组「{current}」</div>}
    </div>
  );
}
