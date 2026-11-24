import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type ServerSoftware = "paper" | "fabric";

export interface HardwareSummary {
  ram_mb: number;
  cores: number;
  tier: string;
  recommended_heap_mb: number;
  recommended_max_players: number;
  recommended_view_distance: number;
  recommended_simulation_distance: number;
  warning: string | null;
}

export interface SetupSummary {
  state: string;
  message: string;
  pct: number;
}

export interface TunnelSummary {
  enabled: boolean;
  claim_url: string | null;
  address: string | null;
  saved_address: string | null;
  error: string | null;
}

export interface BackupEntry {
  filename: string;
  size_bytes: number;
}

export interface CrashDiagnosis {
  title: string;
  explanation: string;
  suggestion: string;
}

export interface StatusSnapshot {
  app_version: string;
  data_dir: string;
  hardware: HardwareSummary;
  servers: string[];
  current: string | null;
  software: ServerSoftware | null;
  loader_version: string | null;
  port: number | null;
  heap_mb: number | null;
  max_players: number | null;
  mc_version: string | null;
  paper_build: number | null;
  backup_hours: number | null;
  restart_hours: number | null;
  perf_mode: boolean | null;
  aikar_flags: boolean;
  duckdns_domain: string | null;
  tailscale_ip: string | null;
  autostart_enabled: boolean;
  lan_ip: string | null;
  world_size_bytes: number;
  running: string | null;
  stopping: boolean;
  uptime_secs: number | null;
  last_crash: CrashDiagnosis | null;
  setup: SetupSummary;
  tunnel: TunnelSummary;
  backups: BackupEntry[];
}

export interface LiveMetrics {
  reachable: boolOrFalse;
  players_text: string | null;
  online_players: string[];
  tps_text: string | null;
  uptime_secs: number | null;
  process_ram_mb: number | null;
  process_cpu_pct: number | null;
}

type boolOrFalse = boolean;

export interface PlayerSummary {
  name: string;
  uuid: string;
  is_admin: boolean;
}

export interface BannedEntry {
  uuid: string | null;
  name: string;
  reason: string | null;
}

export interface PlayersState {
  allowed: PlayerSummary[];
  banned: BannedEntry[];
  whitelist_enabled: boolean;
}

export interface GameRules {
  difficulty: string;
  gamemode: string;
  pvp: boolean;
  enable_command_block: boolean;
  view_distance: number;
  simulation_distance: number;
  sync_chunk_writes: boolean;
  max_players: number;
  motd: string;
  white_list: boolean;
  level_seed: string;
}

export interface PluginHit {
  project_id: string;
  title: string;
  description: string;
  downloads: number;
  author: string;
}

export interface ResolvedPluginVersion {
  version_number: string;
  filename: string;
  url: string;
  sha512: string | null;
  required_dependency_count: number;
}

export interface InstalledPlugin {
  filename: string;
  size_bytes: number;
}

export interface ResolvedPaper {
  mc_version: string;
  build: number;
  url: string;
  sha256: string;
  java_feature: number;
}

export interface UpdateStatus {
  current_version: string;
  current_build: number;
  same_version: ResolvedPaper | null;
  latest_major: ResolvedPaper | null;
}

export interface PingStats {
  ok: boolean;
  sent: number;
  received: number;
  avg_ms: number | null;
  min_ms: number | null;
  max_ms: number | null;
  jitter_ms: number | null;
  loss_pct: number;
  grade: string;
  samples_ms: number[];
}

export interface LinkInfo {
  name: string;
  wifi: boolean;
  gateway: string;
  speed: string;
}

export interface CgnatReport {
  detected: boolean;
  router_wan_ip: string | null;
  public_ip: string | null;
  summary: string;
}

export interface NetworkOverview {
  lan_ip: string | null;
  public_ip: string | null;
  tailscale_ip: string | null;
  link: LinkInfo | null;
  cgnat: CgnatReport;
}

export interface ActionResult {
  ok: boolean;
  needs_admin: boolean;
  message: string;
}

export type CoreEvent =
  | ({ kind: "status_changed" } & StatusSnapshot)
  | { kind: "log_line"; index: number; line: string }
  | { kind: "setup_progress"; state: string; message: string; pct: number }
  | { kind: "alert"; level: "info" | "warning" | "error"; message: string };

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function onCoreEvent(handler: (event: CoreEvent) => void): Promise<UnlistenFn> {
  return listen<CoreEvent>("core://event", (ev) => handler(ev.payload));
}

export const api = {
  listMcVersions: (software: ServerSoftware) =>
    invoke<string[]>("list_mc_versions", { software }),
  getStatus: (selected?: string) =>
    invoke<StatusSnapshot>("get_status", { selected: selected ?? null }),
  getLiveMetrics: (serverName: string) =>
    invoke<LiveMetrics>("get_live_metrics", { serverName }),
  getLogs: (since: number) =>
    invoke<{ next: number; lines: string[] }>("get_logs", { since }),
  createServer: (args: {
    name: string;
    software: ServerSoftware;
    version: string | null;
    heap_mb: number | null;
    tunnel: boolean;
    owner_username: string | null;
    eula: boolean;
  }) => invoke<void>("create_server", { args }),
  startServer: (serverName: string, useTunnel?: boolean) =>
    invoke<void>("start_server", { serverName, useTunnel: useTunnel ?? null }),
  stopServer: (warnSeconds: number) => invoke<void>("stop_server", { warnSeconds }),
  forceStopServer: () => invoke<void>("force_stop_server"),
  restartServer: (serverName: string) => invoke<void>("restart_server", { serverName }),
  sendCommand: (command: string) => invoke<string>("send_command", { command }),
  listPlayers: (serverName: string) => invoke<PlayersState>("list_players", { serverName }),
  addPlayer: (serverName: string, username: string, admin: boolean) =>
    invoke<string>("add_player", { serverName, username, admin }),
  removePlayer: (serverName: string, username: string, adminOnly: boolean) =>
    invoke<string>("remove_player", { serverName, username, adminOnly }),
  playerAdminAction: (action: string, username: string, arg?: string) =>
    invoke<string>("player_admin_action", { action, username, arg: arg ?? null }),
  backupNow: (serverName: string) => invoke<void>("backup_now", { serverName }),
  restoreBackup: (serverName: string, filename: string) =>
    invoke<string>("restore_backup", { serverName, filename }),
  updateSettings: (args: {
    server_name: string;
    heap_mb?: number;
    max_players?: number;
    motd?: string;
    tunnel?: boolean;
    playit_address?: string;
    backup_hours?: number;
    restart_hours?: number;
    perf_mode?: boolean;
    aikar_flags?: boolean;
  }) => invoke<StatusSnapshot>("update_settings", { args }),
  setWhitelistEnabled: (serverName: string, enabled: boolean) =>
    invoke<void>("set_whitelist_enabled", { serverName, enabled }),
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
  checkServerUpdates: (serverName: string) =>
    invoke<UpdateStatus | null>("check_server_updates", { serverName }),
  installPaperUpdate: (serverName: string, pick: ResolvedPaper) =>
    invoke<string>("install_paper_update", { serverName, pick }),
  getGameRules: (serverName: string) => invoke<GameRules>("get_game_rules", { serverName }),
  updateGameRule: (serverName: string, rule: string, value: string) =>
    invoke<GameRules>("update_game_rule", { serverName, rule, value }),
  createNewWorld: (serverName: string, seed: string) =>
    invoke<string>("create_new_world", { serverName, seed }),
  importWorldZip: (serverName: string) =>
    invoke<string | null>("import_world_zip", { serverName }),
  exportWorldZip: (serverName: string) =>
    invoke<string | null>("export_world_zip", { serverName }),
  listAddons: (serverName: string) => invoke<InstalledPlugin[]>("list_addons", { serverName }),
  searchAddons: (serverName: string, query: string) =>
    invoke<PluginHit[]>("search_addons", { serverName, query }),
  resolveAddon: (serverName: string, projectId: string) =>
    invoke<ResolvedPluginVersion>("resolve_addon", { serverName, projectId }),
  installAddon: (serverName: string, resolved: ResolvedPluginVersion) =>
    invoke<string>("install_addon", { serverName, resolved }),
  importLocalAddon: (serverName: string) =>
    invoke<string | null>("import_local_addon", { serverName }),
  removeAddon: (serverName: string, filename: string) =>
    invoke<void>("remove_addon", { serverName, filename }),
  getNetworkOverview: () => invoke<NetworkOverview>("get_network_overview"),
  pingTarget: (target: string, count: number) =>
    invoke<PingStats>("ping_target", { target, count }),
  setupPortForward: (serverName: string) =>
    invoke<[ActionResult, ActionResult]>("setup_port_forward", { serverName }),
  removePortForward: (serverName: string) =>
    invoke<ActionResult>("remove_port_forward", { serverName }),
  saveDuckdns: (serverName: string, domain: string, token: string) =>
    invoke<string>("save_duckdns", { serverName, domain, token }),
  openFolder: (serverName?: string, sub?: string) =>
    invoke<void>("open_folder", { serverName: serverName ?? null, sub: sub ?? null }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
};

export function formatGb(mb: number): string {
  return `${(mb / 1024).toFixed(1)} GB`;
}

export function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

export function formatUptime(secs: number | null): string {
  if (secs == null) return "Offline";
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}
