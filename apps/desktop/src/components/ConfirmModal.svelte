<script lang="ts">
  interface Props {
    open: boolean;
    title: string;
    message: string;
    confirmLabel?: string;
    requireYes?: boolean;
    danger?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  }

  let {
    open,
    title,
    message,
    confirmLabel = "Confirm",
    requireYes = false,
    danger = false,
    onConfirm,
    onCancel,
  }: Props = $props();

  let typed = $state("");

  $effect(() => {
    if (open) typed = "";
  });

  const canConfirm = $derived(!requireYes || typed.trim() === "YES");
</script>

{#if open}
  <div class="overlay" role="dialog" aria-modal="true" aria-labelledby="confirm-title">
    <div class="modal stack">
      <h2 id="confirm-title" class="section-title">{title}</h2>
      <p class="muted">{message}</p>

      {#if requireYes}
        <label class="stack-sm">
          <span class="label">Type YES to continue</span>
          <input
            class="input mono"
            bind:value={typed}
            placeholder="YES"
            autocomplete="off"
          />
        </label>
      {/if}

      <div class="row actions">
        <button class="btn" onclick={onCancel}>Cancel</button>
        <button
          class={danger ? "btn btn-danger" : "btn btn-primary"}
          disabled={!canConfirm}
          onclick={onConfirm}
        >
          {confirmLabel}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.56);
    backdrop-filter: blur(4px);
    display: grid;
    place-items: center;
    padding: 20px;
    z-index: 60;
  }

  .modal {
    width: 100%;
    max-width: 440px;
    background: var(--surface);
    border-radius: var(--radius);
    padding: 24px;
    box-shadow: var(--shadow-lg);
  }

  .label {
    font-size: 13px;
    font-weight: 600;
  }

  .actions {
    justify-content: flex-end;
    margin-top: 4px;
  }
</style>
