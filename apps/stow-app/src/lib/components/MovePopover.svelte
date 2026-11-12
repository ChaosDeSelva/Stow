<script lang="ts">
  // "M" in the list / "Move to…" in the bulk bar: a small popover with the
  // bucket picker, anchored near the row. Moves every id in `itemIds`.
  import { onMount } from "svelte";
  import { rememberFocus } from "../focus";
  import { moveMany } from "../itemActions";
  import { items } from "../stores/items.svelte";
  import type { Item } from "../types";
  import BucketPicker from "./BucketPicker.svelte";

  interface Props {
    itemIds: string[];
    x: number;
    y: number;
    onClose: () => void;
  }
  let { itemIds, x, y, onClose }: Props = $props();
  let el = $state<HTMLDivElement | null>(null);
  const restore = rememberFocus();
  /** The shared bucket when every item is in the same one, else undefined (nothing ticked). */
  const current = $derived.by((): string | null | undefined => {
    const ids = itemIds.map((id) => items.get(id)?.bucket_id ?? null);
    return ids.length && ids.every((b) => b === ids[0]) ? ids[0] : undefined;
  });
  const left = $derived(Math.max(8, Math.min(x, window.innerWidth - 300)));
  const top = $derived(Math.max(8, Math.min(y, window.innerHeight - 280)));

  function close() {
    onClose();
    restore();
  }
  onMount(() => {
    const onDoc = (e: PointerEvent) => {
      if (el && !el.contains(e.target as Node)) onClose();
    };
    const t = setTimeout(() => document.addEventListener("pointerdown", onDoc, true), 0);
    return () => {
      clearTimeout(t);
      document.removeEventListener("pointerdown", onDoc, true);
    };
  });

  async function pick(id: string | null) {
    close();
    const list = itemIds.map((i) => items.get(i)).filter((i): i is Item => !!i && i.bucket_id !== id);
    if (list.length) await moveMany(list, id);
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="popover floating"
  role="dialog"
  aria-label="Move to bucket"
  tabindex="-1"
  bind:this={el}
  style="left:{left}px; top:{top}px"
  onkeydown={(e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      close();
    }
  }}
>
  <div class="small muted">{itemIds.length > 1 ? `Move ${itemIds.length} to…${current === undefined ? " (from several buckets)" : ""}` : "Move to…"}</div>
  <BucketPicker value={current} onChange={pick} autofocus />
</div>

<style>
  .popover {
    position: fixed;
    z-index: 900;
    width: 280px;
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
</style>
