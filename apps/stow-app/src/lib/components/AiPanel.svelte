<script lang="ts">
  import { onMount, untrack } from "svelte";
  import * as api from "../api";
  import { copyText } from "../clipboard";
  import { friendlyText, PROVIDER_LABEL } from "../errors";
  import { parseTaskLines, renderUserMarkdown, wordCount } from "../markdown";
  import { aiRuns } from "../stores/aiRuns.svelte";
  import { buckets } from "../stores/buckets.svelte";
  import { recordings } from "../stores/recordings.svelte";
  import { DEFAULT_MODELS, settings } from "../stores/settings.svelte";
  import { system } from "../stores/system.svelte";
  import { tasks } from "../stores/tasks.svelte";
  import { toast, ui } from "../stores/ui.svelte";
  import { formatRelative } from "../time";
  import type { AiAction, Item, ParentType } from "../types";
  import { hostOf } from "../util";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";

  interface Props {
    parentType: ParentType;
    parentId: string;
    /** The item when the parent is an item (for snapshot status). */
    item?: Item | null;
    appendToNotes: (md: string) => void;
  }
  let { parentType, parentId, item = null, appendToNotes }: Props = $props();

  const runs = $derived(aiRuns.for(parentType, parentId));
  const recs = $derived(recordings.for(parentType, parentId));
  const hasTranscript = $derived(recs.some((r) => r.transcript_status === "done" && r.transcript_md));
  const provider = $derived(settings.value?.ai_provider ?? "anthropic");
  const providerLabel = $derived(PROVIDER_LABEL[provider]);
  const model = $derived(settings.value?.ai_model || DEFAULT_MODELS[provider]);
  const hasKey = $derived(system.hasKey[provider]);
  const notes = $derived(parentType === "item" ? (item?.notes_md ?? "") : (buckets.byId(parentId)?.notes_md ?? ""));
  let pageWords = $state<number | null>(null);
  let focusAction = $state<AiAction | null>(null);
  let consentFor = $state<AiAction | null>(null);
  /** Task preview per run: titles with a checked flag. */
  let preview = $state<Record<string, { title: string; on: boolean }[]>>({});
  let addedTasks = $state<Record<string, number>>({});

  const RUN_LABELS: Record<AiAction, string> = {
    summarize_page: "Page summary",
    summarize_notes: "Notes summary",
    summarize_recording: "Recording summary",
    suggest_tasks: "Suggested tasks",
    extract_metadata: "Fixed title & description",
  };

  // Load this parent's runs/recordings when the parent changes (store writes untracked).
  const parentKey = $derived(`${parentType}:${parentId}`);
  $effect(() => {
    void parentKey;
    untrack(() => {
      void aiRuns.load(parentType, parentId);
      void recordings.load(parentType, parentId);
    });
  });

  onMount(() => {
    if (!settings.value) void settings.load();
    if (system.hasKey.anthropic === null || system.hasKey.openai === null) void system.refreshKeys();
    if (item && item.snapshot_status === "done" && item.kind !== "video") {
      api
        .readSnapshot(item.id)
        .then((md) => (pageWords = wordCount(md)))
        .catch(() => (pageWords = null));
    }
  });

  const titleIsHost = $derived(
    !!item && (!item.title?.trim() || item.title.trim().replace(/^www\./, "") === hostOf(item.url)),
  );

  const actions = $derived.by(() => {
    const out: { action: AiAction; label: string; disabled: boolean; why?: string }[] = [];
    if (parentType === "item") {
      if (item?.kind === "video")
        out.push({ action: "summarize_page", label: "Summarize page", disabled: true, why: "Video pages have no readable text yet." });
      else {
        const ok = item?.snapshot_status === "done";
        out.push({ action: "summarize_page", label: "Summarize page", disabled: !ok, why: ok ? undefined : "Needs a readable copy of the page first." });
      }
    }
    const hasNotes = !!notes.trim();
    out.push({ action: "summarize_notes", label: "Summarize notes", disabled: !hasNotes, why: hasNotes ? undefined : "Write some notes first." });
    out.push({
      action: "summarize_recording",
      label: "Summarize recording",
      disabled: !hasTranscript,
      why: hasTranscript ? undefined : "Needs a recording with a transcript.",
    });
    out.push({ action: "suggest_tasks", label: "Suggest tasks", disabled: false });
    if (parentType === "item" && titleIsHost) out.push({ action: "extract_metadata", label: "Fix title & description", disabled: false });
    return out;
  });

  const anyRunning = $derived(runs.some((r) => r.status === "pending" || r.status === "running"));

  /** "Sends the page text (~N words) to Anthropic · model" for the focused (or first enabled) action. */
  const privacyLine = $derived.by(() => {
    const a = focusAction ?? actions.find((x) => !x.disabled)?.action ?? null;
    if (!a) return "";
    const to = `to ${providerLabel} · ${model}`;
    switch (a) {
      case "summarize_page":
        return `Sends the page text${pageWords ? ` (~${pageWords} words)` : ""} ${to}`;
      case "summarize_notes":
        return `Sends your notes (~${wordCount(notes)} words) ${to}`;
      case "summarize_recording":
        return `Sends the transcript ${to}`;
      case "suggest_tasks":
        return `Sends the notes${parentType === "item" ? " and page text" : ""} ${to}`;
      case "extract_metadata":
        return `Sends the page address and text ${to}`;
    }
  });

  function statusChip(s: string) {
    if (s === "done") return "success";
    if (s === "failed") return "danger";
    return "accent";
  }
  function statusText(s: string) {
    if (s === "done") return "Done";
    if (s === "failed") return "Failed";
    return "Working…";
  }

  function start(a: AiAction) {
    if (!settings.value?.ai_consent_given) {
      consentFor = a;
      return;
    }
    void aiRuns.run(parentType, parentId, a);
  }
  async function consent() {
    const a = consentFor;
    consentFor = null;
    if (!a) return;
    await settings.update({ ai_consent_given: true }, { quiet: true });
    void aiRuns.run(parentType, parentId, a);
  }

  function copy(md: string) {
    copyText(md)
      .then(() => toast.success("Copied"))
      .catch((e) => toast.error(e));
  }
  function showPreview(runId: string, md: string) {
    const titles = parseTaskLines(md);
    if (!titles.length) {
      toast.info("No task lines found in this output.");
      return;
    }
    preview = { ...preview, [runId]: titles.map((title) => ({ title, on: true })) };
  }
  async function addTasks(runId: string) {
    const picked = (preview[runId] ?? []).filter((t) => t.on);
    let n = 0;
    for (const t of picked) if (await tasks.add(parentType, parentId, t.title)) n++;
    const { [runId]: _done, ...rest } = preview;
    preview = rest;
    addedTasks = { ...addedTasks, [runId]: n };
    toast.success(`Added ${n} suggested task${n === 1 ? "" : "s"}`);
  }
</script>

<div class="ai">
  <div class="row header">
    <span class="chip accent">{providerLabel}</span>
    <span class="mono">{model}</span>
    <span class="spacer"></span>
    {#if hasKey === true}
      <span class="muted small"><Icon name="circle-check" size={11} /> Key saved</span>
    {:else if hasKey === false}
      <span class="small warn">No API key</span>
    {/if}
  </div>

  {#if hasKey === false}
    <div class="banner purple">
      <Icon name="key" class="banner-icon" />
      <span
        >Add your {providerLabel} API key in
        <a href="#settings" onclick={(e) => { e.preventDefault(); ui.select({ kind: "settings" }); }}>Settings → AI</a>
        to run AI actions. Keys stay in the macOS Keychain.</span
      >
    </div>
  {/if}

  <div class="actions" role="group" aria-label="AI actions">
    {#each actions as a (a.action)}
      <span class="act" title={a.why ?? ""}>
        <button
          type="button"
          class="small"
          disabled={a.disabled || hasKey !== true || anyRunning}
          aria-describedby={a.why ? `why-${a.action}` : "ai-privacy"}
          onmouseenter={() => (focusAction = a.action)}
          onfocus={() => (focusAction = a.action)}
          onclick={() => start(a.action)}>{a.label}</button
        >
        {#if a.why}<span class="sr-only" id="why-{a.action}">{a.why}</span>{/if}
      </span>
    {/each}
  </div>
  {#if actions.some((a) => a.disabled && a.why)}
    <ul class="whys small muted">
      {#each actions.filter((a) => a.disabled && a.why) as a (a.action)}
        <li>{a.label}: {a.why}</li>
      {/each}
    </ul>
  {/if}
  {#if privacyLine && hasKey}
    <div class="muted small" id="ai-privacy"><Icon name="shield-halved" size={11} /> {privacyLine}. Nothing is sent until you click.</div>
  {/if}

  {#if runs.length}
    <ul class="runs">
      {#each runs as r (r.id)}
        <li>
          <div class="row head">
            <span class="name">{RUN_LABELS[r.action] ?? r.action}</span>
            <span class="chip {statusChip(r.status)}">{statusText(r.status)}</span>
            <span class="muted small">{PROVIDER_LABEL[r.provider as keyof typeof PROVIDER_LABEL] ?? r.provider} · {r.model} · {formatRelative(r.created_at)}</span>
            <span class="spacer"></span>
            {#if r.output_md}
              <button type="button" class="small ghost" onclick={() => copy(r.output_md ?? "")}><Icon name="copy" style="regular" size={11} /> Copy</button>
              <button type="button" class="small ghost" onclick={() => appendToNotes(r.output_md ?? "")}><Icon name="plus" size={11} /> Append to notes</button>
              {#if r.action === "suggest_tasks" && !preview[r.id]}
                <button type="button" class="small ghost" onclick={() => showPreview(r.id, r.output_md ?? "")}>
                  {#if addedTasks[r.id] != null}<Icon name="circle-check" size={11} /> Added {addedTasks[r.id]}{:else}<Icon name="list-check" size={11} /> Add as tasks{/if}
                </button>
              {/if}
            {/if}
          </div>
          {#if r.status === "failed"}
            <div class="error small">{friendlyText(r.error ?? "Failed", { provider })}</div>
            {#if r.error}<details class="err-details"><summary>Details</summary><pre>{r.error}</pre></details>{/if}
          {:else if r.status === "pending" || r.status === "running"}
            <div class="muted small" role="status">Working…</div>
          {/if}
          {#if preview[r.id]}
            {@const rows = preview[r.id]}
            {@const n = rows.filter((t) => t.on).length}
            <fieldset class="preview">
              <legend class="small muted">Pick the tasks to add</legend>
              {#each rows as t, i}
                <label class="check">
                  <input type="checkbox" checked={t.on} onchange={(e) => (preview[r.id][i].on = (e.target as HTMLInputElement).checked)} />
                  {t.title}
                </label>
              {/each}
              <div class="row">
                <button type="button" class="small" disabled={!n} onclick={() => addTasks(r.id)}>Add {n} task{n === 1 ? "" : "s"}</button>
                <button type="button" class="small ghost" onclick={() => { const { [r.id]: _x, ...rest } = preview; preview = rest; }}>Cancel</button>
              </div>
            </fieldset>
          {/if}
          {#if r.output_md}
            <div class="md out">{@html renderUserMarkdown(r.output_md)}</div>
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <div class="muted small">Nothing generated yet.</div>
  {/if}
</div>

{#if consentFor}
  <Modal title="Send to {providerLabel}?" onClose={() => (consentFor = null)}>
    <p>
      AI actions send the text they need (page text, notes or a transcript) to {providerLabel} using your own API key.
      Stow sends nothing unless you click an AI action. {providerLabel}'s data policy applies to what you send.
    </p>
    <p class="muted small">You'll only be asked once.</p>
    {#snippet actions()}
      <button type="button" onclick={() => (consentFor = null)}>Cancel</button>
      <button type="button" class="primary" onclick={consent}>Send to {providerLabel}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .ai {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .header {
    font-size: 13px;
  }
  .warn {
    color: var(--warning-text);
    font-weight: 600;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .whys {
    margin: -4px 0 0;
    padding-left: 18px;
  }
  .runs {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .runs > li {
    padding: 10px 12px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: var(--surface-card);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .head {
    flex-wrap: wrap;
    gap: 6px;
  }
  .name {
    color: var(--ink);
    font-weight: 700;
  }
  .error {
    color: var(--danger-text);
  }
  .preview {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
    padding: 10px 12px;
    border: 1px solid var(--hairline-soft);
    border-radius: var(--radius);
  }
  .out {
    padding: 14px 16px;
    border-radius: var(--radius);
    background: var(--surface-doc);
    border: 1px solid var(--hairline-soft);
  }
  p {
    margin: 0;
  }
</style>
