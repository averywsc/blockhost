<script lang="ts">
  import { api, formatSize, type StatusSnapshot } from "../lib/api";
  import ConfirmModal from "./ConfirmModal.svelte";

  interface Props {
    status: StatusSnapshot;
    onNotify: (msg: string, kind?: "info" | "error") => void;
  }

  let { status, onNotify }: Props = $props();

  let busy = $state(false);
  let restoreTarget = $state<string | null>(null);

  const serverName = $derived(status.current ?? "");
  const isRunning = $derived(status.running !== null && status.running === serverName);
  const totalBackupBytes = $derived(
    status.backups.reduce((sum, b) => sum + b.size_bytes, 0)
  );

  async function backUpNow() {
    if (!serverName) return;
    busy = true;
    try {
      await api.backupNow(serverName);
      onNotify("Backup started. It will appear below once finished.");
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function confirmRestore() {
    if (!serverName || !restoreTarget) return;
    const file = restoreTarget;
    restoreTarget = null;
    busy = true;
    try {
      const msg = await api.restoreBackup(serverName, file);
      onNotify(msg);
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
      <h1 class="page-title">Backups</h1>
      <p class="muted">
        {status.backup_hours && status.backup_hours > 0
          ? `Automatic backup every ${status.backup_hours} hours while running. Keeps the latest 10.`
          : "Automatic timed backups are off. A backup is still made on every server start."}
      </p>
    </div>

    <div class="row">
      <button class="btn" onclick={() => api.openFolder(serverName, "backups")}>Open folder</button>
      <button class="btn btn-primary" disabled={busy} onclick={backUpNow}>Back up now</button>
    </div>
  </div>

  <div class="row">
    <div class="card stat">
      <span class="muted tiny">Current world</span>
      <strong>{formatSize(status.world_size_bytes)}</strong>
    </div>
    <div class="card stat">
      <span class="muted tiny">All backups ({status.backups.length})</span>
      <strong>{formatSize(totalBackupBytes)}</strong>
    </div>
  </div>

  {#if status.backups.length === 0}
    <div class="card">
      <p class="muted">No backups yet. One is saved automatically each time you start the server.</p>
    </div>
  {:else}
    <div class="stack-sm">
      {#each status.backups as bk (bk.filename)}
        <div class="card bk-row">
          <div class="stack-xs">
            <strong class="mono">{bk.filename}</strong>
            <span class="muted tiny">{formatSize(bk.size_bytes)}</span>
          </div>
          <button
            class="btn btn-sm"
            disabled={busy || isRunning}
            title={isRunning ? "Stop the server before restoring" : "Restore this backup"}
            onclick={() => (restoreTarget = bk.filename)}
          >
            Restore
          </button>
        </div>
      {/each}
    </div>
  {/if}

  {#if isRunning}
    <p class="muted tiny">Stop the server first if you want to restore an older backup.</p>
  {/if}
</div>

<ConfirmModal
  open={restoreTarget !== null}
  title="Restore backup"
  message="Restoring {restoreTarget ?? ''} replaces your active world. Your current world is never deleted and will be moved safely into a pre-restore folder."
  confirmLabel="Restore backup"
  requireYes={true}
  danger={true}
  onConfirm={confirmRestore}
  onCancel={() => (restoreTarget = null)}
/>

<style>
  .stack-xs {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .tiny {
    font-size: 12px;
  }

  .stat {
    min-width: 170px;
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .bk-row {
    padding: 14px 18px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
</style>
