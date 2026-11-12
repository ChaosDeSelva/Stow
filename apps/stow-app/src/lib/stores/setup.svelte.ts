// One source for "Get set up" progress, so the sidebar badge, the checklist
// footer and the first-launch default all count the same steps.
import { doneCount, ONBOARDING_STEPS, stepDone, type LiveStatus } from "../onboarding";
import type { OnboardingStep } from "../types";
import { items } from "./items.svelte";
import { settings } from "./settings.svelte";
import { system } from "./system.svelte";

export const setup = {
  get live(): LiveStatus {
    return {
      automation: system.automation,
      notifications: system.notifications,
      hasAiKey: system.anyKey,
      itemCount: items.counts.all ?? items.inboxCount,
    };
  },
  isDone(step: OnboardingStep): boolean {
    return stepDone(step, settings.value?.onboarding_steps, this.live);
  },
  get done(): number {
    return doneCount(settings.value?.onboarding_steps, this.live);
  },
  get total(): number {
    return ONBOARDING_STEPS.length;
  },
  get complete(): boolean {
    return this.done >= this.total;
  },
  /** The sidebar entry shows until the checklist is dismissed or every step is done. */
  get offered(): boolean {
    return !!settings.value && !settings.value.onboarding_dismissed && !this.complete;
  },
};
