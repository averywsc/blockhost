import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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

export interface StatusSnapshot {
  app_version: string;
  data_dir: string;
  hardware: HardwareSummary;
  servers: string[];
  current: string | null;
  port: number | null;
  heap_mb: number | null;
  max_players: number | null;
  mc_version: string | null;
  paper_build: number | null;
  backup_hours: number | null;
  restart_hours: number | null;
  perf_mode: boolean | null;
  autostart_enabled: boolean;
  lan_ip: string | null;
  world_size_bytes: number;
  running: string | null;
  stopping: boolean;
  uptime_secs: number | null;
  setup: SetupSummary;
  tunnel: TunnelSummary;
  backups: BackupEntry[];
}

export type CoreEvent =
  | ({ kind: "status_changed" } & StatusSnapshot)
  | { kind: "log_line"; index: number; line: string }
  | { kind: "setup_progress"; state: string; message: string; pct: number }
  | { kind: "alert"; level: "info" | "warning" | "error"; message: string };

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function getStatus(selected?: string): Promise<StatusSnapshot> {
  return invoke<StatusSnapshot>("get_status", { selected: selected ?? null });
}

export async function pingCore(): Promise<StatusSnapshot> {
  return invoke<StatusSnapshot>("ping_core");
}

export async function onCoreEvent(handler: (event: CoreEvent) => void): Promise<UnlistenFn> {
  return listen<CoreEvent>("core://event", (ev) => handler(ev.payload));
}
