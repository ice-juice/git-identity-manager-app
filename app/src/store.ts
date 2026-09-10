import { create } from "zustand";
import { api, type VaultStatus } from "./lib/ipc";
import { type ThemeMode, getSavedTheme, applyTheme } from "./lib/theme";
import {
  getUnlockAnimEnabled,
  setUnlockAnimEnabledStored,
  getUnlockAnimStyle,
  setUnlockAnimStyleStored,
  type UnlockAnimStyle,
} from "./lib/prefs";

interface AppStore {
  status: VaultStatus | null;
  loading: boolean;
  theme: ThemeMode;
  writesLocked: boolean;
  startupNote: string;
  unlockAnimEnabled: boolean;
  unlockAnimStyle: UnlockAnimStyle;
  playUnlockAnim: boolean;
  animPreviewStyle?: UnlockAnimStyle;
  animPlayId: number;
  setTheme: (t: ThemeMode) => void;
  toggleTheme: () => void;
  setWritesLock: (locked: boolean, note?: string | null) => void;
  setUnlockAnimEnabled: (on: boolean) => void;
  setUnlockAnimStyle: (style: UnlockAnimStyle) => void;
  startUnlockAnim: (style?: UnlockAnimStyle) => void;
  endUnlockAnim: () => void;
  refresh: () => Promise<void>;
  lock: () => Promise<void>;
}

const initialTheme = getSavedTheme();
applyTheme(initialTheme);

export const useApp = create<AppStore>((set, get) => ({
  status: null,
  loading: true,
  theme: initialTheme,
  writesLocked: false,
  startupNote: "",
  unlockAnimEnabled: getUnlockAnimEnabled(),
  unlockAnimStyle: getUnlockAnimStyle(),
  playUnlockAnim: false,
  animPreviewStyle: undefined,
  animPlayId: 0,
  setWritesLock: (locked, note) => {
    set({ writesLocked: locked, startupNote: note ?? "" });
  },
  setUnlockAnimEnabled: (on: boolean) => {
    setUnlockAnimEnabledStored(on);
    set({ unlockAnimEnabled: on });
  },
  setUnlockAnimStyle: (style: UnlockAnimStyle) => {
    setUnlockAnimStyleStored(style);
    set({ unlockAnimStyle: style });
  },
  startUnlockAnim: (style?: UnlockAnimStyle) =>
    set((s) => ({
      playUnlockAnim: true,
      animPreviewStyle: style,
      animPlayId: s.animPlayId + 1,
    })),
  endUnlockAnim: () => set({ playUnlockAnim: false, animPreviewStyle: undefined }),
  setTheme: (t: ThemeMode) => {
    applyTheme(t);
    set({ theme: t });
  },
  toggleTheme: () => {
    const cur = get().theme;
    const next: ThemeMode = cur === "light" ? "dark" : cur === "dark" ? "navy" : "light";
    applyTheme(next);
    set({ theme: next });
  },
  refresh: async () => {
    try {
      const status = await api.vaultStatus();
      set({
        status,
        loading: false,
        writesLocked: !!status.writesLocked,
        startupNote: status.startupNote ?? "",
      });
    } catch {
      set({ loading: false });
    }
  },
  lock: async () => {
    await api.vaultLock();
    const status = await api.vaultStatus();
    set({ status, writesLocked: false, startupNote: "", playUnlockAnim: false });
  },
}));
