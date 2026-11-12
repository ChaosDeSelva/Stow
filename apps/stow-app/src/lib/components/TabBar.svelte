<script lang="ts" module>
  export interface TabDef<T extends string = string> {
    id: T;
    label: string;
    icon?: string;
  }
</script>

<script lang="ts" generics="T extends string">
  // Pill tablist with roving tabindex: ←/→/Home/End move focus AND selection.
  // Panels use id `${idPrefix}-panel-${id}` and aria-labelledby `${idPrefix}-tab-${id}`.
  import { rovingIndex } from "../focus";
  import Icon from "./Icon.svelte";
  interface Props {
    tabs: TabDef<T>[];
    active: T;
    onChange: (id: T) => void;
    idPrefix: string;
    label: string;
  }
  let { tabs, active, onChange, idPrefix, label }: Props = $props();
  let el = $state<HTMLDivElement | null>(null);

  function onKey(e: KeyboardEvent) {
    const i = tabs.findIndex((t) => t.id === active);
    const n = rovingIndex(e.key, i, tabs.length, "horizontal");
    if (n == null) return;
    e.preventDefault();
    onChange(tabs[n].id);
    queueMicrotask(() => el?.querySelector<HTMLElement>(`#${CSS.escape(`${idPrefix}-tab-${tabs[n].id}`)}`)?.focus());
  }
</script>

<div class="pill-tabs" role="tablist" aria-label={label} bind:this={el} onkeydown={onKey} tabindex="-1">
  {#each tabs as t (t.id)}
    <button
      type="button"
      role="tab"
      id="{idPrefix}-tab-{t.id}"
      aria-controls="{idPrefix}-panel-{t.id}"
      aria-selected={active === t.id}
      tabindex={active === t.id ? 0 : -1}
      class="tab"
      class:active={active === t.id}
      onclick={() => onChange(t.id)}>{#if t.icon}<Icon name={t.icon} size={12} />{/if} {t.label}</button
    >
  {/each}
</div>

<style>
  .pill-tabs {
    flex-wrap: wrap;
  }
  .pill-tabs:focus {
    outline: none;
  }
</style>
