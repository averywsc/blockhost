<script lang="ts">
  import { api, type StatusSnapshot } from "../lib/api";

  interface Props {
    status: StatusSnapshot;
    logs: string[];
    onNotify: (msg: string, kind?: "info" | "error") => void;
  }

  let { status, logs, onNotify }: Props = $props();

  let commandInput = $state("");
  let filter = $state("");
  let history = $state<string[]>([]);
  let historyIdx = $state(-1);
  let autoScroll = $state(true);
  let logEl = $state<HTMLDivElement | null>(null);

  const isRunning = $derived(status.running !== null);
  const filtered = $derived(
    filter.trim()
      ? logs.filter((l) => l.toLowerCase().includes(filter.trim().toLowerCase()))
      : logs
  );

  $effect(() => {
    void filtered.length;
    if (autoScroll && logEl) {
      logEl.scrollTop = logEl.scrollHeight;
    }
  });

  function lineClass(line: string): string {
    const u = line.toUpperCase();
    if (u.includes("ERROR") || u.includes("FATAL") || u.includes("EXCEPTION")) return "log-err";
    if (u.includes("WARN")) return "log-warn";
    if (line.startsWith("[blockhost]") || line.startsWith("[setup]")) return "log-sys";
    return "log-info";
  }

  async function runCmd(raw?: string) {
    const cmd = (raw ?? commandInput).trim().replace(/^\/+/, "");
    if (!cmd) return;
    if (!raw) {
      commandInput = "";
      history = [cmd, ...history.slice(0, 49)];
      historyIdx = -1;
    }
    try {
      const reply = await api.sendCommand(cmd);
      if (reply) onNotify(reply);
    } catch (e) {
      onNotify(String(e), "error");
    }
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === "ArrowUp" && history.length > 0) {
      e.preventDefault();
      historyIdx = Math.min(historyIdx + 1, history.length - 1);
      commandInput = history[historyIdx] ?? "";
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      if (historyIdx <= 0) {
        historyIdx = -1;
        commandInput = "";
      } else {
        historyIdx -= 1;
        commandInput = history[historyIdx] ?? "";
      }
    }
  }
</script>

<div class="stack console-page">
  <div class="row-between">
    <div class="stack-xs">
      <h1 class="page-title">Console</h1>
      <p class="muted">Live server output and admin commands</p>
    </div>

    <div class="row">
      <input class="input search" bind:value={filter} placeholder="Filter log lines" />
      <label class="row small muted">
        <input type="checkbox" bind:checked={autoScroll} />
        <span>Auto scroll</span>
      </label>
    </div>
  </div>

  <div class="row">
    <button class="btn btn-sm" disabled={!isRunning} onclick={() => runCmd("list")}>Who is online</button>
    <button class="btn btn-sm" disabled={!isRunning} onclick={() => runCmd("tps")}>Check TPS</button>
    <button class="btn btn-sm" disabled={!isRunning} onclick={() => runCmd("save-all")}>Save world</button>
    <button class="btn btn-sm" disabled={!isRunning} onclick={() => runCmd("time set day")}>Daytime</button>
    <button class="btn btn-sm" disabled={!isRunning} onclick={() => runCmd("weather clear")}>Clear weather</button>
  </div>

  <div class="card log-box mono" bind:this={logEl} role="log" aria-live="polite">
    {#if filtered.length === 0}
      <p class="muted">No console output yet. Start the server to see live logs here.</p>
    {:else}
      {#each filtered as line, i (i)}
        <div class={`log-line ${lineClass(line)}`}>{line}</div>
      {/each}
    {/if}
  </div>

  <form
    class="row"
    onsubmit={(e) => {
      e.preventDefault();
      runCmd();
    }}
  >
    <input
      class="input mono grow"
      bind:value={commandInput}
      onkeydown={handleKey}
      disabled={!isRunning}
      placeholder={isRunning ? "Type any Minecraft command without /" : "Start the server to run commands"}
    />
    <button class="btn btn-primary" type="submit" disabled={!isRunning || !commandInput.trim()}>
      Send
    </button>
  </form>
</div>

<style>
  .console-page {
    height: calc(100vh - 56px);
  }

  .stack-xs {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .search {
    width: 220px;
  }

  .small {
    font-size: 13px;
  }

  .grow {
    flex: 1;
  }

  .log-box {
    flex: 1;
    min-height: 280px;
    overflow-y: auto;
    padding: 16px 18px;
    font-size: 12.5px;
    line-height: 1.55;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .log-line {
    white-space: pre-wrap;
    word-break: break-word;
  }

  .log-err {
    color: var(--danger-text);
  }

  .log-warn {
    color: var(--warn-text);
  }

  .log-sys {
    color: var(--accent);
  }

  .log-info {
    color: var(--text);
  }
</style>
