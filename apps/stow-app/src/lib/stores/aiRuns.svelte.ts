import * as api from "../api";
import type { AiAction, AiRun, ParentType } from "../types";
import { parentKeyed } from "./collection.svelte";
import { toast } from "./ui.svelte";

const col = parentKeyed<AiRun>((pt, pid) => api.listAiRuns(pt as ParentType, pid));

export const aiRuns = {
  /** Newest first. */
  for: (parentType: ParentType, parentId: string): AiRun[] =>
    [...col.get(parentType, parentId)].sort((a, b) => b.created_at.localeCompare(a.created_at)),
  load: (parentType: ParentType, parentId: string) => col.load(parentType, parentId),
  reloadContaining: (id: string) => col.reloadContaining(id),
  async run(parentType: ParentType, parentId: string, action: AiAction): Promise<AiRun | null> {
    try {
      const r = await api.runAi(parentType, parentId, action);
      col.upsert(parentType, parentId, r, "start");
      // A fast `ai-run-updated` may have arrived before the row existed locally.
      void col.load(parentType, parentId);
      return r;
    } catch (e) {
      toast.error(e);
      return null;
    }
  },
};
