import * as api from "../api";
import type { ParentType, Task } from "../types";
import { parentKeyed } from "./collection.svelte";
import { toast } from "./ui.svelte";

const col = parentKeyed<Task>((pt, pid) => api.listTasks(pt as ParentType, pid));

/** Open tasks first in position order; done tasks last, most recently done first. */
export function sortTasks(list: Task[]): Task[] {
  return [...list].sort((a, b) => {
    if (!!a.done_at !== !!b.done_at) return a.done_at ? 1 : -1;
    if (a.done_at && b.done_at) return b.done_at.localeCompare(a.done_at);
    return a.position - b.position || a.created_at.localeCompare(b.created_at);
  });
}

export const tasks = {
  for(parentType: ParentType, parentId: string): Task[] {
    return sortTasks(col.get(parentType, parentId));
  },
  load: (parentType: ParentType, parentId: string) => col.load(parentType, parentId),
  async add(parentType: ParentType, parentId: string, title: string): Promise<Task | null> {
    const t = title.trim();
    if (!t) return null;
    try {
      const task = await api.addTask(parentType, parentId, t);
      col.upsert(parentType, parentId, task);
      return task;
    } catch (e) {
      toast.error(e);
      return null;
    }
  },
  async toggle(id: string) {
    try {
      const task = await api.toggleTask(id);
      col.replace(task);
    } catch (e) {
      toast.error(e);
    }
  },
  async rename(id: string, title: string) {
    const t = title.trim();
    if (!t) return;
    try {
      const task = await api.renameTask(id, t);
      col.replace(task);
    } catch (e) {
      toast.error(e);
    }
  },
  async remove(id: string) {
    try {
      await api.deleteTask(id);
      col.remove(id);
    } catch (e) {
      toast.error(e);
    }
  },
};
