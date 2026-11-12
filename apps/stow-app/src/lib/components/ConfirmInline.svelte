<script lang="ts">
  // Small inline confirmation strip (no window.confirm). role=alertdialog, focuses
  // Cancel (the safe choice), Esc cancels, focus returns to the opener afterwards.
  import { onMount } from "svelte";
  import { rememberFocus } from "../focus";
  import Icon from "./Icon.svelte";
  interface Props {
    message: string;
    confirmLabel?: string;
    onConfirm: () => void;
    onCancel: () => void;
  }
  let { message, confirmLabel = "Delete", onConfirm, onCancel }: Props = $props();
  const id = $props.id();
  let cancelBtn = $state<HTMLButtonElement | null>(null);
  const restore = rememberFocus();

  onMount(() => {
    cancelBtn?.focus();
  });

  function done(fn: () => void) {
    fn();
    restore();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      done(onCancel);
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="confirm" role="alertdialog" aria-labelledby="{id}-msg" tabindex="-1" onkeydown={onKey} onclick={(e) => e.stopPropagation()}>
  <Icon name="triangle-exclamation" class="confirm-icon" />
  <span class="msg" id="{id}-msg">{message}</span>
  <button type="button" class="small ghost" bind:this={cancelBtn} onclick={() => done(onCancel)}>Cancel</button>
  <button type="button" class="small outline-danger" onclick={() => done(onConfirm)}>{confirmLabel}</button>
</div>

<style>
  .confirm {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    padding: 8px 12px;
    border-radius: var(--radius);
    border: 1px solid var(--hairline);
    background: var(--accent-red-soft);
    color: var(--ink);
    font-size: 13px;
    font-weight: 500;
  }
  .msg {
    flex: 1;
    min-width: 0;
  }
</style>
