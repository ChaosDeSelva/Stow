<script lang="ts">
  import { formPresets, fromLocalInputValue, toLocalInputValue } from "../time";

  interface Props {
    onSubmit: (fireAt: string, message: string | null) => void | Promise<unknown>;
    onCancel?: () => void;
    /** Prefilled message (the target's title). */
    defaultMessage?: string;
  }
  let { onSubmit, onCancel, defaultMessage = "" }: Props = $props();
  const id = $props.id();

  let when = $state(toLocalInputValue(new Date(Date.now() + 3600_000).toISOString()));
  let message = $state((() => defaultMessage)());
  let busy = $state(false);

  const quick = $derived(formPresets(new Date()));
  const valid = $derived(fromLocalInputValue(when) != null);

  async function submit() {
    const iso = fromLocalInputValue(when);
    if (!iso) return;
    busy = true;
    try {
      await onSubmit(iso, message.trim() || null);
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    // Enter on a button activates that button (a preset), not "Add reminder".
    if ((e.target as HTMLElement).tagName === "BUTTON") return;
    if (e.key === "Enter" && !e.isComposing) {
      e.preventDefault();
      void submit();
    } else if (e.key === "Escape" && onCancel) {
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="form" onkeydown={onKey}>
  <div class="row quick" role="group" aria-label="Quick times">
    {#each quick as p}
      <button type="button" class="small" aria-pressed={toLocalInputValue(p.fireAt) === when} onclick={() => (when = toLocalInputValue(p.fireAt))}>{p.label}</button>
    {/each}
  </div>
  <div class="row">
    <label class="sr-only" for="{id}-when">When</label>
    <input id="{id}-when" type="datetime-local" bind:value={when} />
    <label class="sr-only" for="{id}-msg">Message</label>
    <input id="{id}-msg" type="text" class="msg" placeholder="Message (optional)" bind:value={message} />
  </div>
  <div class="row">
    <span class="spacer"></span>
    {#if onCancel}<button type="button" class="small ghost" onclick={onCancel}>Cancel</button>{/if}
    <button type="button" class="small" disabled={!valid || busy} onclick={submit}>Add reminder</button>
  </div>
</div>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: var(--surface-card);
  }
  .quick {
    flex-wrap: wrap;
    gap: 6px;
  }
  .quick button[aria-pressed="true"] {
    border-color: var(--ink);
  }
  .msg {
    flex: 1;
    min-width: 0;
  }
</style>
