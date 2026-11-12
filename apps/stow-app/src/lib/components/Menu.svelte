<script lang="ts" module>
  export interface MenuItem {
    label: string;
    /** Font Awesome solid icon name (without `fa-`). */
    icon?: string;
    onSelect?: () => void;
    danger?: boolean;
    disabled?: boolean;
    checked?: boolean;
    children?: MenuItem[];
    separatorAfter?: boolean;
  }

  /** Anchor for a menu opened from `e`: the pointer, or the trigger's rect when opened by keyboard (detail 0). */
  export function menuAnchor(e: MouseEvent): { x: number; y: number } {
    const t = e.currentTarget as HTMLElement | null;
    if (e.detail === 0 && t?.getBoundingClientRect) {
      const r = t.getBoundingClientRect();
      return { x: r.left, y: r.bottom + 2 };
    }
    return { x: e.clientX, y: e.clientY };
  }
</script>

<script lang="ts">
  // Popover menu anchored at a point. Roving focus (arrows, Home/End), ArrowRight/Enter
  // opens a submenu, ArrowLeft/Esc closes it, focus returns to the trigger on close.
  import { onMount, tick } from "svelte";
  import { rememberFocus, rovingIndex } from "../focus";
  import Icon from "./Icon.svelte";
  interface Props {
    items: MenuItem[];
    x: number;
    y: number;
    onClose: () => void;
    label?: string;
  }
  let { items, x, y, onClose, label = "Actions" }: Props = $props();

  let el = $state<HTMLDivElement | null>(null);
  let subEl = $state<HTMLDivElement | null>(null);
  let openSub = $state<number | null>(null);
  let clamped = $state<{ left: number; top: number } | null>(null);
  let subPos = $state<{ left: number; top: number } | null>(null);
  const pos = $derived(clamped ?? { left: x, top: y });
  const restore = rememberFocus();

  function rows(root: HTMLElement | null): HTMLElement[] {
    return root ? Array.from(root.querySelectorAll<HTMLElement>(":scope > [role=menuitem]")) : [];
  }

  onMount(() => {
    if (el) {
      const r = el.getBoundingClientRect();
      const left = Math.min(x, window.innerWidth - r.width - 8);
      const top = Math.min(y, window.innerHeight - r.height - 8);
      clamped = { left: Math.max(4, left), top: Math.max(4, top) };
      rows(el)[0]?.focus();
    }
    // Defer so the click that opened the menu does not immediately close it.
    const t = setTimeout(() => {
      document.addEventListener("pointerdown", onDocPointer, true);
      window.addEventListener("blur", close);
    }, 0);
    return () => {
      clearTimeout(t);
      document.removeEventListener("pointerdown", onDocPointer, true);
      window.removeEventListener("blur", close);
    };
  });

  function close() {
    onClose();
    restore();
  }

  function onDocPointer(e: PointerEvent) {
    const t = e.target as Node;
    if (el?.contains(t) || subEl?.contains(t)) return;
    onClose(); // pointer moved elsewhere: don't steal focus back
  }

  async function showSub(i: number, focus: boolean) {
    openSub = i;
    await tick();
    const row = rows(el)[i];
    if (row && subEl) {
      const r = row.getBoundingClientRect();
      const s = subEl.getBoundingClientRect();
      let left = r.right - 2;
      if (left + s.width > window.innerWidth - 4) left = Math.max(4, r.left - s.width + 2);
      const top = Math.max(4, Math.min(r.top - 5, window.innerHeight - s.height - 8));
      subPos = { left, top };
    }
    if (focus) rows(subEl)[0]?.focus();
  }

  function pick(item: MenuItem, i?: number) {
    if (item.disabled) return;
    if (item.children) {
      if (i != null) void showSub(i, true);
      return;
    }
    item.onSelect?.();
    close();
  }

  function onKey(e: KeyboardEvent, inSub: boolean) {
    const root = inSub ? subEl : el;
    const list = rows(root);
    const i = list.indexOf(document.activeElement as HTMLElement);
    const n = rovingIndex(e.key, i, list.length);
    if (n != null) {
      e.preventDefault();
      list[n]?.focus();
      return;
    }
    const source = inSub && openSub != null ? (items[openSub].children ?? []) : items;
    if (e.key === "Escape" || (inSub && e.key === "ArrowLeft")) {
      e.preventDefault();
      e.stopPropagation();
      if (inSub) {
        const parent = openSub;
        openSub = null;
        if (parent != null) rows(el)[parent]?.focus();
      } else close();
    } else if ((e.key === "ArrowRight" || e.key === "Enter" || e.key === " ") && !inSub && i >= 0 && items[i]?.children) {
      e.preventDefault();
      void showSub(i, true);
    } else if ((e.key === "Enter" || e.key === " ") && i >= 0 && source[i]) {
      e.preventDefault();
      pick(source[i], i);
    } else if (e.key === "Tab") {
      e.preventDefault();
      close();
    }
  }
</script>

<div class="menu floating" bind:this={el} style="left:{pos.left}px; top:{pos.top}px" role="menu" aria-label={label} tabindex="-1" onkeydown={(e) => onKey(e, false)}>
  {#each items as item, i}
    <!-- svelte-ignore a11y_click_events_have_key_events (keys are handled by the menu's roving keydown) -->
    <div
      class="item"
      class:danger={item.danger}
      class:disabled={item.disabled}
      class:open={openSub === i}
      role="menuitem"
      tabindex="-1"
      aria-disabled={item.disabled || undefined}
      aria-haspopup={item.children ? "menu" : undefined}
      aria-expanded={item.children ? openSub === i : undefined}
      onmouseenter={() => (item.children ? showSub(i, false) : (openSub = null))}
      onclick={() => pick(item, i)}
    >
      <span class="check">{#if item.checked}<Icon name="check" size={11} />{:else if item.icon}<Icon name={item.icon} fw size={12} />{/if}</span>
      <span class="label">{item.label}</span>
      {#if item.children}<span class="arrow"><Icon name="chevron-right" size={10} /></span>{/if}
    </div>
    {#if item.separatorAfter}<div class="sep" role="separator"></div>{/if}
  {/each}
</div>

{#if openSub != null && items[openSub]?.children}
  {@const kids = items[openSub].children ?? []}
  <div
    class="menu floating sub"
    bind:this={subEl}
    role="menu"
    aria-label={items[openSub].label}
    tabindex="-1"
    style="left:{subPos?.left ?? -9999}px; top:{subPos?.top ?? -9999}px"
    onkeydown={(e) => onKey(e, true)}
  >
    {#each kids as child, ci}
      <!-- svelte-ignore a11y_click_events_have_key_events (keys are handled by the menu's roving keydown) -->
      <div
        class="item"
        class:danger={child.danger}
        class:disabled={child.disabled}
        role="menuitem"
        tabindex="-1"
        aria-disabled={child.disabled || undefined}
        onclick={() => pick(child, ci)}
      >
        <span class="check">{#if child.checked}<Icon name="check" size={11} label="Current" />{:else if child.icon}<Icon name={child.icon} fw size={12} />{/if}</span>
        <span class="label">{child.label}</span>
      </div>
    {/each}
    {#if !kids.length}
      <div class="item disabled" role="menuitem" aria-disabled="true" tabindex="-1"><span class="check"></span><span class="label">Nothing here</span></div>
    {/if}
  </div>
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 900;
    min-width: 180px;
    max-height: 70vh;
    overflow: auto;
    padding: 4px;
    font-size: 13px;
    color: var(--ink);
  }
  .menu:focus {
    outline: none;
  }
  .menu.sub {
    z-index: 901;
  }
  .item {
    position: relative;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 10px 6px 4px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
    font-weight: 500;
  }
  .item:hover,
  .item:focus,
  .item.open {
    background: var(--surface-soft);
    outline: none;
  }
  .item:focus-visible {
    box-shadow: inset 0 0 0 2px var(--accent-blue);
  }
  .item.disabled {
    color: var(--mute);
  }
  .item.disabled:hover {
    background: transparent;
  }
  .item.danger {
    color: var(--danger-text);
  }
  .item.danger:hover,
  .item.danger:focus {
    background: var(--accent-red-soft);
  }
  .check {
    width: 18px;
    display: inline-grid;
    place-items: center;
    color: var(--mute);
  }
  .item:hover .check,
  .item.danger .check {
    color: inherit;
  }
  .label {
    flex: 1;
  }
  .arrow {
    margin-left: 12px;
    color: var(--mute);
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--hairline-soft);
  }
</style>
