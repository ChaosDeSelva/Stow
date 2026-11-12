<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { deletions } from "../stores/deletions.svelte";
  import { effectiveTime, reminderState, reminders } from "../stores/reminders.svelte";
  import { ui } from "../stores/ui.svelte";
  import { resolveTarget, type ResolvedTarget } from "../targets";
  import { formatAbsolute, formatRelative, presets } from "../time";
  import type { Reminder } from "../types";
  import Icon from "./Icon.svelte";

  let titles = $state<Record<string, ResolvedTarget | null>>({});
  const agenda = $derived(reminders.agenda);

  onMount(() => {
    void reminders.loadAgenda();
    // Background refresh: quiet, so an outage toasts once rather than every minute.
    const t = setInterval(() => reminders.loadAgenda(new Date(), true), 60_000);
    return () => clearInterval(t);
  });

  $effect(() => {
    const a = agenda;
    if (!a) return;
    for (const r of [...a.overdue, ...a.today, ...a.upcoming]) {
      const k = `${r.target_type}:${r.target_id}`;
      if (untrack(() => k in titles)) continue;
      titles = { ...untrack(() => titles), [k]: null };
      void resolveTarget(r.target_type, r.target_id)
        .then((res) => {
          titles = { ...titles, [k]: res };
        })
        .catch(() => {
          titles = { ...titles, [k]: null };
        });
    }
  });

  function titleFor(r: Reminder): string {
    const t = titles[`${r.target_type}:${r.target_id}`];
    return t?.title ?? (t === null ? "…" : "(missing)");
  }

  async function go(r: Reminder) {
    const t = (await resolveTarget(r.target_type, r.target_id, { fresh: true })) ?? null;
    if (!t) return;
    ui.selectBucketAndItem(t.bucketId, t.itemId);
    if (t.itemId) ui.detailTab = r.target_type === "task" ? "tasks" : "reminders";
  }

  const visible = (l: Reminder[]) => l.filter((r) => !deletions.isHidden(r.id));
  const sections = $derived(
    agenda
      ? [
          { key: "overdue", title: "Overdue", list: visible(agenda.overdue) },
          { key: "today", title: "Today", list: visible(agenda.today) },
          { key: "upcoming", title: "Upcoming", list: visible(agenda.upcoming) },
        ]
      : [],
  );
  const snoozes = $derived(presets(new Date()));

  function label(r: Reminder) {
    return r.message || titleFor(r);
  }
  function done(r: Reminder) {
    deletions.schedule(r.id, label(r), () => reminders.remove(r.id), { message: `Done: “${label(r)}”.` });
  }
  const total = $derived(sections.reduce((n, s) => n + s.list.length, 0));
</script>

<div class="today">
  <header>
    <h1 class="h">Today</h1>
    <span class="muted small">{new Date().toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" })}</span>
    <span class="spacer"></span>
    <button type="button" class="small ghost" onclick={() => reminders.loadAgenda()}><Icon name="rotate" size={11} /> Refresh</button>
  </header>

  {#if !agenda && reminders.agendaLoading}
    <div class="muted small">Loading…</div>
  {:else if total === 0}
    <div class="empty">Nothing scheduled. Add a reminder from an item, a bucket or a task (<Icon name="bell" size={11} label="Remind" />).</div>
  {/if}

  {#each sections as s (s.key)}
    {#if s.list.length}
      <section class={s.key}>
        <h2 class="section-title">{s.title} <span class="count">{s.list.length}</span></h2>
        <ul>
          {#each s.list as r (r.id)}
            <li class:fired={!!r.fired_at}>
              <div class="when">
                <span class="rel">{formatRelative(effectiveTime(r))}</span>
                <span class="abs muted small">{formatAbsolute(effectiveTime(r))}</span>
              </div>
              <div
                class="body"
                role="button"
                tabindex="0"
                onclick={() => go(r)}
                onkeydown={(e) => e.key === "Enter" && go(r)}
              >
                <span class="target truncate">
                  <span class="chip">{r.target_type}</span>
                  {titleFor(r)}
                </span>
                {#if r.message}<span class="msg muted truncate">{r.message}</span>{/if}
                {#if reminderState(r)}{@const st = reminderState(r)}<span class="chip {st?.cls}">{st?.label}</span>{/if}
              </div>
              <div class="actions">
                <button type="button" class="small ghost" onclick={() => done(r)}><Icon name="check" size={11} /> Done</button>
                {#each snoozes as p}
                  <button type="button" class="small ghost" aria-label="Snooze: {p.label}" onclick={() => reminders.snooze(r.id, p.fireAt)}>{p.label}</button>
                {/each}
                <button type="button" class="small ghost icon" aria-label="Delete reminder" onclick={() => deletions.schedule(r.id, label(r), () => reminders.remove(r.id))}><Icon name="trash" size={12} /></button>
              </div>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  {/each}
</div>

<style>
  .today {
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 20px 24px;
    overflow: auto;
    height: 100%;
    background: var(--surface-card);
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .h {
    margin: 0;
    font-size: 21px;
    letter-spacing: -0.5px;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .count {
    margin-left: 4px;
    font-weight: 600;
    color: var(--mute);
  }
  section.overdue .section-title,
  section.overdue .rel {
    color: var(--danger-text);
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
    gap: 10px;
    padding: 8px 8px 8px 14px;
    border-bottom: 1px solid var(--hairline-soft);
  }
  li:last-child {
    border-bottom: 0;
  }
  li:hover {
    background: var(--surface-doc);
  }
  li.fired {
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
    gap: 8px;
    min-width: 0;
    cursor: default;
  }
  .target {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    color: var(--ink);
    font-weight: 500;
  }
  .msg {
    min-width: 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 2px;
    opacity: 0;
  }
  li:hover .actions,
  .actions:focus-within {
    opacity: 1;
  }
</style>
