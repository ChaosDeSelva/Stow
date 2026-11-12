import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushSync, mount, unmount } from "svelte";
import NotesEditor from "./NotesEditor.svelte";
import { flushAll } from "../autosave";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: (p: string) => p }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(), openPath: vi.fn() }));

function type(target: HTMLElement, text: string) {
  const ta = target.querySelector("textarea") as HTMLTextAreaElement;
  ta.value = text;
  ta.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

describe("NotesEditor", () => {
  let target: HTMLElement;
  beforeEach(() => {
    vi.useFakeTimers();
    target = document.createElement("div");
    document.body.appendChild(target);
  });
  afterEach(() => {
    vi.useRealTimers();
    target.remove();
  });

  it("flushes the pending edit exactly once on unmount, with the record key and latest text", () => {
    const onSave = vi.fn();
    const app = mount(NotesEditor, {
      target,
      props: { value: "hello", onSave, recordKey: "item:1" },
    });
    flushSync();
    type(target, "hello w");
    type(target, "hello world");
    expect(onSave).not.toHaveBeenCalled();
    unmount(app);
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith("item:1", "hello world");
    vi.advanceTimersByTime(2000);
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it("saves after the 600 ms debounce without unmounting", () => {
    const onSave = vi.fn();
    const app = mount(NotesEditor, { target, props: { value: "", onSave, recordKey: "item:1" } });
    flushSync();
    type(target, "draft");
    vi.advanceTimersByTime(599);
    expect(onSave).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onSave).toHaveBeenCalledWith("item:1", "draft");
    unmount(app);
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it("record changes without blur: the flush goes to the OLD key, the new record is shown untouched", () => {
    const onSave = vi.fn();
    let props = $state({ value: "A notes", onSave, recordKey: "bucket:A" });
    const app = mount(NotesEditor, { target, props });
    flushSync();
    type(target, "A notes edited");

    // The parent re-points the editor (same onSave function, new key + value) without any blur.
    props.value = "B notes";
    props.recordKey = "bucket:B";
    flushSync();

    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith("bucket:A", "A notes edited");
    const ta = target.querySelector("textarea") as HTMLTextAreaElement;
    expect(ta.value).toBe("B notes");

    vi.advanceTimersByTime(2000);
    unmount(app);
    // Nothing was typed into B, so B is never saved.
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it("uses the onSave that was current when the record was bound", () => {
    const saveA = vi.fn();
    const saveB = vi.fn();
    let props = $state({ value: "A", onSave: saveA, recordKey: "item:A" });
    const app = mount(NotesEditor, { target, props });
    flushSync();
    type(target, "A2");
    props.value = "B";
    props.onSave = saveB;
    props.recordKey = "item:B";
    flushSync();
    expect(saveA).toHaveBeenCalledWith("item:A", "A2");
    expect(saveB).not.toHaveBeenCalled();
    type(target, "B2");
    unmount(app);
    expect(saveB).toHaveBeenCalledWith("item:B", "B2");
    expect(saveA).toHaveBeenCalledTimes(1);
  });

  it("flushAll (app-will-quit) hands back the store's save promise and settles only after it", async () => {
    let finish!: () => void;
    const onSave = vi.fn(() => new Promise<void>((r) => (finish = r)));
    const app = mount(NotesEditor, { target, props: { value: "", onSave, recordKey: "item:1" } });
    flushSync();
    type(target, "typed right before ⌘Q");
    const promises = flushAll();
    expect(onSave).toHaveBeenCalledWith("item:1", "typed right before ⌘Q");
    expect(promises).toHaveLength(1);
    let settled = false;
    void Promise.allSettled(promises).then(() => (settled = true));
    for (let i = 0; i < 5; i++) await Promise.resolve();
    expect(settled).toBe(false);
    expect(target.textContent).toContain("Saving…");
    finish();
    await Promise.allSettled(promises);
    expect(settled).toBe(true);
    flushSync();
    expect(target.textContent).toContain("Saved");
    expect(flushAll()).toEqual([]);
    unmount(app);
  });

  it("the exported flush() returns the save promise too", async () => {
    const onSave = vi.fn(async () => {});
    const app = mount(NotesEditor, { target, props: { value: "", onSave, recordKey: "item:2" } });
    flushSync();
    type(target, "x");
    const p = app.flush();
    expect(p).toBeInstanceOf(Promise);
    await p;
    expect(onSave).toHaveBeenCalledTimes(1);
    unmount(app);
  });
});
