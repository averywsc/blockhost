<script lang="ts">
  import { api, formatGb, isTauriRuntime, type ServerSoftware, type StatusSnapshot } from "../lib/api";

  interface Props {
    status: StatusSnapshot;
    onNotify: (msg: string, kind?: "info" | "error") => void;
    onDone?: () => void;
  }

  let { status, onNotify, onDone }: Props = $props();

  let serverName = $state("my-server");
  let software = $state<ServerSoftware>("paper");
  let versions = $state<string[]>([]);
  let selectedVersion = $state("");
  let loadingVersions = $state(false);
  let ownerUsername = $state("");
  let heapMb = $state(2048);
  let tunnel = $state(false);
  let eulaAccepted = $state(false);
  let initializedRam = $state(false);

  $effect(() => {
    if (!initializedRam && status.hardware.recommended_heap_mb > 0) {
      heapMb = status.hardware.recommended_heap_mb;
      initializedRam = true;
    }
  });

  $effect(() => {
    const sw = software;
    selectedVersion = "";
    if (!isTauriRuntime()) {
      versions = ["26.1", "1.21.11", "1.21.4", "1.20.4"];
      return;
    }
    loadingVersions = true;
    api
      .listMcVersions(sw)
      .then((list) => {
        if (software === sw) versions = list.slice(0, 40);
      })
      .catch(() => {
        if (software === sw) versions = [];
      })
      .finally(() => {
        if (software === sw) loadingVersions = false;
      });
  });

  const maxHeap = $derived(Math.max(1024, status.hardware.ram_mb - 1024));
  const ramTooHigh = $derived(heapMb > status.hardware.recommended_heap_mb * 1.25);
  const working = $derived(status.setup.state === "working");

  $effect(() => {
    if (status.setup.state === "done" && onDone) {
      onDone();
    }
  });

  async function submit() {
    if (!eulaAccepted) {
      onNotify("Please accept the Minecraft EULA first.", "error");
      return;
    }
    try {
      await api.createServer({
        name: serverName.trim(),
        software,
        version: selectedVersion || null,
        heap_mb: heapMb,
        tunnel,
        owner_username: ownerUsername.trim() || null,
        eula: true,
      });
    } catch (e) {
      onNotify(String(e), "error");
    }
  }
</script>

<div class="wizard-wrap">
  <div class="card stack wizard-card">
    <div class="stack-sm">
      <h1 class="page-title">Create your Minecraft server</h1>
      <p class="muted">
        Pick your Minecraft version and software. BlockHost handles Java, safe whitelist defaults, and backups automatically.
      </p>
    </div>

    {#if working}
      <div class="subcard stack-sm">
        <div class="row-between">
          <strong>{status.setup.message || "Setting up your server"}</strong>
          <span class="badge badge-green">{status.setup.pct}%</span>
        </div>
        <div class="progress-track">
          <div class="progress-fill" style="width: {Math.max(6, status.setup.pct)}%"></div>
        </div>
        <p class="muted">This one-time download usually takes under a minute.</p>
      </div>
    {:else}
      {#if status.setup.state === "error" && status.setup.message}
        <div class="banner banner-danger">{status.setup.message}</div>
      {/if}

      <div class="stack-sm">
        <span class="field-label">Server software</span>
        <div class="software-grid">
          <button
            type="button"
            class="choice"
            class:selected={software === "paper"}
            onclick={() => (software = "paper")}
          >
            <div class="row-between">
              <strong>Paper</strong>
              <span class="badge badge-green">Recommended</span>
            </div>
            <p class="muted small">
              Fastest speed on old laptops. Supports plugins from Modrinth. Friends join with vanilla Minecraft.
            </p>
          </button>

          <button
            type="button"
            class="choice"
            class:selected={software === "fabric"}
            onclick={() => (software = "fabric")}
          >
            <div class="row-between">
              <strong>Fabric</strong>
              <span class="badge badge-blue">Mods</span>
            </div>
            <p class="muted small">
              Lightweight mod loader. Install Fabric mods from Modrinth or drop jar files into your mods folder.
            </p>
          </button>
        </div>
      </div>

      <div class="three-col">
        <label class="stack-sm">
          <span class="field-label">Minecraft version</span>
          <select class="select" bind:value={selectedVersion} disabled={loadingVersions}>
            <option value="">
              {loadingVersions ? "Loading versions" : "Latest stable"}
            </option>
            {#each versions as v (v)}
              <option value={v}>{v}</option>
            {/each}
          </select>
        </label>

        <label class="stack-sm">
          <span class="field-label">Server name</span>
          <input
            class="input"
            bind:value={serverName}
            placeholder="my-server"
            maxlength="32"
          />
        </label>

        <label class="stack-sm">
          <span class="field-label">Your username (admin)</span>
          <input
            class="input"
            bind:value={ownerUsername}
            placeholder="e.g. Steve"
            maxlength="16"
          />
        </label>
      </div>

      <div class="subcard stack-sm">
        <div class="row-between">
          <span class="field-label">Memory for Minecraft</span>
          <strong>{formatGb(heapMb)}</strong>
        </div>
        <input
          type="range"
          min="1024"
          max={maxHeap}
          step="256"
          bind:value={heapMb}
        />
        <div class="row-between muted tiny">
          <span>This PC has {formatGb(status.hardware.ram_mb)}</span>
          <span>Recommended {formatGb(status.hardware.recommended_heap_mb)}</span>
        </div>
        {#if ramTooHigh}
          <div class="banner banner-warn">
            Using more than the recommended memory can slow down Windows on this PC.
          </div>
        {/if}
        {#if status.hardware.warning}
          <div class="banner banner-warn">{status.hardware.warning}</div>
        {/if}
      </div>

      <div class="stack-sm">
        <span class="field-label">How friends join over the internet</span>
        <div class="software-grid">
          <button
            type="button"
            class="choice"
            class:selected={!tunnel}
            onclick={() => (tunnel = false)}
          >
            <div class="row-between">
              <strong>Direct or Tailscale</strong>
              <span class="badge badge-green">Best NZ ping</span>
            </div>
            <p class="muted small">
              Skip playit.gg completely. Use direct port forwarding with DuckDNS or Tailscale in Network for lowest lag.
            </p>
          </button>

          <button
            type="button"
            class="choice"
            class:selected={tunnel}
            onclick={() => (tunnel = true)}
          >
            <div class="row-between">
              <strong>Playit.gg tunnel</strong>
              <span class="badge">No router setup</span>
            </div>
            <p class="muted small">
              Easy free relay if your internet provider blocks port forwarding (CGNAT) and friends do not want Tailscale.
            </p>
          </button>
        </div>
      </div>

      <label class="subcard check-row">
        <input type="checkbox" bind:checked={eulaAccepted} />
        <div class="stack-xs">
          <strong>I accept the Minecraft End User License Agreement</strong>
          <span class="muted tiny">
            Required by Mojang to run a server (https://aka.ms/MinecraftEULA).
          </span>
        </div>
      </label>

      <div class="row-between">
        <span class="muted tiny">Whitelist starts ON so only invited players can join.</span>
        <button
          class="btn btn-primary create-btn"
          disabled={!eulaAccepted || !serverName.trim()}
          onclick={submit}
        >
          Create server
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  .wizard-wrap {
    max-width: 720px;
    margin: 12px auto;
  }

  .wizard-card {
    padding: 28px;
  }

  .field-label {
    font-size: 13px;
    font-weight: 600;
  }

  .software-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 12px;
  }

  .three-col {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 12px;
  }

  .choice {
    text-align: left;
    background: var(--surface-alt);
    border-radius: var(--radius-sm);
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    box-shadow: inset 0 0 0 2px transparent;
  }

  .choice:hover {
    background: var(--surface-hover);
  }

  .choice.selected {
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 2px var(--accent);
  }

  .check-row {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    cursor: pointer;
  }

  .stack-xs {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .small {
    font-size: 13px;
  }

  .tiny {
    font-size: 12px;
  }

  .create-btn {
    padding: 11px 22px;
  }

  .progress-track {
    height: 10px;
    border-radius: 999px;
    background: var(--input-bg);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    border-radius: 999px;
    background: var(--accent);
    transition: width 220ms ease;
  }
</style>
