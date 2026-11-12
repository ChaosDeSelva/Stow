<script lang="ts">
  import { untrack } from "svelte";
  import { deletions } from "../stores/deletions.svelte";
  import { effectiveTime, reminderState, reminders } from "../stores/reminders.svelte";
  import { formatAbsolute, formatRelative, inOneHour, tomorrowNine } from "../time";
  import type { Reminder, TargetType } from "../types";
  import Icon from "./Icon.svelte";
  import ReminderForm from "./ReminderForm.svelte";

  interface Props {
    targetType: TargetType;
    targetId: string;
    compact?: boolean;
    /** Default reminder text: the target's title. */
    defaultMessage?: string;
  }
  let { targetType, targetId, compact = false, defaultMessage = "" }: Props = $props();

  const list = $derived(reminders.for(targetType, targetId).filter((r) => !deletions.isHidden(r.id)));
  let adding = $state(false);

  const targetKey = $derived(`${targetType}:${targetId}`);
  $effect(() => {
    void targetKey;
    untrack(() => void reminders.load(targetType, targetId));
  });

  function label(r: Reminder) {
    return r.message || defaultMessage || "Reminder";
  }
  function done(r: Reminder) {
    deletions.schedule(r.id, label(r), () => reminders.remove(r.id), { message: `Done: “${label(r)}”.` });
  }
  function remove(r: Reminder) {
    deletions.schedule(r.id, label(r), () => reminders.remove(r.id));
  }
</script>

<div class="reminders" class:compact>
  {#if list.length}
    <ul>
      {#each list as r (r.id)}
        {@const st = reminderState(r)}
        <li class:fired={!!r.fired_at}>
          <div class="when">
            <span class="rel">{formatRelative(effectiveTime(r))}</span>
            <span class="abs muted small">{formatAbsolute(effectiveTime(r))}</span>
          </div>
          <div class="body">
            <span class="msg">{label(r)}</span>
            {#if st}<span class="chip {st.cls}">{st.label}</span>{/if}
          </div>
          <div class="actions">
            <button type="button" class="small ghost" onclick={() => done(r)}><Icon name="check" size={11} /> Done</button>
            <button type="button" class="small ghost" aria-label="Snooze one hour" onclick={() => reminders.snooze(r.id, inOneHour())}
              ><Icon name="clock" size={11} /> +1h</button
            >
            <button type="button" class="small ghost" aria-label="Snooze until tomorrow 9:00" onclick={() => reminders.snooze(r.id, tomorrowNine())}>Tomorrow</button>
            <button type="button" class="small ghost icon" aria-label="Delete reminder" onclick={() => remove(r)}><Icon name="trash" size={12} /></button>
          </div>
        </li>
      {/each}
    </ul>
  {:else if !compact && !adding}
    <div class="muted small pad">No reminders.</div>
  {/if}

  {#if adding}
    <ReminderForm
      {defaultMessage}
      onCancel={() => (adding = false)}
      onSubmit={async (fireAt, message) => {
        const r = await reminders.add(targetType, targetId, fireAt, message);
        if (r) adding = false;
      }}
    />
  {:else}
    <div>
      <button type="button" class="small" onclick={() => (adding = true)}><Icon name="plus" size={11} /> Reminder</button>
    </div>
  {/if}
</div>

<style>
  .reminders {
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
    display: grid;
    grid-template-columns: 120px 1fr auto;
    align-items: center;
    gap: 8px;
    padding: 6px 6px 6px 12px;
    border-bottom: 1px solid var(--hairline-soft);
  }
  li:last-child {
    border-bottom: 0;
  }
  li:hover {
    background: var(--surface-doc);
  }
  li.fired,
  li.fired .rel {
    color: var(--mute);
  }
  .when {
    display: flex;
    flex-direction: column;
    line-height: 1.3;
  }
  .rel {
    color: var(--ink);
    font-weight: 600;
  }
  .body {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .msg {
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    gap: 2px;
    opacity: 0;
  }
  li:hover .actions,
  .actions:focus-within {
    opacity: 1;
  }
  .pad {
    padding: 2px 4px;
  }
</style>
