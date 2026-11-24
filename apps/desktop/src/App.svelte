<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    isTauriRuntime,
    onCoreEvent,
    type LiveMetrics,
    type StatusSnapshot,
    type UpdateStatus,
  } from "./lib/api";
  import AddonsView from "./components/AddonsView.svelte";
  import BackupsView from "./components/BackupsView.svelte";
  import ConsoleView from "./components/ConsoleView.svelte";
  import DashboardView from "./components/DashboardView.svelte";
  import NetworkView from "./components/NetworkView.svelte";
  import PlayersView from "./components/PlayersView.svelte";
  import SettingsView from "./components/SettingsView.svelte";
  import WizardView from "./components/WizardView.svelte";
  import WorldView from "./components/WorldView.svelte";

  type TabId =
    | "dashboard"
    | "console"
    | "players"
    | "world"
    | "addons"
    | "backups"
    | "network"
    | "settings"
    | "wizard";

  let status = $state<StatusSnapshot | null>(null);
  let metrics = $state<LiveMetrics | null>(null);
  let updateInfo = $state<UpdateStatus | null>(null);
  let logs = $state<string[]>([]);
  let activeTab = $state<TabId>("dashboard");
  let toast = $state<{ message: string; kind: "info" | "error" } | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  const hasServer = $derived(Boolean(status && status.servers.length > 0));
  const isFabric = $derived(status?.software === "fabric");

  function notify(message: string, kind: "info" | "error" = "info") {
    toast = { message, kind };
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toast = null;
    }, 4500);
  }

  async function refreshUpdates() {
    if (!status?.current || !isTauriRuntime()) return;
    try {
      updateInfo = await api.checkServerUpdates(status.current);
    } catch {
      // Quietly ignore offline update checks
    }
  }

  async function selectServer(name: string) {
    if (!isTauriRuntime()) return;
    status = await api.getStatus(name);
    activeTab = "dashboard";
    void refreshUpdates();
  }

  onMount(() => {
    if (!isTauriRuntime()) {
      status = {
        app_version: "0.1.0",
        data_dir: "C:\\Users\\Preview\\AppData\\Local\\BlockHost",
        hardware: {
          ram_mb: 8192,
          cores: 4,
          tier: "standard",
          recommended_heap_mb: 4915,
          recommended_max_players: 16,
          recommended_view_distance: 10,
          recommended_simulation_distance: 8,
          warning: null,
        },
        servers: ["kiwi-smp"],
        current: "kiwi-smp",
        software: "fabric",
        loader_version: "0.16.10",
        port: 25565,
        heap_mb: 4096,
        max_players: 12,
        mc_version: "1.21.4",
        paper_build: 0,
        backup_hours: 6,
        restart_hours: 0,
        perf_mode: true,
        aikar_flags: true,
        duckdns_domain: "kiwi-smp.duckdns.org",
        discord_enabled: false,
        tailscale_ip: null,
        autostart_enabled: false,
        lan_ip: "192.168.1.42",
        world_size_bytes: 184_000_000,
        running: null,
        stopping: false,
        uptime_secs: null,
        last_crash: null,
        setup: { state: "", message: "", pct: 0 },
        tunnel: {
          enabled: true,
          claim_url: null,
          address: null,
          saved_address: "kiwi-smp.joinmc.link",
          error: null,
        },
        backups: [],
      };
      return;
    }

    let unlisten: (() => void) | undefined;
    let poll: ReturnType<typeof setInterval> | undefined;

    void (async () => {
      try {
        unlisten = await onCoreEvent((ev) => {
          if (ev.kind === "status_changed") {
            status = ev;
            if (ev.servers.length === 0) activeTab = "wizard";
          } else if (ev.kind === "log_line") {
            logs = [...logs.slice(-999), ev.line];
          } else if (ev.kind === "setup_progress" && status) {
            status = {
              ...status,
              setup: { state: ev.state, message: ev.message, pct: ev.pct },
            };
          } else if (ev.kind === "alert") {
            notify(ev.message, ev.level === "error" ? "error" : "info");
          }
        });

        const initial = await api.getStatus();
        status = initial;
        if (initial.servers.length === 0) {
          activeTab = "wizard";
        } else {
          void refreshUpdates();
        }

        const initialLogs = await api.getLogs(0);
        logs = initialLogs.lines;

        poll = setInterval(async () => {
          if (status?.running && status.current) {
            try {
              metrics = await api.getLiveMetrics(status.current);
            } catch {
              // ignore transient RCON startup polls
            }
          } else {
            metrics = null;
          }
        }, 4000);
      } catch (e) {
        notify(String(e), "error");
      }
    })();

    return () => {
      unlisten?.();
      clearInterval(poll);
    };
  });

  const navItems = $derived<{ id: TabId; label: string }[]>([
    { id: "dashboard", label: "Dashboard" },
    { id: "console", label: "Console" },
    { id: "players", label: "Players" },
    { id: "addons", label: isFabric ? "Mods" : "Plugins" },
    { id: "world", label: "World" },
    { id: "backups", label: "Backups" },
    { id: "network", label: "Network" },
    { id: "settings", label: "Settings" },
  ]);
</script>

{#if !status}
  <div class="loading">
    <span class="muted">Starting BlockHost</span>
  </div>
{:else if !hasServer || activeTab === "wizard"}
  <div class="wizard-shell">
    {#if hasServer}
      <div class="wizard-top">
        <button class="btn btn-sm" onclick={() => (activeTab = "dashboard")}>Back to server</button>
      </div>
    {/if}
    <WizardView
      {status}
      onNotify={notify}
      onDone={() => {
        activeTab = "dashboard";
        void refreshUpdates();
      }}
    />
  </div>
{:else}
  <div class="layout">
    <aside class="sidebar">
      <div class="brand">
        <div class="logo" aria-hidden="true"></div>
        <div class="stack-xs">
          <strong class="brand-name">BlockHost</strong>
          <span class="muted tiny">
            {status.running ? "Server online" : "Server stopped"}
          </span>
        </div>
      </div>

      {#if status.servers.length > 1}
        <select
          class="select"
          value={status.current ?? ""}
          aria-label="Select server"
          onchange={(e) => selectServer((e.currentTarget as HTMLSelectElement).value)}
        >
          {#each status.servers as name (name)}
            <option value={name}>{name}</option>
          {/each}
        </select>
      {/if}

      <nav class="nav" aria-label="Main navigation">
        {#each navItems as item (item.id)}
          <button
            class="nav-item"
            class:active={activeTab === item.id}
            onclick={() => (activeTab = item.id)}
          >
            {item.label}
          </button>
        {/each}
      </nav>

      <div class="sidebar-footer">
        <button class="btn btn-sm full" onclick={() => (activeTab = "wizard")}>
          New server
        </button>
      </div>
    </aside>

    <main class="content">
      {#if activeTab === "dashboard"}
        <DashboardView
          {status}
          {metrics}
          {updateInfo}
          onNotify={notify}
          onNavigate={(t) => (activeTab = t as TabId)}
        />
      {:else if activeTab === "console"}
        <ConsoleView {status} {logs} onNotify={notify} />
      {:else if activeTab === "players"}
        <PlayersView {status} {metrics} onNotify={notify} />
      {:else if activeTab === "addons"}
        <AddonsView {status} onNotify={notify} />
      {:else if activeTab === "world"}
        <WorldView {status} onNotify={notify} />
      {:else if activeTab === "backups"}
        <BackupsView {status} onNotify={notify} />
      {:else if activeTab === "network"}
        <NetworkView {status} onNotify={notify} />
      {:else if activeTab === "settings"}
        <SettingsView
          {status}
          {updateInfo}
          onNotify={notify}
          onRefreshUpdates={refreshUpdates}
        />
      {/if}
    </main>
  </div>
{/if}

{#if toast}
  <div
    class="toast {toast.kind === 'error' ? 'toast-error' : 'toast-info'}"
    role="status"
    aria-live="polite"
  >
    {toast.message}
  </div>
{/if}

<style>
  .loading {
    height: 100%;
    display: grid;
    place-items: center;
  }

  .wizard-shell {
    padding: 24px;
  }

  .wizard-top {
    max-width: 680px;
    margin: 0 auto 12px;
  }

  .layout {
    display: grid;
    grid-template-columns: 210px 1fr;
    height: 100%;
  }

  .sidebar {
    background: var(--sidebar);
    padding: 20px 14px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 4px 6px;
  }

  .logo {
    width: 30px;
    height: 30px;
    border-radius: 8px;
    background: linear-gradient(180deg, #4ab256 0%, #4ab256 42%, #78563c 42%, #78563c 100%);
    flex-shrink: 0;
  }

  .brand-name {
    font-size: 15px;
    letter-spacing: -0.01em;
  }

  .stack-xs {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .tiny {
    font-size: 12px;
  }

  .nav {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
  }

  .nav-item {
    text-align: left;
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    font-weight: 550;
    color: var(--muted);
  }

  .nav-item:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .nav-item.active {
    background: var(--surface);
    color: var(--text);
    font-weight: 650;
    box-shadow: var(--shadow);
  }

  .sidebar-footer {
    padding-top: 8px;
  }

  .full {
    width: 100%;
  }

  .content {
    overflow-y: auto;
    padding: 26px 30px 40px;
  }

  .toast {
    position: fixed;
    right: 22px;
    bottom: 22px;
    max-width: 420px;
    padding: 12px 16px;
    border-radius: var(--radius-sm);
    font-weight: 550;
    box-shadow: var(--shadow-lg);
    z-index: 80;
  }

  .toast-info {
    background: var(--surface);
    color: var(--text);
  }

  .toast-error {
    background: var(--danger);
    color: #ffffff;
  }
</style>
