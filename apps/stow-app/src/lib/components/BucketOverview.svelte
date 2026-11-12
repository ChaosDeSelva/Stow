<script lang="ts">
  // Right pane when a bucket is selected and no item is: the bucket's own notes,
  // tasks, reminders, files, recordings and AI, with the same tabs as an item.
  import { appendNotes, saveNotesByKey } from "../notesSave";
  import { ui, type BucketTab } from "../stores/ui.svelte";
  import type { Bucket } from "../types";
  import AiPanel from "./AiPanel.svelte";
  import AttachmentList from "./AttachmentList.svelte";
  import NotesEditor from "./NotesEditor.svelte";
  import RecordingList from "./RecordingList.svelte";
  import ReminderList from "./ReminderList.svelte";
  import TabBar, { type TabDef } from "./TabBar.svelte";
  import TaskList from "./TaskList.svelte";

  interface Props {
    bucket: Bucket;
  }
  let { bucket }: Props = $props();

  const TABS: TabDef<BucketTab>[] = [
    { id: "notes", label: "Notes", icon: "note-sticky" },
    { id: "tasks", label: "Tasks", icon: "list-check" },
    { id: "reminders", label: "Reminders", icon: "bell" },
    { id: "files", label: "Files", icon: "paperclip" },
    { id: "recordings", label: "Recordings", icon: "microphone" },
    { id: "ai", label: "AI", icon: "wand-magic-sparkles" },
  ];

  function appendToNotes(md: string) {
    appendNotes("bucket", bucket.id, md);
    ui.bucketTab = "notes";
  }
</script>

<div class="overview">
  <header>
    <h2><span class="emoji-box" aria-hidden="true">{bucket.emoji ?? "•"}</span> {bucket.name}</h2>
    <p class="muted small">Bucket notes, tasks and reminders. Select an item on the left to open it.</p>
  </header>
  <div class="tabs">
    <TabBar tabs={TABS} active={ui.bucketTab} onChange={(t) => (ui.bucketTab = t)} idPrefix="bucket" label="Bucket sections" />
  </div>
  <div class="body" role="tabpanel" id="bucket-panel-{ui.bucketTab}" aria-labelledby="bucket-tab-{ui.bucketTab}" tabindex="-1">
    {#if ui.bucketTab === "notes"}
      <NotesEditor recordKey={`bucket:${bucket.id}`} value={bucket.notes_md} onSave={saveNotesByKey} minRows={14} placeholder="Bucket notes (Markdown)…" label="Bucket notes" />
    {:else if ui.bucketTab === "tasks"}
      <TaskList parentType="bucket" parentId={bucket.id} />
    {:else if ui.bucketTab === "reminders"}
      <ReminderList targetType="bucket" targetId={bucket.id} defaultMessage={bucket.name} />
    {:else if ui.bucketTab === "files"}
      <AttachmentList parentType="bucket" parentId={bucket.id} />
    {:else if ui.bucketTab === "recordings"}
      <RecordingList parentType="bucket" parentId={bucket.id} {appendToNotes} />
    {:else}
      <AiPanel parentType="bucket" parentId={bucket.id} {appendToNotes} />
    {/if}
  </div>
</div>

<style>
  .overview {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--surface-card);
  }
  header {
    padding: 18px 24px 6px;
  }
  h2 {
    margin: 0;
    font-size: 18px;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  p {
    margin: 4px 0 0;
  }
  .tabs {
    padding: 6px 24px 10px;
    border-bottom: 1px solid var(--hairline);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 16px 24px 24px;
  }
</style>
