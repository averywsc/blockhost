<script lang="ts">
  import { api, type GameRules, type StatusSnapshot } from "../lib/api";
  import ConfirmModal from "./ConfirmModal.svelte";

  interface Props {
    status: StatusSnapshot;
    onNotify: (msg: string, kind?: "info" | "error") => void;
  }

  let { status, onNotify }: Props = $props();

  let rules = $state<GameRules | null>(null);
  let seedInput = $state("");
  let confirmNewWorld = $state(false);
  let busy = $state(false);

  const serverName = $derived(status.current ?? "");
  const isRunning = $derived(status.running !== null && status.running === serverName);

  async function refresh() {
    if (!serverName) return;
    try {
      rules = await api.getGameRules(serverName);
    } catch (e) {
      onNotify(String(e), "error");
    }
  }

  $effect(() => {
    void serverName;
    void refresh();
  });

  async function setRule(rule: string, value: string, message: string) {
    if (!serverName) return;
    busy = true;
    try {
      rules = await api.updateGameRule(serverName, rule, value);
      onNotify(message);
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function doNewWorld() {
    if (!serverName) return;
    confirmNewWorld = false;
    busy = true;
    try {
      const msg = await api.createNewWorld(serverName, seedInput.trim());
      seedInput = "";
      onNotify(msg);
      await refresh();
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function uploadWorld() {
    if (!serverName) return;
    busy = true;
    try {
      const msg = await api.importWorldZip(serverName);
      if (msg) {
        onNotify(msg);
        await refresh();
      }
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function exportWorld() {
    if (!serverName) return;
    busy = true;
    try {
      const msg = await api.exportWorldZip(serverName);
      if (msg) onNotify(msg);
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
      <h1 class="page-title">World and rules</h1>
      <p class="muted">Upload a world zip, export your save, adjust rules, or start fresh safely.</p>
    </div>

    <div class="row">
      <button
        class="btn btn-primary"
        disabled={busy || isRunning}
        title={isRunning ? "Stop the server before uploading a world" : "Import a zipped Minecraft world"}
        onclick={uploadWorld}
      >
        Upload world (.zip)
      </button>
      <button class="btn" disabled={busy} onclick={exportWorld}>
        Export world (.zip)
      </button>
    </div>
  </div>

  {#if rules}
    <div class="card rules-grid">
      <label class="stack-sm">
        <span class="field-label">Difficulty</span>
        <select
          class="select"
          value={rules.difficulty}
          disabled={busy}
          onchange={(e) =>
            setRule(
              "difficulty",
              (e.currentTarget as HTMLSelectElement).value,
              isRunning ? "Difficulty updated immediately." : "Difficulty saved."
            )}
        >
          <option value="peaceful">Peaceful</option>
          <option value="easy">Easy</option>
          <option value="normal">Normal</option>
          <option value="hard">Hard</option>
        </select>
      </label>

      <label class="stack-sm">
        <span class="field-label">Default game mode</span>
        <select
          class="select"
          value={rules.gamemode}
          disabled={busy}
          onchange={(e) =>
            setRule(
              "gamemode",
              (e.currentTarget as HTMLSelectElement).value,
              "Default game mode saved. Applies on restart."
            )}
        >
          <option value="survival">Survival</option>
          <option value="creative">Creative</option>
          <option value="adventure">Adventure</option>
          <option value="spectator">Spectator</option>
        </select>
      </label>

      <label class="subcard toggle-row">
        <div class="stack-xs">
          <strong>Players can fight (PvP)</strong>
          <span class="muted tiny">Allow players to damage each other.</span>
        </div>
        <input
          type="checkbox"
          checked={rules.pvp}
          disabled={busy}
          onchange={(e) =>
            setRule(
              "pvp",
              String((e.currentTarget as HTMLInputElement).checked),
              "PvP setting updated. Applies on restart."
            )}
        />
      </label>

      <label class="subcard toggle-row">
        <div class="stack-xs">
          <strong>Command blocks</strong>
          <span class="muted tiny">Enable redstone command blocks in the world.</span>
        </div>
        <input
          type="checkbox"
          checked={rules.enable_command_block}
          disabled={busy}
          onchange={(e) =>
            setRule(
              "command_blocks",
              String((e.currentTarget as HTMLInputElement).checked),
              "Command blocks updated. Applies on restart."
            )}
        />
      </label>
    </div>

    <div class="card stack">
      <div class="row-between">
        <div class="stack-xs">
          <h2 class="section-title">Performance and view distance</h2>
          <span class="muted tiny">Lower view distance keeps old 4 GB laptops smooth.</span>
        </div>
        <strong>{rules.view_distance} chunks</strong>
      </div>

      <input
        type="range"
        min="3"
        max="24"
        value={rules.view_distance}
        disabled={busy}
        onchange={(e) =>
          setRule(
            "view_distance",
            (e.currentTarget as HTMLInputElement).value,
            "View distance saved. Applies on restart."
          )}
      />

      <div class="row">
        <button
          class="btn"
          disabled={busy}
          onclick={() =>
            setRule(
              "low_lag",
              "true",
              "Low-lag mode enabled (view 6, sim 4, async chunk writes). Restart to apply."
            )}
        >
          Low-lag mode for old laptops
        </button>
        <button
          class="btn"
          disabled={busy}
          onclick={() =>
            setRule(
              "reset_recommended",
              "true",
              "Speed settings reset to match this PC. Restart to apply."
            )}
        >
          Reset to recommended
        </button>
      </div>
    </div>

    <div class="card stack">
      <div class="stack-xs">
        <h2 class="section-title">Start a brand new world</h2>
        <p class="muted tiny">
          Your current world is backed up and moved aside into the old-worlds folder. Nothing is ever deleted.
        </p>
      </div>

      <div class="row">
        <input
          class="input grow"
          bind:value={seedInput}
          placeholder="Optional world seed (leave empty for random)"
          maxlength="40"
        />
        <button
          class="btn btn-danger-soft"
          disabled={busy || isRunning}
          onclick={() => (confirmNewWorld = true)}
        >
          Create new world
        </button>
      </div>
      {#if isRunning}
        <p class="muted tiny">Stop the server first before creating a new world.</p>
      {/if}
    </div>
  {/if}
</div>

<ConfirmModal
  open={confirmNewWorld}
  title="Start a brand new world"
  message="Your current world will be backed up and moved safely into old-worlds so nothing is lost. A fresh world will generate next time you start the server."
  confirmLabel="Start new world"
  requireYes={true}
  danger={true}
  onConfirm={doNewWorld}
  onCancel={() => (confirmNewWorld = false)}
/>

<style>
  .stack-xs {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .field-label {
    font-size: 13px;
    font-weight: 600;
  }

  .tiny {
    font-size: 12px;
  }

  .rules-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 14px;
  }

  .toggle-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    cursor: pointer;
  }

  .grow {
    flex: 1;
    min-width: 220px;
  }
</style>
