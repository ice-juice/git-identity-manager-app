import { create } from "zustand";
import { api, type VaultStatus } from "./lib/ipc";
import { type ThemeMode, getSavedTheme, applyTheme } from "./lib/theme";
import { getUnlockAnimEnabled, setUnlockAnimEnabledStored } from "./lib/prefs";

interface AppStore {
  status: VaultStatus | null;
  loading: boolean;
  theme: ThemeMode;
  writesLocked: boolean;
  startupNote: string;
  unlockAnimEnabled: boolean;
  playUnlockAnim: boolean;
  setTheme: (t: ThemeMode) => void;
  toggleTheme: () => void;
  setWritesLock: (locked: boolean, note?: string | null) => void;
  setUnlockAnimEnabled: (on: boolean) => void;
  startUnlockAnim: () => void;
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
  playUnlockAnim: false,
  setWritesLock: (locked, note) => {
    set({ writesLocked: locked, startupNote: note ?? "" });
  },
  setUnlockAnimEnabled: (on: boolean) => {
    setUnlockAnimEnabledStored(on);
    set({ unlockAnimEnabled: on });
  },
  startUnlockAnim: () => set({ playUnlockAnim: true }),
  endUnlockAnim: () => set({ playUnlockAnim: false }),
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
