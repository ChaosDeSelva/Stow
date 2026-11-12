<script lang="ts">
  import { ui } from "../stores/ui.svelte";
  import Icon from "./Icon.svelte";

  const ICON = { error: "triangle-exclamation", info: "circle-info", success: "circle-check" } as const;
  const TONE = { error: "red", info: "blue", success: "green" } as const;
  const errors = $derived(ui.toasts.filter((t) => t.kind === "error"));
  const others = $derived(ui.toasts.filter((t) => t.kind !== "error"));
</script>

{#snippet toastView(t: (typeof ui.toasts)[number])}
  <div class="toast banner {TONE[t.kind]}">
    <span class="toast-icon"><Icon name={ICON[t.kind]} size={15} /></span>
    <div class="body">
      <span class="msg">{t.message}{#if t.count > 1}<span class="count"> ×{t.count}</span>{/if}</span>
      {#if t.detail}
        <details class="err-details">
          <summary>Details</summary>
          <pre>{t.detail}</pre>
        </details>
      {/if}
      {#if t.actions?.length}
        <div class="actions">
          {#each t.actions as a}
            <button
              type="button"
              class="small"
              class:ghost={a.secondary}
              onclick={() => {
                a.run();
                ui.toast.dismiss(t.id);
              }}>{a.label}</button
            >
          {/each}
        </div>
      {/if}
    </div>
    <button type="button" class="ghost icon small close" aria-label="Dismiss" onclick={() => ui.toast.dismiss(t.id)}
      ><Icon name="xmark" size={12} /></button
    >
  </div>
{/snippet}

<div class="toasts">
  <div role="alert" class="stack">
    {#each errors as t (t.id)}{@render toastView(t)}{/each}
  </div>
  <div role="status" aria-live="polite" class="stack">
    {#each others as t (t.id)}{@render toastView(t)}{/each}
  </div>
</div>

<style>
  .toasts {
    position: fixed;
    right: 14px;
    bottom: 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 1000;
    max-width: 400px;
    pointer-events: none;
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: 8px;
    border: 1px solid var(--hairline);
    background: var(--surface-card);
    font-weight: 500;
    text-align: left;
    padding: 10px 6px 10px 14px;
  }
  /* Neutral card for every kind; the icon alone carries the tone. */
  .toast-icon {
    flex: none;
    display: flex;
    align-items: center;
    height: 1.45em;
  }
  .toast.red .toast-icon {
    color: var(--accent-red);
  }
  .toast.blue .toast-icon {
    color: var(--accent-blue);
  }
  .toast.green .toast-icon {
    color: var(--accent-green);
  }
  .body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .msg {
    user-select: text;
    -webkit-user-select: text;
    color: var(--ink);
  }
  .count {
    color: var(--mute);
    font-weight: 600;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .close {
    flex: none;
    margin-top: -4px;
  }
</style>
