<script lang="ts">
  import { untrack } from "svelte";
  import { deletions } from "../stores/deletions.svelte";
  import { reminders } from "../stores/reminders.svelte";
  import { tasks } from "../stores/tasks.svelte";
  import { toast } from "../stores/ui.svelte";
  import { formatAbsolute } from "../time";
  import type { ParentType } from "../types";
  import Icon from "./Icon.svelte";
  import ReminderForm from "./ReminderForm.svelte";

  interface Props {
    parentType: ParentType;
    parentId: string;
    compact?: boolean;
  }
  let { parentType, parentId, compact = false }: Props = $props();

  const list = $derived(tasks.for(parentType, parentId).filter((t) => !deletions.isHidden(t.id)));
  let draft = $state("");
  let editingId = $state<string | null>(null);
  let editText = $state("");
  let remindId = $state<string | null>(null);

  // Read the parent, then load untracked: the store's writes can't re-run this effect.
  const parentKey = $derived(`${parentType}:${parentId}`);
  $effect(() => {
    void parentKey;
    untrack(() => void tasks.load(parentType, parentId));
  });

  async function add() {
    const t = draft.trim();
    if (!t) return;
    draft = "";
    await tasks.add(parentType, parentId, t);
  }

  function startEdit(id: string, title: string) {
    editingId = id;
    editText = title;
  }
  async function commitEdit() {
    if (!editingId) return;
    const id = editingId;
    const title = editText.trim();
    editingId = null;
    const cur = list.find((t) => t.id === id);
    if (title && cur && title !== cur.title) await tasks.rename(id, title);
  }
  function editKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      void commitEdit();
    } else if (e.key === "Escape") {
      editingId = null;
    }
  }
  function remove(id: string, title: string) {
    deletions.schedule(id, title, () => tasks.remove(id));
  }
  async function addReminder(taskId: string, fireAt: string, message: string | null) {
    const r = await reminders.add("task", taskId, fireAt, message);
    if (r) {
      remindId = null;
      toast.success(`Reminder set for ${formatAbsolute(fireAt)}`);
    }
  }

  function focusInput(el: HTMLInputElement) {
    el.focus();
    el.select();
  }
</script>

<div class="tasks" class:compact>
  <input
    type="text"
    class="add"
    aria-label="New task"
    placeholder="Add a task and press Enter"
    bind:value={draft}
    onkeydown={(e) => e.key === "Enter" && add()}
  />
  {#if list.length}
    <ul>
      {#each list as t (t.id)}
        <li class:done={!!t.done_at}>
          <input type="checkbox" aria-label="Done: {t.title}" checked={!!t.done_at} onchange={() => tasks.toggle(t.id)} />
          {#if editingId === t.id}
            <input
              type="text"
              class="edit"
              aria-label="Task title"
              bind:value={editText}
              onkeydown={editKey}
              onblur={commitEdit}
              use:focusInput
            />
          {:else}
            <span
              class="title"
              role="button"
              tabindex="0"
              title="Double-click to rename"
              ondblclick={() => startEdit(t.id, t.title)}
              onkeydown={(e) => e.key === "Enter" && startEdit(t.id, t.title)}>{t.title}</span
            >
          {/if}
          <button type="button" class="ghost icon small reveal" aria-label="Remind me about {t.title}" title="Add a reminder" onclick={() => (remindId = remindId === t.id ? null : t.id)}
            ><Icon name="bell" size={12} /></button
          >
          <button type="button" class="ghost icon small reveal" aria-label="Delete task {t.title}" title="Delete task" onclick={() => remove(t.id, t.title)}
            ><Icon name="xmark" size={12} /></button
          >
        </li>
        {#if remindId === t.id}
          <li class="remind">
            <ReminderForm defaultMessage={t.title} onCancel={() => (remindId = null)} onSubmit={(at, msg) => addReminder(t.id, at, msg)} />
          </li>
        {/if}
      {/each}
    </ul>
  {:else if !compact}
    <div class="muted small pad">No tasks yet.</div>
  {/if}
</div>

<style>
  .tasks {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .add {
    width: 100%;
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
    gap: 6px;
    min-height: 34px;
    padding: 2px 4px 2px 10px;
    border-bottom: 1px solid var(--hairline-soft);
  }
  li:last-child {
    border-bottom: 0;
  }
  li:hover {
    background: var(--surface-doc);
  }
  li.remind {
    display: block;
    padding: 8px;
    background: var(--surface-doc);
  }
  li .title {
    flex: 1;
    min-width: 0;
    color: var(--ink);
    overflow-wrap: anywhere;
  }
  li.done .title {
    text-decoration: line-through;
    color: var(--mute);
  }
  .edit {
    flex: 1;
    height: var(--control-h-sm);
  }
  .pad {
    padding: 2px 4px;
  }
</style>
