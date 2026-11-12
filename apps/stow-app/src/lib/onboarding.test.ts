import { describe, expect, it } from "vitest";
import { doneCount, markStep, stepDone } from "./onboarding";

describe("onboarding checklist", () => {
  it("persisted flags win", () => {
    expect(stepDone("extension", { extension: true }, {})).toBe(true);
    expect(stepDone("extension", {}, {})).toBe(false);
  });

  it("live status completes steps without a write", () => {
    expect(stepDone("captured", {}, { itemCount: 2 })).toBe(true);
    expect(stepDone("automation", {}, { automation: "granted" })).toBe(true);
    expect(stepDone("automation", {}, { automation: "denied" })).toBe(false);
    expect(stepDone("notifications", {}, { notifications: true })).toBe(true);
    expect(stepDone("ai_key", {}, { hasAiKey: true })).toBe(true);
    expect(stepDone("whisper", {}, { hasAiKey: true })).toBe(false);
  });

  it("counts done steps", () => {
    expect(doneCount({ whisper: true }, { itemCount: 1, automation: "granted" })).toBe(3);
    expect(doneCount(undefined, {})).toBe(0);
  });

  it("markStep returns a patch only when the flag changes (no write loop)", () => {
    expect(markStep({ captured: true }, "captured")).toBeNull();
    // Only the changed key: update_settings merges onboarding_steps server-side.
    expect(markStep({ captured: true }, "extension")).toEqual({ onboarding_steps: { extension: true } });
    expect(markStep(undefined, "whisper")).toEqual({ onboarding_steps: { whisper: true } });
  });
});
