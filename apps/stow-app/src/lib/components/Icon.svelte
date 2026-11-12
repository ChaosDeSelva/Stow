<script lang="ts">
  // Inline SVG glyph (tree-shaken Font Awesome). `name` is the FA name without `fa-`.
  import { iconData, type IconStyle } from "../icons";
  interface Props {
    name: string;
    /** solid (default), regular, or brands */
    style?: IconStyle;
    /** Accessible label; omit for purely decorative icons next to visible text. */
    label?: string;
    /** Fixed width for alignment in lists/menus. */
    fw?: boolean;
    size?: number;
    class?: string;
  }
  let { name, style = "solid", label, fw = false, size = 14, class: cls = "" }: Props = $props();
  const data = $derived(iconData(name, style));
</script>

<svg
  class="icon {cls}"
  class:fw
  viewBox="0 0 {data?.w ?? 512} {data?.h ?? 512}"
  width={fw ? size * 1.25 : ((data?.w ?? 512) / (data?.h ?? 512)) * size}
  height={size}
  aria-hidden={label ? undefined : "true"}
  aria-label={label}
  role={label ? "img" : undefined}
  focusable="false"
>
  {#if data}<path fill="currentColor" d={data.d} />{/if}
</svg>

<style>
  svg {
    display: inline-block;
    flex: none;
    vertical-align: -0.125em;
    overflow: visible;
  }
</style>
