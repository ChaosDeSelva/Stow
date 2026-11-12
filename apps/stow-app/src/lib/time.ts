// Local-time helpers. The backend speaks RFC 3339 UTC; the UI speaks local time.

export interface DayBounds {
  start: string;
  end: string;
}

/** Start of the given local day and start of the next local day, as RFC3339 UTC. */
export function dayBounds(date: Date = new Date()): DayBounds {
  const start = new Date(date.getFullYear(), date.getMonth(), date.getDate(), 0, 0, 0, 0);
  const end = new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1, 0, 0, 0, 0);
  return { start: start.toISOString(), end: end.toISOString() };
}

const pad = (n: number) => String(n).padStart(2, "0");

/** ISO/RFC3339 → `YYYY-MM-DDTHH:MM` in local time, for `<input type="datetime-local">`. */
export function toLocalInputValue(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** `YYYY-MM-DDTHH:MM[:SS]` local → RFC3339 UTC, or null when empty/invalid. */
export function fromLocalInputValue(v: string): string | null {
  if (!v) return null;
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/.exec(v);
  if (!m) return null;
  const d = new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], m[6] ? +m[6] : 0, 0);
  if (Number.isNaN(d.getTime())) return null;
  return d.toISOString();
}

/** Short relative phrase: "now", "5 min ago", "in 2 h", "3 d ago", "in 2 wk". */
export function formatRelative(iso: string, now: Date = new Date()): string {
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return "";
  const diff = t - now.getTime();
  const abs = Math.abs(diff);
  const sec = Math.round(abs / 1000);
  if (sec < 45) return "now";
  let n: number;
  let unit: string;
  if (sec < 3600) {
    n = Math.round(sec / 60);
    unit = "min";
  } else if (sec < 86400) {
    n = Math.round(sec / 3600);
    unit = "h";
  } else if (sec < 7 * 86400) {
    n = Math.round(sec / 86400);
    unit = "d";
  } else if (sec < 30 * 86400) {
    n = Math.round(sec / (7 * 86400));
    unit = "wk";
  } else if (sec < 365 * 86400) {
    n = Math.round(sec / (30 * 86400));
    unit = "mo";
  } else {
    n = Math.round(sec / (365 * 86400));
    unit = "yr";
  }
  return diff < 0 ? `${n} ${unit} ago` : `in ${n} ${unit}`;
}

/** Locale time of day, e.g. "9:00 AM" / "09:00". */
export function formatTime(d: Date): string {
  return d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

/** Absolute local date-time for display in the user's locale, e.g. "Nov 8, 2026, 7:00 PM". */
export function formatAbsolute(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${pad(m)}:${pad(s)}`;
}

/** `YYYY-MM-DD` for the local date (used for "Tabs <date>" bucket names). */
export function localDateStamp(d: Date = new Date()): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export interface Preset {
  label: string;
  fireAt: string;
}

const at = (base: Date, dayOffset: number, h: number, m = 0) =>
  new Date(base.getFullYear(), base.getMonth(), base.getDate() + dayOffset, h, m, 0, 0);

/** Next Monday 9:00 (never today). */
export function nextMonday(now: Date = new Date()): Date {
  const delta = ((8 - now.getDay()) % 7) || 7;
  return at(now, delta, 9);
}

/** Saturday 10:00 of this weekend; next weekend once that has passed. */
export function thisWeekend(now: Date = new Date()): Date {
  const delta = (6 - now.getDay() + 7) % 7;
  const sat = at(now, delta, 10);
  if (sat.getTime() > now.getTime()) return sat;
  if (now.getDay() === 6) {
    const sun = at(now, 1, 10);
    return sun;
  }
  return at(now, delta + 7, 10);
}

/** Quick reminder presets (+1h · Tonight · Tomorrow 9:00 · Next week); "Tonight" only while ahead. */
export function presets(now: Date = new Date()): Preset[] {
  const out: Preset[] = [];
  out.push({ label: "+1h", fireAt: new Date(now.getTime() + 3600_000).toISOString() });
  const tonight = at(now, 0, 19);
  if (tonight.getTime() > now.getTime()) out.push({ label: "Tonight", fireAt: tonight.toISOString() });
  const tomorrow = at(now, 1, 9);
  out.push({ label: `Tomorrow ${formatTime(tomorrow)}`, fireAt: tomorrow.toISOString() });
  out.push({ label: "Next week", fireAt: at(now, 7, 9).toISOString() });
  return out;
}

/** Presets plus "This weekend" and "Next Monday", for the reminder form. */
export function formPresets(now: Date = new Date()): Preset[] {
  return [
    ...presets(now),
    { label: "This weekend", fireAt: thisWeekend(now).toISOString() },
    { label: "Next Monday", fireAt: nextMonday(now).toISOString() },
  ];
}

export function tomorrowNine(now: Date = new Date()): string {
  return at(now, 1, 9).toISOString();
}

export function inOneHour(now: Date = new Date()): string {
  return new Date(now.getTime() + 3600_000).toISOString();
}
