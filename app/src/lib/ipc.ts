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
  launchAtLogin: boolean;
  graceDays: number;
  graceActive: boolean;
  graceExpiresAt: string | null;
  closeAction: "tray" | "quit" | null;
  writesLocked?: boolean;
  startupNote?: string | null;
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
  deployedPath: string | null;
  importedAt: string;
}
export interface RevealedKeyMaterial {
  publicOpenssh: string;
  privateOpenssh: string;
  passphrase: string | null;
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
  workspacePath: string | null;
  systemPath: string;
  registered: boolean;
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
export interface EnvCheck {
  key: string;
  label: string;
  ok: boolean;
  current: string | null;
  expected: string | null;
}
export interface AgentUnifyStatus {
  gitInstalled: boolean;
  agentRunning: boolean;
  gitSsh: string | null;
  sshAdd: string | null;
  authSock: string | null;
  agentPid: string | null;
  gitConfigOk: boolean;
  userGitSshOk: boolean;
  userSockOk: boolean;
  powershellProfileOk: boolean;
  bashProfileOk: boolean;
  aligned: boolean;
  gitConfigValue: string | null;
  userGitSsh: string | null;
  userAuthSock: string | null;
  checks: EnvCheck[];
}
export interface AgentUnifyReport {
  gitSsh: string;
  sshAdd: string;
  authSock: string;
  agentPid: string | null;
  gitConfig: boolean;
  userEnv: boolean;
  powershellProfile: boolean;
  bashProfile: boolean;
  steps: string[];
  hint: string;
}
export interface AgentStatus {
  running: boolean;
  usingFallback: boolean;
  ssh: string | null;
  sshAdd: string | null;
  authSock: string | null;
  keys: AgentKeyResolved[];
  unify: AgentUnifyStatus;
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
export interface AuthResult {
  ok: boolean;
  account: string | null;
  message: string;
  errorCode: string | null;
}
export interface CreateIdentityArgs {
  name: string;
  platform: string;
  hostAlias: string;
  realHost: string;
  user?: string;
  email?: string;
  gitUserName?: string;
  strictMode: boolean;
  keyId?: string;
  keyComment?: string;
  owners?: string[];
}
export interface UpdateIdentityArgs {
  id: string;
  name: string;
  platform: string;
  hostAlias: string;
  realHost: string;
  user?: string;
  email?: string;
  gitUserName?: string;
  strictMode: boolean;
  owners?: string[];
}
export interface CreateIdentityResult {
  identity: Identity;
  publicOpenssh: string;
  configBackup: string | null;
  configVerified: boolean;
  identityFile: string;
}
export interface ClonePlan {
  kind: "empty" | "missing" | "parent" | "existingProject" | "alreadyGit" | "alreadyGitHasRemote" | "blocked" | string;
  suggestedMode: "clone" | "init" | "addRemote" | "blocked" | string;
  dest: string;
  targetPath: string;
  repoName: string;
  message: string;
  canProceed: boolean;
}
export interface CloneResult {
  dest: string;
  usedUrl: string;
  identityName: string;
  mode: string;
}
export interface CloneOrInitArgs {
  url: string;
  destDir: string;
  identityId: string;
  mode: string;
}
export interface ManagedRepoView {
  id: string;
  path: string;
  name: string;
  remoteUrl: string | null;
  identityId: string | null;
  identityName: string | null;
  addedAt: string;
  source: string;
  exists: boolean;
  currentAlias: string | null;
  needsAliasFix: boolean;
}
export interface ImportScanResult {
  imported: number;
  updated: number;
  skippedNoRemote: number;
  repos: ManagedRepoView[];
}

export interface BackupSummary {
  workspaceId: string;
  createdAt: string;
  identityCount: number;
  keyCount: number;
  repoCount: number;
  hasGithubPat: boolean;
}

export interface S3Config {
  endpoint: string;
  bucket: string;
  region: string;
  accessKeyId: string;
  secretAccessKey: string;
  prefix: string;
}

export interface CloudSyncStatus {
  remoteExists: boolean;
  remoteUpdatedAt?: string;
  remoteWorkspaceId?: string;
  localIdentityCount: number;
  localKeyCount: number;
  localRepoCount: number;
  status: "synced" | "local_ahead" | "remote_ahead" | "not_synced" | "different_workspace" | "unconfigured";
  headerReady?: boolean;
}

export interface SyncResult {
  syncedAt: string;
  objectsTransferred: number;
  identityCount: number;
  keyCount: number;
  repoCount: number;
  message: string;
}

export interface AutoSyncSettings {
  minutes: number;
  lastAutoSyncAt?: string | null;
  lastAutoSyncMessage?: string | null;
  defaultMinutes: number;
}

export interface CloudSnapshot {
  id: string;
  createdAt: string;
  clientName: string;
  identityCount: number;
  keyCount: number;
  repoCount: number;
  isRecent?: boolean;
  isDailyFirst?: boolean;
}

export interface CloudRestorePreview {
  workspaceId: string;
  updatedAt?: string | null;
  identityCount: number;
  keyCount: number;
  repoCount: number;
  hasManifest: boolean;
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
  vaultTryGraceUnlock: () => invoke<boolean>("vault_try_grace_unlock"),
  setLaunchAtLogin: (enabled: boolean) => invoke<void>("set_launch_at_login", { enabled }),
  setGraceDays: (days: number) => invoke<void>("set_grace_days", { days }),
  factoryReset: (confirmed: boolean, confirmPhrase: string) =>
    invoke<{ steps: string[] }>("factory_reset", { confirmed, confirmPhrase }),
  applyCloseChoice: (action: "tray" | "quit" | "cancel", remember: boolean) =>
    invoke<void>("apply_close_choice", { action, remember }),
  getClosePreference: () => invoke<"tray" | "quit" | null>("get_close_preference"),
  clearClosePreference: () => invoke<void>("clear_close_preference"),

  // assets (M2)
  readSshConfig: () => invoke<ConfigView>("read_ssh_config"),
  openSshConfig: () => invoke<string>("open_ssh_config"),
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
  testConnection: (hostAlias: string) => invoke<AuthResult>("test_connection", { hostAlias }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),

  // write (M3)
  generateKey: (comment: string, name?: string) => invoke<KeyRecord>("generate_key", { comment, name }),
  previewConfig: (entry: ManagedEntry) => invoke<ConfigPreview>("preview_config", { entry }),
  applyConfig: (entry: ManagedEntry) => invoke("apply_config", { entry }),
  createIdentity: (args: CreateIdentityArgs) => invoke<CreateIdentityResult>("create_identity", { args }),
  stageIdentityDraft: (args: {
    name: string;
    hostAlias: string;
    realHost: string;
    user?: string;
    strictMode: boolean;
    keyId: string;
  }) =>
    invoke<{ identityFile: string; configVerified: boolean; keyId: string }>("stage_identity_draft", { args }),
  abortIdentityDraft: (args: { hostAlias: string; keyId: string }) =>
    invoke<void>("abort_identity_draft", { args }),
  updateIdentity: (args: UpdateIdentityArgs) => invoke<Identity>("update_identity", { args }),
  deleteIdentity: (identityId: string) => invoke<void>("delete_identity", { identityId }),
  revealKeyPassphrase: (password: string, keyId: string) =>
    invoke<string>("reveal_key_passphrase", { password, keyId }),
  revealKeyMaterial: (password: string, keyId: string) =>
    invoke<RevealedKeyMaterial>("reveal_key_material", { password, keyId }),

  // agent (M4)
  agentStatus: () => invoke<AgentStatus>("agent_status"),
  agentEnsure: () => invoke<AgentStatus>("agent_ensure"),
  agentUnifyEnv: (confirmed: boolean) => invoke<AgentUnifyReport>("agent_unify_env", { confirmed }),
  agentLoad: (keyId: string) => invoke<void>("agent_load", { keyId }),
  agentLoadIdentity: (identityId: string) => invoke<void>("agent_load_identity", { identityId }),
  agentLoadAll: () => invoke<number>("agent_load_all"),
  agentUnload: (keyId: string) => invoke<void>("agent_unload", { keyId }),
  agentClear: () => invoke<void>("agent_clear"),

  // repo (M5)
  resolveUrl: (url: string) => invoke<Inference>("resolve_url", { url }),
  scanRepos: (root: string, maxDepth?: number) => invoke<RepoInfo[]>("scan_repos", { root, maxDepth }),
  scanAndImportRepos: (root: string, maxDepth?: number) =>
    invoke<ImportScanResult>("scan_and_import_repos", { root, maxDepth }),
  listManagedRepos: () => invoke<ManagedRepoView[]>("list_managed_repos"),
  removeManagedRepo: (repoId: string) => invoke<void>("remove_managed_repo", { repoId }),
  setRepoRemote: (args: { repoId: string; remoteUrl: string; identityId?: string }) =>
    invoke<ManagedRepoView>("set_repo_remote", { args }),
  openRepoDir: (path: string) => invoke<void>("open_repo_dir", { path }),
  addOwner: (identityId: string, owner: string) => invoke<void>("add_owner", { identityId, owner }),
  switchRepoIdentity: (repoPath: string, identityId: string) =>
    invoke<string>("switch_repo_identity", { repoPath, identityId }),
  inspectCloneTarget: (destDir: string, repoName: string) =>
    invoke<ClonePlan>("inspect_clone_target", { destDir, repoName }),
  cloneRepo: (args: CloneOrInitArgs) => invoke<CloneResult>("clone_repo", { args }),
  githubPatStatus: () => invoke<{ configured: boolean }>("github_pat_status"),
  setGithubPat: (token: string) => invoke<void>("set_github_pat", { token }),
  clearGithubPat: () => invoke<void>("clear_github_pat"),
  testGithubPat: () => invoke<string>("test_github_pat"),
  listGithubOrgs: () => invoke<string[]>("list_github_orgs"),
  uploadPublicKey: (keyId: string, title: string) => invoke<void>("upload_public_key", { keyId, title }),

  // backup (M6)
  exportVaultBackup: (destPath: string, password: string) =>
    invoke<BackupSummary>("export_vault_backup", { destPath, password }),
  inspectVaultBackup: (srcPath: string, password: string) =>
    invoke<BackupSummary>("inspect_vault_backup", { srcPath, password }),
  importVaultBackup: (srcPath: string, password: string, merge: boolean) =>
    invoke<BackupSummary>("import_vault_backup", { srcPath, password, merge }),

  // sync (v1.1)
  getCloudSyncConfig: () => invoke<S3Config | null>("get_cloud_sync_config"),
  saveCloudSyncConfig: (syncConfig: S3Config | null) =>
    invoke<void>("save_cloud_sync_config", { syncConfig }),
  exportS3Config: (destPath: string, syncConfig: S3Config) =>
    invoke<void>("export_s3_config", { destPath, syncConfig }),
  importS3Config: (srcPath: string) => invoke<S3Config>("import_s3_config", { srcPath }),
  testCloudSyncConfig: (syncConfig: S3Config) =>
    invoke<number>("test_cloud_sync_config", { syncConfig }),
  getCloudSyncStatus: () => invoke<CloudSyncStatus>("get_cloud_sync_status"),
  cloudSyncPush: () => invoke<SyncResult>("cloud_sync_push"),
  cloudSyncPull: () => invoke<SyncResult>("cloud_sync_pull"),
  getAutoSyncSettings: () => invoke<AutoSyncSettings>("get_auto_sync_settings"),
  setAutoSyncMinutes: (minutes: number) => invoke<number>("set_auto_sync_minutes", { minutes }),
  listCloudSnapshots: () => invoke<CloudSnapshot[]>("list_cloud_snapshots"),
  restoreCloudSnapshot: (snapshotId: string) =>
    invoke<SyncResult>("restore_cloud_snapshot", { snapshotId }),
  runAutoSyncNow: () => invoke<SyncResult | null>("run_auto_sync_now"),
  previewCloudRestore: (syncConfig: S3Config, recoveryKey: string) =>
    invoke<CloudRestorePreview>("preview_cloud_restore", { syncConfig, recoveryKey }),
  restoreFromCloud: (args: {
    path: string;
    password: string;
    recoveryKey: string;
    syncConfig: S3Config;
    includeRepos?: boolean;
  }) => invoke<SyncResult>("restore_from_cloud", args),
};

export function errMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as AppErrorShape).message);
  return String(e);
}
