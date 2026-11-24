<script lang="ts">
  import {
    api,
    formatSize,
    type InstalledPlugin,
    type PluginHit,
    type ResolvedPluginVersion,
    type StatusSnapshot,
  } from "../lib/api";
  import ConfirmModal from "./ConfirmModal.svelte";

  interface Props {
    status: StatusSnapshot;
    onNotify: (msg: string, kind?: "info" | "error") => void;
  }

  let { status, onNotify }: Props = $props();

  let installed = $state<InstalledPlugin[]>([]);
  let query = $state("");
  let hits = $state<PluginHit[]>([]);
  let searching = $state(false);
  let busy = $state(false);
  let pendingInstall = $state<{ hit: PluginHit; version: ResolvedPluginVersion } | null>(null);

  const serverName = $derived(status.current ?? "");
  const isFabric = $derived(status.software === "fabric");
  const isRunning = $derived(status.running !== null && status.running === serverName);
  const noun = $derived(isFabric ? "Mods" : "Plugins");
  const nounSingle = $derived(isFabric ? "mod" : "plugin");

  async function refreshInstalled() {
    if (!serverName) return;
    try {
      installed = await api.listAddons(serverName);
    } catch (e) {
      onNotify(String(e), "error");
    }
  }

  $effect(() => {
    void serverName;
    void refreshInstalled();
  });

  async function doSearch() {
    if (!serverName || !query.trim()) return;
    searching = true;
    try {
      hits = await api.searchAddons(serverName, query.trim());
      if (hits.length === 0) {
        onNotify(`No ${noun.toLowerCase()} found for Minecraft ${status.mc_version ?? ""}.`);
      }
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      searching = false;
    }
  }

  async function prepareInstall(hit: PluginHit) {
    if (!serverName) return;
    busy = true;
    try {
      const version = await api.resolveAddon(serverName, hit.project_id);
      pendingInstall = { hit, version };
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function confirmInstall() {
    if (!serverName || !pendingInstall) return;
    const item = pendingInstall;
    pendingInstall = null;
    busy = true;
    try {
      const msg = await api.installAddon(serverName, item.version);
      onNotify(msg);
      await refreshInstalled();
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function remove(filename: string) {
    if (!serverName) return;
    busy = true;
    try {
      await api.removeAddon(serverName, filename);
      onNotify(`Removed ${filename}`);
      await refreshInstalled();
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function addLocalJar() {
    if (!serverName) return;
    busy = true;
    try {
      const msg = await api.importLocalAddon(serverName);
      if (msg) {
        onNotify(msg);
        await refreshInstalled();
      }
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }
</script>

<div class="stack">
  <div class="row-between">
    <div class="stack-xs">
      <div class="row">
        <h1 class="page-title">{noun}</h1>
        <span class="badge badge-blue">
          {isFabric ? "Fabric" : "Paper"} {status.mc_version ?? ""}
        </span>
      </div>
      <p class="muted">
        Search verified Modrinth {noun.toLowerCase()} or add local .jar files directly.
      </p>
    </div>

    <div class="row">
      <button class="btn btn-primary" disabled={busy} onclick={addLocalJar}>
        Add .jar file
      </button>
      <button class="btn" onclick={() => api.openFolder(serverName, "addons")}>
        Open {isFabric ? "mods" : "plugins"} folder
      </button>
    </div>
  </div>

  <form
    class="card row"
    onsubmit={(e) => {
      e.preventDefault();
      doSearch();
    }}
  >
    <input
      class="input grow"
      bind:value={query}
      placeholder={isFabric
        ? "Search Fabric mods on Modrinth (e.g. fabric api, lithium, voice chat)"
        : "Search Paper plugins on Modrinth (e.g. chunky, worldedit, essentials)"}
    />
    <button class="btn btn-primary" type="submit" disabled={searching || !query.trim()}>
      {searching ? "Searching" : "Search"}
    </button>
  </form>

  {#if hits.length > 0}
    <div class="stack-sm">
      <h2 class="section-title">Modrinth results</h2>
      {#each hits as hit (hit.project_id)}
        <div class="card item-row">
          <div class="stack-xs grow">
            <div class="row">
              <strong>{hit.title}</strong>
              <span class="muted tiny">by {hit.author}</span>
              <span class="badge">{hit.downloads.toLocaleString()} downloads</span>
            </div>
            <p class="muted small">{hit.description}</p>
          </div>
          <button class="btn btn-primary btn-sm" disabled={busy} onclick={() => prepareInstall(hit)}>
            Install
          </button>
        </div>
      {/each}
    </div>
  {/if}

  <div class="stack-sm">
    <h2 class="section-title">Installed {noun.toLowerCase()} ({installed.length})</h2>
    {#if installed.length === 0}
      <div class="card">
        <p class="muted">No {noun.toLowerCase()} installed yet.</p>
      </div>
    {:else}
      {#each installed as item (item.filename)}
        <div class="card item-row">
          <div class="stack-xs">
            <strong class="mono">{item.filename}</strong>
            <span class="muted tiny">{formatSize(item.size_bytes)}</span>
          </div>
          <button
            class="btn btn-danger-soft btn-sm"
            disabled={busy || isRunning}
            onclick={() => remove(item.filename)}
          >
            Remove
          </button>
        </div>
      {/each}
    {/if}
    {#if isRunning && installed.length > 0}
      <p class="muted tiny">Stop the server first to remove installed {noun.toLowerCase()}.</p>
    {/if}
  </div>
</div>

<ConfirmModal
  open={pendingInstall !== null}
  title={`Install ${pendingInstall?.hit.title ?? nounSingle}`}
  message={pendingInstall
    ? `Install ${pendingInstall.hit.title} ${pendingInstall.version.version_number} by ${pendingInstall.hit.author}? Only install ${noun.toLowerCase()} from authors you trust. Downloads are SHA-512 verified from Modrinth.${
        pendingInstall.version.required_dependency_count > 0
          ? ` Note: this ${nounSingle} requires ${pendingInstall.version.required_dependency_count} other ${nounSingle}(s) to load.`
          : ""
      }`
    : ""}
  confirmLabel="Install"
  onConfirm={confirmInstall}
  onCancel={() => (pendingInstall = null)}
/>

<style>
  .stack-xs {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .grow {
    flex: 1;
    min-width: 220px;
  }

  .small {
    font-size: 13px;
  }

  .tiny {
    font-size: 12px;
  }

  .item-row {
    padding: 14px 18px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    flex-wrap: wrap;
  }
</style>
