<script lang="ts">
  // Modal dialog: focus trap, Esc closes, focus returns to the opener.
  import { onMount, type Snippet } from "svelte";
  import { focusables, rememberFocus, trapTab } from "../focus";
  interface Props {
    title: string;
    onClose: () => void;
    children: Snippet;
    actions?: Snippet;
    width?: number;
    /** alertdialog for confirmations. */
    role?: "dialog" | "alertdialog";
  }
  let { title, onClose, children, actions, width = 460, role = "dialog" }: Props = $props();
  const id = $props.id();
  let el = $state<HTMLDivElement | null>(null);
  const restore = rememberFocus();

  onMount(() => {
    queueMicrotask(() => (focusables(el)[0] ?? el)?.focus());
    return restore;
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      e.preventDefault();
      onClose();
    } else trapTab(e, el);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" onmousedown={(e) => e.target === e.currentTarget && onClose()}>
  <div
    class="dialog"
    {role}
    aria-modal="true"
    aria-labelledby="{id}-title"
    tabindex="-1"
    style="width:{width}px"
    bind:this={el}
    onkeydown={onKey}
  >
    <h3 id="{id}-title">{title}</h3>
    <div class="content">{@render children()}</div>
    {#if actions}<div class="row actions"><span class="spacer"></span>{@render actions()}</div>{/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 800;
    background: var(--scrim);
    display: grid;
    place-items: center;
  }
  .dialog {
    max-width: calc(100vw - 40px);
    max-height: calc(100vh - 60px);
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 24px;
    border-radius: var(--radius);
    background: var(--surface-card);
    border: 1px solid var(--stone);
  }
  .dialog:focus {
    outline: none;
  }
  h3 {
    margin: 0;
    font-size: 18px;
  }
  .content {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
</style>
