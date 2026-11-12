<script lang="ts">
  import * as api from "../api";
  import { open } from "@tauri-apps/plugin-dialog";
  import { attachments } from "../stores/attachments.svelte";
  import { toast } from "../stores/ui.svelte";
  import { formatAbsolute } from "../time";
  import { humanSize } from "../util";
  import { untrack } from "svelte";
  import type { ParentType } from "../types";
  import ConfirmInline from "./ConfirmInline.svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    parentType: ParentType;
    parentId: string;
  }
  let { parentType, parentId }: Props = $props();

  const list = $derived(attachments.for(parentType, parentId));
  let confirmId = $state<string | null>(null);
  let adding = $state(false);

  const parentKey = $derived(`${parentType}:${parentId}`);
  $effect(() => {
    void parentKey;
    untrack(() => void attachments.load(parentType, parentId));
  });

  async function addFiles() {
    try {
      const picked = await open({ multiple: true, directory: false });
      const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
      if (!paths.length) return;
      adding = true;
      await attachments.addPaths(parentType, parentId, paths);
    } catch (e) {
      toast.error(e);
    } finally {
      adding = false;
    }
  }

  function openFile(id: string) {
    api.openAttachment(id).catch((e) => toast.error(e));
  }
</script>

<div class="files">
  <div class="row">
    <button type="button" class="small" disabled={adding} onclick={addFiles}><Icon name="file-arrow-up" size={12} /> Add files…</button>
    <span class="muted small">or drop files onto this pane</span>
  </div>
  {#if list.length}
    <ul>
      {#each list as a (a.id)}
        <li>
          <div class="info">
            <span class="name truncate" title={a.path}>{a.filename}</span>
            <span class="meta muted small">{humanSize(a.size_bytes)} · {a.mime || "file"} · {formatAbsolute(a.created_at)}</span>
          </div>
          {#if confirmId === a.id}
            <ConfirmInline
              message={`Delete “${a.filename}”?`}
              onConfirm={() => {
                confirmId = null;
                void attachments.remove(a.id);
              }}
              onCancel={() => (confirmId = null)}
            />
          {:else}
            <div class="actions">
              <button type="button" class="small" aria-label="Open {a.filename}" onclick={() => openFile(a.id)}><Icon name="arrow-up-right-from-square" size={11} /> Open</button>
              <button type="button" class="small ghost danger" onclick={() => (confirmId = a.id)}><Icon name="trash" size={11} /> Delete</button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <div class="muted small">No files attached.</div>
  {/if}
</div>

<style>
  .files {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    overflow: hidden;
    background: var(--surface-card);
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--hairline-soft);
  }
  li:last-child {
    border-bottom: 0;
  }
  li:hover {
    background: var(--surface-doc);
  }
  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.35;
  }
  .name {
    color: var(--ink);
    font-weight: 500;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
</style>
