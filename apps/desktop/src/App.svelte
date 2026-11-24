<script lang="ts">
  import { onMount } from "svelte";
  import {
    getStatus,
    isTauriRuntime,
    onCoreEvent,
    pingCore,
    type StatusSnapshot,
  } from "./lib/api";

  let status = $state<StatusSnapshot | null>(null);
  let events = $state<string[]>([]);
  let error = $state<string | null>(null);
  let busy = $state(false);

  function formatGb(mb: number): string {
    return `${(mb / 1024).toFixed(1)} GB`;
  }

  async function sendPing() {
    if (!isTauriRuntime()) {
      events = [...events, "Preview mode: open inside BlockHost desktop to talk to Rust core."];
      return;
    }
    busy = true;
    error = null;
    try {
      status = await pingCore();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    let unlisten: (() => void) | undefined;

    if (!isTauriRuntime()) {
      events = ["Browser preview mode (Tauri backend not attached)."];
      return;
    }

    void (async () => {
      try {
        unlisten = await onCoreEvent((ev) => {
          if (ev.kind === "status_changed") {
            status = ev;
            events = [...events.slice(-19), `Status update received (v${ev.app_version})`];
          } else if (ev.kind === "log_line") {
            events = [...events.slice(-19), ev.line];
          } else if (ev.kind === "alert") {
            events = [...events.slice(-19), `[${ev.level}] ${ev.message}`];
          }
        });
        status = await getStatus();
      } catch (e) {
        error = e instanceof Error ? e.message : String(e);
      }
    })();

    return () => {
      unlisten?.();
    };
  });
</script>

<main class="shell">
  <header class="hero">
    <div>
      <h1>BlockHost</h1>
      <p class="subtitle">
        One-click 24/7 Minecraft server host &mdash; M0 walking skeleton
      </p>
    </div>
    <button class="primary" onclick={sendPing} disabled={busy}>
      {busy ? "Checking..." : "Ping Core"}
    </button>
  </header>

  {#if error}
    <section class="alert" role="alert">
      <strong>Something went wrong:</strong> {error}
    </section>
  {/if}

  {#if status}
    <section class="grid" aria-label="Detected hardware and server state">
      <article class="card">
        <h2>This PC</h2>
        <dl>
          <div><dt>Memory</dt><dd>{formatGb(status.hardware.ram_mb)}</dd></div>
          <div><dt>CPU cores</dt><dd>{status.hardware.cores}</dd></div>
          <div><dt>Recommended RAM</dt><dd>{formatGb(status.hardware.recommended_heap_mb)} ({status.hardware.tier})</dd></div>
          <div><dt>Suggested players</dt><dd>Up to {status.hardware.recommended_max_players}</dd></div>
        </dl>
        {#if status.hardware.warning}
          <p class="warn">{status.hardware.warning}</p>
        {/if}
      </article>

      <article class="card">
        <h2>Server Status</h2>
        <dl>
          <div><dt>State</dt><dd>{status.running ? `Running (${status.running})` : "Stopped"}</dd></div>
          <div><dt>Servers found</dt><dd>{status.servers.length}</dd></div>
          <div><dt>Data folder</dt><dd class="mono">{status.data_dir}</dd></div>
        </dl>
        <p class="hint">
          Closing this window keeps BlockHost running quietly in the system tray to save RAM.
        </p>
      </article>
    </section>
  {/if}

  <section class="card" aria-label="Live event stream">
    <h2>Live Core Events</h2>
    {#if events.length === 0}
      <p class="muted">Click "Ping Core" to verify the Tauri command &rarr; live event loop.</p>
    {:else}
      <ul class="log-list">
        {#each events as line, i (i)}
          <li class="mono">{line}</li>
        {/each}
      </ul>
    {/if}
  </section>
</main>

<style>
  .shell {
    max-width: 920px;
    margin: 0 auto;
    padding: 28px 24px 40px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .hero {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  h1 {
    margin: 0;
    font-size: 1.75rem;
    letter-spacing: -0.02em;
  }

  h2 {
    margin: 0 0 12px;
    font-size: 1rem;
  }

  .subtitle {
    margin: 4px 0 0;
    color: var(--muted);
  }

  .primary {
    border: none;
    border-radius: 10px;
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 600;
    padding: 10px 18px;
  }

  .primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .primary:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 16px;
  }

  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 18px 20px;
  }

  dl {
    margin: 0;
    display: grid;
    gap: 8px;
  }

  dl > div {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }

  dt {
    color: var(--muted);
  }

  dd {
    margin: 0;
    font-weight: 600;
    text-align: right;
  }

  .mono {
    font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
    font-size: 0.85rem;
    word-break: break-all;
  }

  .warn,
  .alert {
    margin: 12px 0 0;
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--warn-bg);
    color: var(--warn-text);
    font-size: 0.9rem;
  }

  .hint,
  .muted {
    margin: 12px 0 0;
    color: var(--muted);
    font-size: 0.9rem;
  }

  .log-list {
    margin: 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
</style>
