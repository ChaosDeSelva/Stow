<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { subscribe } from "../api";
  import { friendlyText } from "../errors";
  import { renderUserMarkdown } from "../markdown";
  import { recordings } from "../stores/recordings.svelte";
  import { system } from "../stores/system.svelte";
  import { fileSrc, lsGet, lsSet } from "../util";
  import { formatAbsolute, formatDuration } from "../time";
  import type { ModelDownloadProgress, ParentType } from "../types";
  import ConfirmInline from "./ConfirmInline.svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    parentType: ParentType;
    parentId: string;
    appendToNotes: (md: string) => void;
  }
  let { parentType, parentId, appendToNotes }: Props = $props();

  const list = $derived(recordings.for(parentType, parentId));
  let now = $state(Date.now());
  let expanded = $state<Record<string, boolean>>({});
  let confirmId = $state<string | null>(null);
  let progress = $state<ModelDownloadProgress | null>(null);
  let busy = $state(false);
  const EXPLAINED_KEY = "stow.recording.explained";
  let explained = $state(lsGet(EXPLAINED_KEY) === "1");

  const elapsed = $derived(
    recordings.isRecording && recordings.startedAt != null ? formatDuration(now - recordings.startedAt) : "00:00",
  );
  /** Recording in progress for a different parent than the one shown here. */
  const elsewhere = $derived(
    recordings.isRecording &&
      !!recordings.session &&
      (recordings.session.parentType !== parentType || recordings.session.parentId !== parentId),
  );

  const parentKey = $derived(`${parentType}:${parentId}`);
  $effect(() => {
    void parentKey;
    untrack(() => void recordings.load(parentType, parentId));
  });

  onMount(() => {
    void recordings.syncState();
    const tick = setInterval(() => (now = Date.now()), 500);
    const off = subscribe("model-download-progress", (p) => {
      progress = p.total != null && p.received >= p.total ? null : p;
    });
    return () => {
      clearInterval(tick);
      off();
    };
  });

  async function toggleRecord() {
    busy = true;
    try {
      if (recordings.isRecording) await recordings.stop(parentType, parentId);
      else {
        // Ask macOS for the microphone up front when it has never been asked (no-op on older backends).
        if (system.microphone === null || system.microphone === "not_determined") await system.askMicrophone();
        await recordings.start(parentType, parentId);
      }
    } finally {
      busy = false;
    }
  }

  function dismissExplainer() {
    explained = true;
    lsSet(EXPLAINED_KEY, "1");
  }

  function statusChip(s: string): string {
    if (s === "done") return "success";
    if (s === "failed") return "danger";
    if (s === "running" || s === "pending") return "accent";
    return "";
  }
  function statusLabel(s: string): string {
    switch (s) {
      case "pending":
        return "Waiting";
      case "running":
        return "Transcribing…";
      case "failed":
        return "Couldn't transcribe";
      case "done":
        return "Transcript ready";
      default:
        return "No transcript";
    }
  }
</script>

<div class="recordings">
  {#if !explained && !list.length}
    <div class="banner">
      <Icon name="circle-info" class="banner-icon" />
      <div class="explain">
        <span>
          Record a voice note with your Mac's microphone (macOS asks once). The transcript is made on this Mac; the audio
          never leaves it. The first transcription needs a one-time 142 MB download.
        </span>
        <div><button type="button" class="small" onclick={dismissExplainer}>Got it</button></div>
      </div>
    </div>
  {/if}

  <div class="row">
    <button type="button" class:danger={recordings.isRecording} disabled={busy} onclick={toggleRecord}>
      {#if recordings.isRecording}<Icon name="stop" size={12} /> Stop{:else}<Icon name="microphone" size={12} /> Record{/if}
    </button>
    <span class="timer mono" class:live={recordings.isRecording} aria-live="off">{elapsed}</span>
    {#if elsewhere}
      <span class="muted small" role="status">Recording for another item; Stop saves it there.</span>
    {:else if recordings.isRecording}
      <span class="muted small" role="status">Recording from the default microphone…</span>
    {/if}
  </div>

  {#if progress}
    <div class="progress" role="status">
      <div class="small">Downloading the transcription model… {Math.round(progress.received / 1048576)} MB{progress.total ? ` of ${Math.round(progress.total / 1048576)} MB` : ""}</div>
      <div class="bar" role="progressbar" aria-label="Model download" aria-valuemin="0" aria-valuemax="100" aria-valuenow={progress.total ? Math.round((100 * progress.received) / progress.total) : undefined}>
        <div class="fill" style="width:{progress.total ? Math.min(100, (100 * progress.received) / progress.total) : 100}%"></div>
      </div>
    </div>
  {/if}

  {#if list.length}
    <ul>
      {#each list as r (r.id)}
        {@const src = fileSrc(r.audio_path)}
        <li>
          <div class="head">
            <span class="created">{formatAbsolute(r.created_at)}</span>
            <span class="muted mono">{formatDuration(r.duration_ms)}</span>
            <span class="chip {statusChip(r.transcript_status)}">{statusLabel(r.transcript_status)}</span>
            <span class="spacer"></span>
            {#if r.transcript_md}
              <button type="button" class="small ghost" aria-expanded={!!expanded[r.id]} onclick={() => (expanded = { ...expanded, [r.id]: !expanded[r.id] })}>
                {expanded[r.id] ? "Hide transcript" : "Show transcript"}
              </button>
              <button type="button" class="small ghost" onclick={() => appendToNotes(r.transcript_md ?? "")}><Icon name="plus" size={11} /> Insert into notes</button>
            {/if}
            <button
              type="button"
              class="small ghost"
              disabled={r.transcript_status === "pending" || r.transcript_status === "running"}
              onclick={() => recordings.retranscribe(r.id)}><Icon name="rotate" size={11} /> Transcribe again</button
            >
            <button type="button" class="small ghost danger" onclick={() => (confirmId = r.id)}><Icon name="trash" size={11} /> Delete</button>
          </div>
          {#if src}
            <audio controls preload="none" {src} aria-label="Recording from {formatAbsolute(r.created_at)}"></audio>
          {/if}
          {#if r.transcript_status === "failed" && r.transcript_error}
            <div class="error small">{friendlyText(r.transcript_error)}</div>
            <details class="err-details"><summary>Details</summary><pre>{r.transcript_error}</pre></details>
          {/if}
          {#if confirmId === r.id}
            <ConfirmInline
              message="Delete this recording and its transcript?"
              onConfirm={() => {
                confirmId = null;
                void recordings.remove(r.id);
              }}
              onCancel={() => (confirmId = null)}
            />
          {/if}
          {#if expanded[r.id] && r.transcript_md}
            <div class="md transcript">{@html renderUserMarkdown(r.transcript_md)}</div>
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <div class="muted small">No recordings yet.</div>
  {/if}
</div>

<style>
  .recordings {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .timer {
    font-size: 14px;
    min-width: 46px;
    font-weight: 600;
    color: var(--ink);
  }
  .timer.live {
    color: var(--danger-text);
  }
  .progress {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .bar {
    height: 8px;
    border-radius: 9999px;
    background: var(--surface-soft);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent-blue);
    transition: width 0.2s;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  li {
    padding: 10px 12px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: var(--surface-card);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .created {
    color: var(--ink);
    font-weight: 600;
  }
  .error {
    color: var(--danger-text);
  }
  .explain {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  audio {
    width: 100%;
    max-width: 420px;
    height: 32px;
  }
  .transcript {
    padding: 14px 16px;
    border-radius: var(--radius);
    background: var(--surface-doc);
    border: 1px solid var(--hairline-soft);
  }
</style>
