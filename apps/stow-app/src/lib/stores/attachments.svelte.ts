import * as api from "../api";
import type { Attachment, ParentType } from "../types";
import { parentKeyed } from "./collection.svelte";
import { toast } from "./ui.svelte";

const col = parentKeyed<Attachment>((pt, pid) => api.listAttachments(pt as ParentType, pid));

export const attachments = {
  for: (parentType: ParentType, parentId: string): Attachment[] => col.get(parentType, parentId),
  load: (parentType: ParentType, parentId: string) => col.load(parentType, parentId),
  /** Add several files; returns how many succeeded. Errors are toasted per file. */
  async addPaths(parentType: ParentType, parentId: string, paths: string[]): Promise<number> {
    let ok = 0;
    for (const p of paths) {
      try {
        const a = await api.addAttachment(parentType, parentId, p);
        col.upsert(parentType, parentId, a);
        ok++;
      } catch (e) {
        toast.error(e);
      }
    }
    return ok;
  },
  async remove(id: string) {
    try {
      await api.deleteAttachment(id);
      col.remove(id);
    } catch (e) {
      toast.error(e);
    }
  },
};
