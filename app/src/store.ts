import { create } from "zustand";
import { api, type VaultStatus } from "./lib/ipc";

interface AppStore {
  status: VaultStatus | null;
  loading: boolean;
  refresh: () => Promise<void>;
  lock: () => Promise<void>;
}

export const useApp = create<AppStore>((set) => ({
  status: null,
  loading: true,
  refresh: async () => {
    try {
      const status = await api.vaultStatus();
      set({ status, loading: false });
    } catch {
      set({ loading: false });
    }
  },
  lock: async () => {
    await api.vaultLock();
    const status = await api.vaultStatus();
    set({ status });
  },
}));
