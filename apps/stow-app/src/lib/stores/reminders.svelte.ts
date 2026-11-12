import * as api from "../api";
import { dayBounds } from "../time";
import type { Reminder, ReminderAgenda, TargetType } from "../types";
import { parentKeyed } from "./collection.svelte";
import { toast } from "./ui.svelte";

const col = parentKeyed<Reminder>((tt, tid) => api.listReminders(tt as TargetType, tid));

let agenda = $state<ReminderAgenda | null>(null);
let agendaLoading = $state(false);
let agendaQuietFailed = false;

/** The moment a reminder will (or did) fire: snooze wins over the original time. */
export function effectiveTime(r: Reminder): string {
  return r.snoozed_until ?? r.fire_at;
}

/** Chip for a reminder row: Sent / Snoozed / Due, or null when it is simply upcoming. */
export function reminderState(r: Reminder, now: number = Date.now()): { label: string; cls: string } | null {
  if (r.fired_at) return { label: "Sent", cls: "" };
  if (r.snoozed_until) return { label: "Snoozed", cls: "warning" };
  if (new Date(effectiveTime(r)).getTime() < now) return { label: "Due", cls: "danger" };
  return null;
}

function sortByTime(list: Reminder[]): Reminder[] {
  return [...list].sort((a, b) => {
    if (!!a.fired_at !== !!b.fired_at) return a.fired_at ? 1 : -1;
    return effectiveTime(a).localeCompare(effectiveTime(b));
  });
}

function patchAgenda(fn: (list: Reminder[]) => Reminder[]) {
  if (!agenda) return;
  agenda = { overdue: fn(agenda.overdue), today: fn(agenda.today), upcoming: fn(agenda.upcoming) };
}

export const reminders = {
  for(targetType: TargetType, targetId: string): Reminder[] {
    return sortByTime(col.get(targetType, targetId));
  },
  load: (targetType: TargetType, targetId: string) => col.load(targetType, targetId),

  get agenda() {
    return agenda;
  },
  get agendaLoading() {
    return agendaLoading;
  },
  /** Unfired reminders whose time has passed (Today nav badge). */
  get overdueCount(): number {
    return agenda?.overdue.length ?? 0;
  },
  /** `quiet` (polls, background reloads): only the first consecutive failure is toasted. */
  async loadAgenda(date: Date = new Date(), quiet = false) {
    agendaLoading = true;
    try {
      const { start, end } = dayBounds(date);
      agenda = await api.reminderAgenda(start, end);
      agendaQuietFailed = false;
    } catch (e) {
      if (!quiet || !agendaQuietFailed) toast.error(e);
      if (quiet) agendaQuietFailed = true;
    } finally {
      agendaLoading = false;
    }
  },

  /** After a reminder fired anywhere: refresh every loaded list and the agenda. */
  async reloadAll() {
    await Promise.all([col.reloadAll(), this.loadAgenda(new Date(), true)]);
  },

  async add(
    targetType: TargetType,
    targetId: string,
    fireAt: string,
    message?: string | null,
  ): Promise<Reminder | null> {
    try {
      const r = await api.addReminder(targetType, targetId, fireAt, message);
      col.upsert(targetType, targetId, r);
      if (agenda) void this.loadAgenda();
      return r;
    } catch (e) {
      toast.error(e);
      return null;
    }
  },
  async snooze(id: string, until: string) {
    try {
      const r = await api.snoozeReminder(id, until);
      if (!col.replace(r)) void col.reloadAll();
      if (agenda) void this.loadAgenda();
    } catch (e) {
      toast.error(e);
    }
  },
  async remove(id: string) {
    try {
      await api.deleteReminder(id);
      col.remove(id);
      patchAgenda((l) => l.filter((r) => r.id !== id));
    } catch (e) {
      toast.error(e, { what: "reminder" });
    }
  },
};
