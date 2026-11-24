<script lang="ts">
  import { api, type LiveMetrics, type PlayersState, type StatusSnapshot } from "../lib/api";
  import ConfirmModal from "./ConfirmModal.svelte";

  interface Props {
    status: StatusSnapshot;
    metrics: LiveMetrics | null;
    onNotify: (msg: string, kind?: "info" | "error") => void;
  }

  let { status, metrics, onNotify }: Props = $props();

  let players = $state<PlayersState>({ allowed: [], banned: [], whitelist_enabled: true });
  let newUsername = $state("");
  let makeAdmin = $state(false);
  let busy = $state(false);
  let kickReason = $state("");
  let banTarget = $state<string | null>(null);
  let tab = $state<"allowed" | "banned">("allowed");

  const serverName = $derived(status.current ?? "");
  const isRunning = $derived(status.running !== null && status.running === serverName);
  const onlineSet = $derived(new Set((metrics?.online_players ?? []).map((n) => n.toLowerCase())));

  async function refresh() {
    if (!serverName) return;
    try {
      players = await api.listPlayers(serverName);
    } catch (e) {
      onNotify(String(e), "error");
    }
  }

  $effect(() => {
    void serverName;
    void refresh();
  });

  async function add() {
    const u = newUsername.trim();
    if (!u || !serverName) return;
    busy = true;
    try {
      const msg = await api.addPlayer(serverName, u, makeAdmin);
      newUsername = "";
      makeAdmin = false;
      onNotify(msg);
      await refresh();
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function toggleAdmin(name: string, currentlyAdmin: boolean) {
    if (!serverName) return;
    busy = true;
    try {
      const msg = currentlyAdmin
        ? await api.removePlayer(serverName, name, true)
        : await api.addPlayer(serverName, name, true);
      onNotify(msg);
      await refresh();
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function removeAllowed(name: string) {
    if (!serverName) return;
    busy = true;
    try {
      const msg = await api.removePlayer(serverName, name, false);
      onNotify(msg);
      await refresh();
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function runAction(action: string, username: string, arg?: string) {
    busy = true;
    try {
      const msg = await api.playerAdminAction(action, username, arg);
      onNotify(msg || "Done");
      await refresh();
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function confirmBan() {
    if (!banTarget) return;
    const target = banTarget;
    banTarget = null;
    await runAction("ban", target, kickReason.trim() || undefined);
    kickReason = "";
  }
</script>

<div class="stack">
  <div class="row-between">
    <div class="stack-xs">
      <h1 class="page-title">Players and admin</h1>
      <p class="muted">
        {players.whitelist_enabled
          ? "Only allowed players can join this server."
          : "Whitelist is currently off so anyone with the address can join."}
      </p>
    </div>

    <div class="row">
      <button
        class={tab === "allowed" ? "btn btn-primary btn-sm" : "btn btn-sm"}
        onclick={() => (tab = "allowed")}
      >
        Allowed ({players.allowed.length})
      </button>
      <button
        class={tab === "banned" ? "btn btn-primary btn-sm" : "btn btn-sm"}
        onclick={() => (tab = "banned")}
      >
        Banned ({players.banned.length})
      </button>
    </div>
  </div>

  <form
    class="card row"
    onsubmit={(e) => {
      e.preventDefault();
      add();
    }}
  >
    <input
      class="input grow"
      bind:value={newUsername}
      placeholder="Minecraft username to invite"
      maxlength="16"
    />
    <label class="row small">
      <input type="checkbox" bind:checked={makeAdmin} />
      <span>Make admin</span>
    </label>
    <button class="btn btn-primary" type="submit" disabled={busy || !newUsername.trim()}>
      Add player
    </button>
  </form>

  {#if isRunning}
    <div class="card row">
      <input
        class="input grow"
        bind:value={kickReason}
        placeholder="Optional reason for kick or ban actions"
      />
      <span class="muted tiny">Online right now: {metrics?.online_players.length ?? 0}</span>
    </div>
  {/if}

  {#if tab === "allowed"}
    {#if players.allowed.length === 0}
      <div class="card">
        <p class="muted">Nobody is on the allowed list yet. Add your Minecraft username above.</p>
      </div>
    {:else}
      <div class="stack-sm">
        {#each players.allowed as p (p.name)}
          {@const online = onlineSet.has(p.name.toLowerCase())}
          <div class="card player-row">
            <div class="row">
              <strong>{p.name}</strong>
              {#if online}
                <span class="badge badge-green">Online</span>
              {/if}
              {#if p.is_admin}
                <span class="badge badge-blue">Admin</span>
              {/if}
            </div>

            <div class="row">
              {#if isRunning && online}
                <select
                  class="select mode-select"
                  aria-label="Change game mode for {p.name}"
                  onchange={(e) => {
                    const val = (e.currentTarget as HTMLSelectElement).value;
                    if (val) runAction("gamemode", p.name, val);
                    (e.currentTarget as HTMLSelectElement).value = "";
                  }}
                >
                  <option value="">Game mode</option>
                  <option value="survival">Survival</option>
                  <option value="creative">Creative</option>
                  <option value="adventure">Adventure</option>
                  <option value="spectator">Spectator</option>
                </select>
                {#if (metrics?.online_players.length ?? 0) > 1}
                  <select
                    class="select mode-select"
                    aria-label="Teleport {p.name} to player"
                    onchange={(e) => {
                      const target = (e.currentTarget as HTMLSelectElement).value;
                      if (target) runAction("tp", p.name, target);
                      (e.currentTarget as HTMLSelectElement).value = "";
                    }}
                  >
                    <option value="">Teleport to</option>
                    {#each metrics?.online_players ?? [] as other (other)}
                      {#if other.toLowerCase() !== p.name.toLowerCase()}
                        <option value={other}>{other}</option>
                      {/if}
                    {/each}
                  </select>
                {/if}
                <button
                  class="btn btn-sm"
                  disabled={busy}
                  onclick={() => runAction("heal", p.name)}
                >
                  Heal
                </button>
                <button
                  class="btn btn-sm"
                  disabled={busy}
                  onclick={() => runAction("feed", p.name)}
                >
                  Feed
                </button>
                <button
                  class="btn btn-sm"
                  disabled={busy}
                  onclick={() => runAction("kick", p.name, kickReason.trim() || undefined)}
                >
                  Kick
                </button>
              {/if}

              <button
                class="btn btn-sm"
                disabled={busy}
                onclick={() => toggleAdmin(p.name, p.is_admin)}
              >
                {p.is_admin ? "Remove admin" : "Make admin"}
              </button>

              {#if isRunning}
                <button
                  class="btn btn-danger-soft btn-sm"
                  disabled={busy}
                  onclick={() => (banTarget = p.name)}
                >
                  Ban
                </button>
              {/if}

              <button
                class="btn btn-danger-soft btn-sm"
                disabled={busy}
                onclick={() => removeAllowed(p.name)}
              >
                Remove
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {:else if players.banned.length === 0}
    <div class="card">
      <p class="muted">No banned players.</p>
    </div>
  {:else}
    <div class="stack-sm">
      {#each players.banned as b (b.name)}
        <div class="card player-row">
          <div class="stack-xs">
            <strong>{b.name}</strong>
            {#if b.reason}
              <span class="muted tiny">{b.reason}</span>
            {/if}
          </div>
          <button
            class="btn btn-sm"
            disabled={busy || !isRunning}
            onclick={() => runAction("pardon", b.name)}
          >
            Unban
          </button>
        </div>
      {/each}
    </div>
  {/if}
</div>

<ConfirmModal
  open={banTarget !== null}
  title="Ban player"
  message="Ban {banTarget ?? ''} from joining this server? You can unban them later from the Banned tab."
  confirmLabel="Ban player"
  danger={true}
  onConfirm={confirmBan}
  onCancel={() => (banTarget = null)}
/>

<style>
  .stack-xs {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .grow {
    flex: 1;
    min-width: 200px;
  }

  .small {
    font-size: 13px;
  }

  .tiny {
    font-size: 12px;
  }

  .player-row {
    padding: 14px 18px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }

  .mode-select {
    width: auto;
    padding: 6px 10px;
    font-size: 13px;
  }
</style>
