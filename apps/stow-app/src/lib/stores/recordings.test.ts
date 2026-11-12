import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Recording } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: (p: string) => p }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import { invoke } from "@tauri-apps/api/core";
import { recordings } from "./recordings.svelte";

const mockInvoke = vi.mocked(invoke);

const rec: Recording = {
  id: "r1",
  parent_type: "item",
  parent_id: "itemA",
  audio_path: "/tmp/r1.wav",
  duration_ms: 12_000,
  transcript_md: null,
  transcript_status: "pending",
  transcript_error: null,
  created_at: "2026-11-08T10:00:00Z",
};

describe("recordings store", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    recordings.reset();
  });

  it("start sends the parent and remembers it as the active session", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await recordings.start("item", "itemA");
    expect(mockInvoke).toHaveBeenCalledWith("start_recording", { parentType: "item", parentId: "itemA" });
    expect(recordings.isRecording).toBe(true);
    expect(recordings.session).toMatchObject({ parentType: "item", parentId: "itemA" });
    expect(typeof recordings.session?.startedAt).toBe("number");
  });

  it("stop sends the parent captured at start, not whatever is mounted now", async () => {
    mockInvoke.mockResolvedValueOnce(undefined); // start
    await recordings.start("item", "itemA");
    mockInvoke.mockResolvedValueOnce(rec); // stop
    mockInvoke.mockResolvedValueOnce([rec]); // list_recordings reload after stop
    const got = await recordings.stop();
    expect(mockInvoke).toHaveBeenCalledWith("stop_recording", { parentType: "item", parentId: "itemA" });
    expect(mockInvoke).toHaveBeenCalledWith("list_recordings", { parentType: "item", parentId: "itemA" });
    expect(got).toEqual(rec);
    expect(recordings.isRecording).toBe(false);
    expect(recordings.session).toBeNull();
    // the new recording lands in the list of the parent it was started for
    expect(recordings.for("item", "itemA").map((r) => r.id)).toEqual(["r1"]);
  });

  it("stop without a known session falls back to the given parent", async () => {
    mockInvoke.mockResolvedValueOnce(rec);
    mockInvoke.mockResolvedValueOnce([rec]);
    await recordings.stop("bucket", "bk1");
    expect(mockInvoke).toHaveBeenCalledWith("stop_recording", { parentType: "bucket", parentId: "bk1" });
  });

  it("recording-state from the backend drives the flag; stopping (e.g. tray Stop) forgets the session", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await recordings.start("item", "itemA");
    recordings.setRecording(true); // the event for our own start: nothing changes
    expect(recordings.session).toMatchObject({ parentType: "item", parentId: "itemA" });
    recordings.setRecording(false);
    expect(recordings.isRecording).toBe(false);
    expect(recordings.session).toBeNull();
    recordings.setRecording(true); // started elsewhere: recording, parent unknown
    expect(recordings.isRecording).toBe(true);
    expect(recordings.session).toBeNull();
    expect(recordings.startedAt).toBeNull();
  });

  it("start failure leaves no session behind", async () => {
    mockInvoke.mockRejectedValueOnce("mic busy");
    await recordings.start("item", "itemA");
    expect(recordings.isRecording).toBe(false);
    expect(recordings.session).toBeNull();
  });
});
