/**
 * Debounced autosave with an explicit flush, so a pending edit is never lost
 * when the editor unmounts or is re-pointed at a different record.
 */
export interface Autosave<T> {
  schedule(value: T): void;
  /** Save the pending value now; returns the save's promise (if it made one) so quit can await it. */
  flush(): Promise<unknown> | undefined;
  cancel(): void;
  readonly pending: boolean;
}

export function createAutosave<T>(save: (value: T) => void | Promise<unknown>, delayMs = 600): Autosave<T> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pendingValue: T | undefined;
  let hasPending = false;
  let lastSaved: T | undefined;
  let hasSaved = false;

  const fire = (): Promise<unknown> | undefined => {
    timer = null;
    if (!hasPending) return undefined;
    const v = pendingValue as T;
    hasPending = false;
    pendingValue = undefined;
    if (hasSaved && v === lastSaved) return undefined;
    lastSaved = v;
    hasSaved = true;
    return save(v) ?? undefined;
  };

  return {
    schedule(value: T) {
      pendingValue = value;
      hasPending = true;
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => void fire(), delayMs);
    },
    flush() {
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
      return fire();
    },
    cancel() {
      if (timer) clearTimeout(timer);
      timer = null;
      hasPending = false;
      pendingValue = undefined;
    },
    get pending() {
      return hasPending;
    },
  };
}

// ---- Global registry: lets the app flush every live editor at once (app-will-quit).

const registry = new Set<Autosave<unknown>>();

/** Register a live autosave; returns the unregister function (call it on destroy). */
export function registerAutosave<T>(a: Autosave<T>): () => void {
  registry.add(a as Autosave<unknown>);
  return () => {
    registry.delete(a as Autosave<unknown>);
  };
}

/** Flush every registered autosave immediately; returns the in-flight save promises (app-will-quit awaits them). */
export function flushAll(): Promise<unknown>[] {
  const out: Promise<unknown>[] = [];
  for (const a of registry) {
    const p = a.flush();
    if (p) out.push(p);
  }
  return out;
}
