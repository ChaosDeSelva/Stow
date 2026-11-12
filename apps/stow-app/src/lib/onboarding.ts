// "Get set up" checklist: which steps are done, from persisted flags plus live status.
import type { OnboardingStep } from "./types";

export const ONBOARDING_STEPS: OnboardingStep[] = [
  "captured",
  "automation",
  "notifications",
  "extension",
  "ai_key",
  "whisper",
];

export interface LiveStatus {
  automation?: string | null;
  notifications?: boolean | null;
  hasAiKey?: boolean | null;
  itemCount?: number;
}

export function stepDone(
  step: OnboardingStep,
  persisted: Partial<Record<OnboardingStep, boolean>> | undefined,
  live: LiveStatus,
): boolean {
  if (persisted?.[step]) return true;
  switch (step) {
    case "captured":
      return (live.itemCount ?? 0) > 0;
    case "automation":
      return live.automation === "granted";
    case "notifications":
      return live.notifications === true;
    case "ai_key":
      return live.hasAiKey === true;
    default:
      return false;
  }
}

export function doneCount(
  persisted: Partial<Record<OnboardingStep, boolean>> | undefined,
  live: LiveStatus,
): number {
  return ONBOARDING_STEPS.filter((s) => stepDone(s, persisted, live)).length;
}

/**
 * Patch for `update_settings` marking one step, or null when it is already set (no write loop).
 * Only the changed key is sent: the backend merges `onboarding_steps` into the stored object.
 */
export function markStep(
  persisted: Partial<Record<OnboardingStep, boolean>> | undefined,
  step: OnboardingStep,
): { onboarding_steps: Partial<Record<OnboardingStep, boolean>> } | null {
  if (persisted?.[step]) return null;
  return { onboarding_steps: { [step]: true } };
}
