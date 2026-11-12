import * as api from "../api";
import type { ParentType, Recording } from "../types";
import { parentKeyed } from "./collection.svelte";
import { toast } from "./ui.svelte";

const col = parentKeyed<Recording>((pt, pid) => api.listRecordings(pt as ParentType, pid));

export interface RecordingSession {
  parentType: ParentType;
  parentId: string;
  startedAt: number;
}

let recording = $state(false);
/** The parent the current recording was started for; stop() always uses this. */
let session = $state<RecordingSession | null>(null);

export const recordings = {
  for: (parentType: ParentType, parentId: string): Recording[] =>
    [...col.get(parentType, parentId)].sort((a, b) => b.created_at.localeCompare(a.created_at)),
  load: (parentType: ParentType, parentId: string) => col.load(parentType, parentId),
  reloadContaining: (id: string) => col.reloadContaining(id),

  get isRecording() {
    return recording;
  },
  get session() {
    return session;
  },
  get startedAt() {
    return session?.startedAt ?? null;
  },
  reset() {
    recording = false;
    session = null;
  },
  /**
   * `recording-state` from the backend: recording started or stopped by ANY path (a
   * command from this window, or the tray's Stop). Stopping forgets the session; a start
   * we didn't make keeps whatever session is known (none → the bar shows no parent).
   */
  setRecording(on: boolean) {
    recording = on;
    if (!on) session = null;
  },
  /** Reconcile with the backend (e.g. on mount after a reload). */
  async syncState() {
    try {
      recording = await api.isRecording();
      if (!recording) session = null;
    } catch (e) {
      toast.error(e);
    }
  },
  async start(parentType: ParentType, parentId: string) {
    try {
      await api.startRecording(parentType, parentId);
      recording = true;
      session = { parentType, parentId, startedAt: Date.now() };
    } catch (e) {
      toast.error(e);
      recording = false;
      session = null;
    }
  },
  /**
   * Stop and attribute the recording to the parent captured at start. The
   * explicit arguments are only a fallback when no session is known (e.g. the
   * app was reloaded while recording).
   */
  async stop(fallbackType?: ParentType, fallbackId?: string): Promise<Recording | null> {
    const pt = session?.parentType ?? fallbackType;
    const pid = session?.parentId ?? fallbackId;
    if (!pt || !pid) {
      toast.error("Stow doesn't know which item this recording belongs to. Open the item and stop from there.");
      return null;
    }
    try {
      const r = await api.stopRecording(pt, pid);
      col.upsert(r.parent_type, r.parent_id, r, "start");
      // A fast `recording-updated` may have arrived before the row existed locally.
      void col.load(r.parent_type, r.parent_id);
      return r;
    } catch (e) {
      toast.error(e);
      return null;
    } finally {
      recording = false;
      session = null;
    }
  },
  async retranscribe(id: string) {
    try {
      await api.retranscribe(id);
      const o = col.ownerOf(id);
      if (o) {
        const cur = col.get(o.ownerType as ParentType, o.ownerId).find((r) => r.id === id);
        if (cur) col.replace({ ...cur, transcript_status: "pending", transcript_error: null });
      }
    } catch (e) {
      toast.error(e);
    }
  },
  async remove(id: string) {
    try {
      await api.deleteRecording(id);
      col.remove(id);
    } catch (e) {
      toast.error(e);
    }
  },
};
