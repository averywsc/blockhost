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
  claim_url: string | null;
  address: string | null;
  error: string | null;
}

export interface StatusSnapshot {
  app_version: string;
  data_dir: string;
  hardware: HardwareSummary;
  servers: string[];
  current: string | null;
  port: number | null;
  mc_version: string | null;
  paper_build: number | null;
  running: string | null;
  stopping: boolean;
  setup: SetupSummary;
  tunnel: TunnelSummary;
  backups: string[];
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
