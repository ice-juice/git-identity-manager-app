// 类型化 IPC 封装：对 Rust 命令的薄包装 + 类型定义。
import { invoke } from "@tauri-apps/api/core";

export interface AppErrorShape {
  code: string;
  message: string;
}

// ---- 类型 ----
export interface VaultStatus {
  initialized: boolean;
  unlocked: boolean;
  workspacePath: string | null;
  workspaceId: string | null;
  autoLockMinutes: number;
}
export interface InitResult {
  recoveryKey: string;
  workspaceId: string;
}
export interface PathCheck {
  warning: string | null;
}
export interface KeyInfo {
  algorithm: string;
  fingerprint: string;
  publicOpenssh: string;
  bits: number | null;
  encrypted: boolean | null;
  comment: string;
}
export interface KeyRecord {
  id: string;
  name: string;
  algorithm: string;
  fingerprint: string;
  publicOpenssh: string;
  bits: number | null;
  hasPassphrase: boolean;
  weak: boolean;
  sourcePath: string | null;
  importedAt: string;
}
export interface Identity {
  id: string;
  name: string;
  platform: string;
  hostAlias: string;
  realHost: string;
  user: string;
  email: string | null;
  gitUserName: string | null;
  keyId: string | null;
  owners: string[];
  strictMode: boolean;
}
export interface HostBlock {
  patterns: string[];
  options: [string, string][];
  startLine: number;
}
export interface Diagnostic {
  severity: "warn" | "error";
  code: string;
  message: string;
  host: string | null;
}
export interface ConfigView {
  path: string;
  raw: string;
  blocks: HostBlock[];
  diagnostics: Diagnostic[];
}
export interface ScannedKey {
  path: string;
  info: KeyInfo;
  inVault: boolean;
}
export interface SshBinary {
  path: string;
  version: string | null;
  source: string;
}
export interface Toolchain {
  system: SshBinary | null;
  git: SshBinary | null;
  gitUses: string | null;
}
export interface AgentKey {
  bits: number | null;
  fingerprint: string;
  comment: string;
  algo: string;
}
export interface AgentKeyResolved {
  agent: AgentKey;
  identityName: string | null;
  keyName: string | null;
}
export interface AgentStatus {
  running: boolean;
  usingFallback: boolean;
  keys: AgentKeyResolved[];
}
export interface Candidate {
  identityId: string;
  identityName: string;
  hostAlias: string;
  confidence: "certain" | "veryHigh" | "mediumHigh" | "low";
  basis: string;
}
export interface Inference {
  rewrittenUrl: string | null;
  recommended: Candidate | null;
  candidates: Candidate[];
  needsProbe: boolean;
}
export interface RepoInfo {
  path: string;
  remoteUrl: string | null;
  currentAlias: string | null;
  gitUserName: string | null;
  gitUserEmail: string | null;
  inferredIdentity: string | null;
  needsAliasFix: boolean;
  fixCommand: string | null;
}
export interface ConfigPreview {
  diff: string;
  newText: string;
}
export interface ManagedEntry {
  alias: string;
  hostName: string;
  user: string;
  identityFile: string;
  identitiesOnly: boolean;
}

// ---- 命令 ----
export const api = {
  // vault
  vaultStatus: () => invoke<VaultStatus>("vault_status"),
  checkWorkspacePath: (path: string) => invoke<PathCheck>("check_workspace_path", { path }),
  vaultInit: (path: string, password: string) => invoke<InitResult>("vault_init", { path, password }),
  vaultUnlock: (password: string) => invoke<void>("vault_unlock", { password }),
  vaultUnlockRecovery: (recoveryKey: string) => invoke<void>("vault_unlock_recovery", { recoveryKey }),
  vaultLock: () => invoke<void>("vault_lock"),
  changePassword: (oldPassword: string, newPassword: string) =>
    invoke<void>("change_password", { oldPassword, newPassword }),
  rotateRecoveryKey: () => invoke<InitResult>("rotate_recovery_key"),

  // assets (M2)
  readSshConfig: () => invoke<ConfigView>("read_ssh_config"),
  scanKeys: () => invoke<ScannedKey[]>("scan_keys"),
  detectToolchain: () => invoke<Toolchain>("detect_toolchain"),
  listKeys: () => invoke<KeyRecord[]>("list_keys"),
  listIdentities: () => invoke<Identity[]>("list_identities"),
  importKey: (args: {
    privateText: string;
    publicText?: string;
    passphrase?: string;
    sourcePath?: string;
    name?: string;
  }) => invoke<KeyRecord>("import_key", { args }),
  importKeyFromPath: (path: string) => invoke<KeyRecord>("import_key_from_path", { path }),
  testConnection: (hostAlias: string) => invoke("test_connection", { hostAlias }),

  // write (M3)
  generateKey: (comment: string, name?: string) => invoke<KeyRecord>("generate_key", { comment, name }),
  previewConfig: (entry: ManagedEntry) => invoke<ConfigPreview>("preview_config", { entry }),
  applyConfig: (entry: ManagedEntry) => invoke("apply_config", { entry }),
  createIdentity: (args: Record<string, unknown>) => invoke("create_identity", { args }),
  revealKeyPassphrase: (password: string, keyId: string) =>
    invoke<string>("reveal_key_passphrase", { password, keyId }),

  // agent (M4)
  agentStatus: () => invoke<AgentStatus>("agent_status"),
  agentEnsure: () => invoke<AgentStatus>("agent_ensure"),
  agentLoad: (keyId: string) => invoke<void>("agent_load", { keyId }),
  agentLoadIdentity: (identityId: string) => invoke<void>("agent_load_identity", { identityId }),
  agentLoadAll: () => invoke<number>("agent_load_all"),
  agentUnload: (keyId: string) => invoke<void>("agent_unload", { keyId }),
  agentClear: () => invoke<void>("agent_clear"),

  // repo (M5)
  resolveUrl: (url: string) => invoke<Inference>("resolve_url", { url }),
  scanRepos: (root: string, maxDepth?: number) => invoke<RepoInfo[]>("scan_repos", { root, maxDepth }),
  addOwner: (identityId: string, owner: string) => invoke<void>("add_owner", { identityId, owner }),
  switchRepoIdentity: (repoPath: string, identityId: string) =>
    invoke<string>("switch_repo_identity", { repoPath, identityId }),
  setGithubPat: (token: string) => invoke<void>("set_github_pat", { token }),
  testGithubPat: () => invoke<string>("test_github_pat"),
  listGithubOrgs: () => invoke<string[]>("list_github_orgs"),
  uploadPublicKey: (keyId: string, title: string) => invoke<void>("upload_public_key", { keyId, title }),
};

export function errMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as AppErrorShape).message);
  return String(e);
}
