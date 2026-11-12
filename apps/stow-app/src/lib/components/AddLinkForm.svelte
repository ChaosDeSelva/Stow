<script lang="ts">
  // Inline "Add link" row under the list header (⌘L). Takes one or many URLs
  // (commas / whitespace / newlines), a title when there is exactly one, and a
  // bucket for the whole batch. Pre-fills from the clipboard when it holds a link.
  // Focus goes to the field on open (and again on every ⌘L) and back where it
  // came from on Cancel / Esc / save.
  import { onMount, tick } from "svelte";
  import { readText } from "@tauri-apps/plugin-clipboard-manager";
  import { friendlyText } from "../errors";
  import { rememberFocus } from "../focus";
  import { markStep } from "../onboarding";
  import { buckets } from "../stores/buckets.svelte";
  import { items } from "../stores/items.svelte";
  import { settings } from "../stores/settings.svelte";
  import { ui } from "../stores/ui.svelte";
  import type { Item } from "../types";
  import { countIgnoredLines, extractUrls } from "../url";
  import { plural } from "../util";
  import BucketPicker from "./BucketPicker.svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    /** Bucket pre-selected in the picker (null = Inbox). */
    bucketId: string | null;
    onClose: () => void;
  }
  let { bucketId: initialBucket, onClose }: Props = $props();

  let text = $state("");
  let title = $state("");
  // svelte-ignore state_referenced_locally (the picker starts on the current bucket and is then user-owned)
  let bucketId = $state<string | null>(initialBucket);
  let saving = $state(false);
  let fromClipboard = $state(false);
  let field = $state<HTMLTextAreaElement | null>(null);
  let formEl = $state<HTMLFormElement | null>(null);
  const restoreFocus = rememberFocus();

  const urls = $derived(extractUrls(text));
  const rows = $derived(Math.min(6, Math.max(1, text.split("\n").length)));
  const countLabel = $derived(urls.length ? plural(urls.length, "link") : "No links found");
  const ignored = $derived(countIgnoredLines(text));
  const ignoredNote = $derived(
    ignored === 1 ? "Ignored 1 line that wasn't a web address" : `Ignored ${ignored} lines that weren't web addresses`,
  );

  onMount(() => {
    void prefill();
  });

  // Open, and every later ⌘L / "Add link" while already open: put the caret in the field.
  $effect(() => {
    void ui.addLinkFocusTick;
    field?.focus();
  });

  /** Close and hand focus back (opener, else the header's Add link button) when it was in the form. */
  function close() {
    const active = document.activeElement;
    const ours = !active || active === document.body || !!formEl?.contains(active);
    onClose();
    if (!ours) return;
    restoreFocus();
    if (!document.activeElement || document.activeElement === document.body || formEl?.contains(document.activeElement)) {
      document.querySelector<HTMLElement>("[data-addlink-toggle]")?.focus();
    }
  }

  /** Select the duplicate in its own list (bucket, Inbox or Archived). */
  function show(it: Item) {
    ui.reveal(it);
  }

  /** Clipboard plugin first, web clipboard second; anything failing leaves the field empty. */
  async function prefill() {
    let clip = "";
    try {
      clip = await readText();
    } catch {
      try {
        clip = (await navigator.clipboard?.readText?.()) ?? "";
      } catch {
        return;
      }
    }
    const found = extractUrls(clip);
    if (found.length && !text.trim()) {
      text = found.join("\n");
      fromClipboard = true;
      await tick();
      field?.select();
    }
  }

  function mark() {
    const patch = markStep(settings.value?.onboarding_steps, "captured");
    if (patch && settings.value) void settings.update(patch, { quiet: true });
  }

  async function save() {
    if (saving || !urls.length) return;
    saving = true;
    const where = buckets.nameOf(bucketId);
    const single = urls.length === 1;
    let created = 0;
    const dupes: Item[] = [];
    const failures: { url: string; error: unknown }[] = [];
    try {
      for (const u of urls) {
        try {
          const r = await items.addItem(u, bucketId, single ? title.trim() || null : null, { quiet: true });
          if (r?.created) created++;
          else if (r) dupes.push(r.item);
        } catch (e) {
          failures.push({ url: u, error: e });
        }
      }
    } finally {
      saving = false;
    }
    if (created) mark();
    const showDupes = dupes.length
      ? [{ label: single ? "Show it" : "Show them", secondary: true, run: () => show(dupes[0]) }]
      : undefined;
    if (single) {
      if (created) ui.toast.success(`Saved to ${where}`);
      // The existing row may live elsewhere than the picked bucket: name where it really is.
      else if (dupes.length) ui.toast.info(dupes[0].done_at ? "Already in Stow (archived)" : `Already in ${buckets.nameOf(dupes[0].bucket_id)}`, { actions: showDupes, duration: 8000 });
      else if (failures.length) ui.toast.error(failures[0].error);
    } else {
      const parts: string[] = [];
      if (created) parts.push(`Saved ${created} to ${where}`);
      if (dupes.length) parts.push(`${dupes.length} already in Stow`);
      if (failures.length) parts.push(`Couldn't save ${failures.length} (see Details)`);
      const msg = parts.join(" · ");
      const detail = failures.map((f) => `${f.url}: ${friendlyText(f.error)}`).join("\n");
      if (failures.length) ui.toast.error(new Error(msg), {}, { detail, actions: showDupes });
      else if (created) ui.toast.success(msg, { actions: showDupes, duration: showDupes ? 8000 : undefined });
      else ui.toast.info(msg, { actions: showDupes, duration: 8000 });
    }
    if (!failures.length) close();
    else text = failures.map((f) => f.url).join("\n");
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      if ((e.target as HTMLElement).tagName === "BUTTON") return;
      e.preventDefault();
      void save();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<form class="addlink" aria-label="Add link" bind:this={formEl} onsubmit={(e) => { e.preventDefault(); void save(); }} onkeydown={onKey}>
  <label class="sr-only" for="addlink-url">Web addresses</label>
  <textarea
    id="addlink-url"
    bind:this={field}
    bind:value={text}
    {rows}
    placeholder="Paste one or more links — commas, spaces or new lines are fine"
    spellcheck="false"
    autocomplete="off"
    oninput={() => (fromClipboard = false)}
  ></textarea>
  <div class="meta small">
    <span class="count muted" role="status">{countLabel}</span>
    {#if fromClipboard}<span class="chip accent clip">From clipboard</span>{/if}
    <span class="hint muted" aria-hidden="true">⏎ save · ⇧⏎ new line · esc cancel</span>
  </div>
  {#if ignored}
    <p class="ignored small muted" role="status">{ignoredNote}</p>
  {/if}
  {#if urls.length <= 1}
    <label class="sr-only" for="addlink-title">Title</label>
    <input id="addlink-title" type="text" placeholder="Title (optional)" bind:value={title} />
  {/if}
  <div class="picker"><BucketPicker value={bucketId} onChange={(id) => (bucketId = id)} onSubmit={save} label="Save to bucket" placeholder="Type to pick a bucket…" /></div>
  <div class="buttons">
    <button type="button" class="small" onclick={close}>Cancel</button>
    <button type="submit" class="small primary" disabled={saving || !urls.length}>
      <Icon name="plus" size={11} /> {saving ? "Saving…" : urls.length > 1 ? `Save ${urls.length}` : "Save"}
    </button>
  </div>
</form>

<style>
  /* Rows: URLs · count + clipboard chip | hint · title (≤ 1 URL) · bucket · Cancel / Save.
     The form is a size container so the hint can drop out on narrow list panes. */
  .addlink {
    container-type: inline-size;
    /* Containment makes this a stacking context: keep it above the rows so the picker's dropdown is too. */
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: var(--surface-card);
    min-width: 0;
  }
  textarea {
    width: 100%;
    resize: none;
    font-family: var(--font-sans);
    font-size: 13px;
    line-height: 1.4;
    padding: 6px 8px;
    overflow-wrap: anywhere;
  }
  input {
    width: 100%;
    height: var(--control-h-sm);
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    min-height: 18px;
  }
  .count,
  .hint,
  .clip {
    white-space: nowrap;
  }
  .clip {
    text-transform: none;
    letter-spacing: 0;
    font-weight: 500;
    line-height: 1.4;
    padding: 0 7px;
  }
  .hint {
    margin-left: auto;
    flex: none;
  }
  .ignored {
    margin: 0;
  }
  @container (max-width: 459px) {
    .hint {
      display: none;
    }
  }
  .picker {
    width: 100%;
    min-width: 0;
  }
  .picker :global(input) {
    height: var(--control-h-sm);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
</style>
