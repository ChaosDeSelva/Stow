import * as api from "../api";
import { applyTheme } from "../theme";
import type { Settings } from "../types";
import { toast } from "./ui.svelte";

let value = $state<Settings | null>(null);

export const DEFAULT_MODELS: Record<Settings["ai_provider"], string> = {
  anthropic: "claude-sonnet-5-5",
  openai: "gpt-5-mini",
};

/** Models offered in Settings → AI ("Custom…" covers anything else). */
export const KNOWN_MODELS: Record<Settings["ai_provider"], string[]> = {
  anthropic: ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"],
  openai: ["gpt-5", "gpt-5-mini"],
};

export const settings = {
  get value() {
    return value;
  },
  async load(): Promise<Settings | null> {
    try {
      value = await api.getSettings();
      applyTheme(value.theme ?? "light");
      return value;
    } catch (e) {
      toast.error(e);
      return null;
    }
  },
  /**
   * Apply a patch; rejects with the command error (for callers that show it inline).
   * On rejection nothing was saved and `value` is left as it was.
   */
  async save(patch: Partial<Settings>): Promise<Settings> {
    value = await api.updateSettings(patch);
    applyTheme(value.theme ?? "light");
    return value;
  },
  /** `quiet`: don't toast failures (background bookkeeping such as onboarding flags). */
  async update(patch: Partial<Settings>, opts: { quiet?: boolean } = {}): Promise<Settings | null> {
    try {
      return await this.save(patch);
    } catch (e) {
      if (!opts.quiet) toast.error(e);
      return null;
    }
  },
  /** Test helper. */
  set(v: Settings | null) {
    value = v;
  },
};
