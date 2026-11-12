// Command errors → user copy. Commands reject with `CmdError {code, message}`
// (tauri-contract.md "v0.1 hardening"); older builds reject with bare strings,
// so every code also has a string-prefix fallback. `detail` keeps the raw
// message for the "Details" disclosure.

export interface FriendlyError {
  title: string;
  detail?: string;
}

export interface CmdError {
  code: string;
  message: string;
}

export interface ErrorContext {
  /** What a not_found refers to ("That item was deleted."). */
  what?: "item" | "bucket" | "task" | "reminder" | "recording" | "file";
  /** Provider to name when the message doesn't say. */
  provider?: "anthropic" | "openai";
}

export function isCmdError(e: unknown): e is CmdError {
  return (
    typeof e === "object" &&
    e !== null &&
    typeof (e as CmdError).code === "string" &&
    typeof (e as CmdError).message === "string"
  );
}

/** Raw message of any rejection value. */
export function errorMessage(e: unknown): string {
  if (isCmdError(e)) return e.message;
  if (e instanceof Error) return e.message;
  if (typeof e === "string") return e;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

/** Contract code of a rejection, inferred from legacy strings when absent. */
export function errorCode(e: unknown): string | null {
  if (isCmdError(e)) return e.code;
  const s = errorMessage(e).toLowerCase();
  const prefixes: [string, string][] = [
    ["database error", "db"],
    ["io error", "io"],
    ["not found", "not_found"],
    ["invalid url", "invalid_url"],
    ["invalid input", "invalid_input"],
    ["http error", "http"],
    ["keychain error", "keychain"],
    ["audio error", "audio"],
    ["transcription error", "transcribe"],
    ["ai error", "provider"],
    ["invalid hotkey", "hotkey"],
    ["could not register", "hotkey"],
    ["chrome_not_running", "chrome_not_running"],
    ["permission_denied", "permission_denied"],
    ["no_tab", "no_tab"],
    ["script_error", "script_error"],
  ];
  for (const [p, code] of prefixes) if (s.startsWith(p)) return code;
  if (/\bnot found\b/.test(s)) return "not_found";
  return null;
}

export const PROVIDER_LABEL = { anthropic: "Anthropic", openai: "OpenAI" } as const;

function providerName(msg: string, ctx: ErrorContext): string | null {
  if (/anthropic|claude/i.test(msg)) return "Anthropic";
  if (/openai|gpt/i.test(msg)) return "OpenAI";
  return ctx.provider ? PROVIDER_LABEL[ctx.provider] : null;
}

function providerCopy(msg: string, ctx: ErrorContext): string {
  const p = providerName(msg, ctx);
  const P = p ?? "The AI provider";
  if (/no api key|missing api key|api key not set/i.test(msg))
    return p ? `Add your ${p} API key in Settings → AI.` : "Add your API key in Settings → AI.";
  if (/\b(401|403)\b|unauthori[sz]ed|invalid.*api key/i.test(msg))
    return `${P} rejected the API key. Paste a new one in Settings → AI.`;
  if (/\b429\b|rate.?limit/i.test(msg)) return `${P} is rate-limiting requests. Wait a minute and try again.`;
  if (/\b5\d\d\b|overloaded/i.test(msg)) return `${P} is having trouble right now. Try again in a few minutes.`;
  return `${P} couldn't complete that request. Try again.`;
}

function titleFor(code: string | null, msg: string, ctx: ErrorContext): string | null {
  switch (code) {
    case "db":
      return "Stow couldn't save that change. Try again; if it keeps happening, restart Stow.";
    case "io":
      return /no space left|disk full|enospc/i.test(msg)
        ? "Your Mac is out of disk space. Free up some space and try again."
        : "Stow couldn't read or write a file on this Mac. Try again.";
    case "not_found":
      return `That ${ctx.what ?? "item"} was deleted.`;
    case "invalid_url":
      return "That doesn't look like a web address (http:// or https://).";
    case "invalid_input":
      return "Stow couldn't use that value. Check it and try again.";
    case "http":
      return "Couldn't reach the site. Check your connection and try again.";
    case "not_html":
      return "That address isn't a web page, so there's no readable copy.";
    case "too_large":
      return "That page is too large to save a readable copy.";
    case "keychain":
      return "macOS Keychain refused access. Allow Stow in the Keychain prompt and try again.";
    case "audio":
      if (/no input device/i.test(msg)) return "No microphone found. Connect one or check Sound settings.";
      if (/already recording/i.test(msg)) return "A recording is already running. Stop it first.";
      if (/permission|denied|not allowed/i.test(msg))
        return "Stow isn't allowed to use the microphone. Turn it on in Privacy Settings.";
      return "Couldn't record audio. Try again.";
    case "transcribe":
      if (/download|network|connect|http/i.test(msg))
        return "Couldn't download the transcription model. Check your connection, then try Transcribe again.";
      return "Couldn't transcribe this recording. Try Transcribe again.";
    case "provider":
      return providerCopy(msg, ctx);
    case "hotkey":
      return "That shortcut can't be used. Try another combination.";
    case "chrome_not_running":
      return "Chrome isn't running.";
    case "permission_denied":
      return "Stow isn't allowed to control Chrome. Open Privacy Settings and turn on Google Chrome under Stow.";
    case "no_tab":
      return "No Chrome tab is open.";
    case "script_error":
      return "Chrome didn't respond. Make sure a normal Chrome window is open.";
    case "internal":
      return "Something went wrong inside Stow. Try again.";
    case "busy":
      // refetch_item / retranscribe: the message is already user-ready ("That page is already being fetched.").
      return msg || "Stow is already working on that.";
  }
  // Bare provider strings ("401 Unauthorized", "anthropic 529 overloaded") from older builds.
  if (/^(anthropic|openai)\b/i.test(msg) || /^\d{3}\b/.test(msg)) {
    if (/\b(401|403|429|5\d\d)\b/.test(msg)) return providerCopy(msg, ctx);
  }
  return null;
}

/** Map any rejection to `{title, detail}`; unknown strings are shown as-is. */
export function friendlyError(e: unknown, ctx: ErrorContext = {}): FriendlyError {
  const msg = errorMessage(e).trim();
  const title = titleFor(errorCode(e), msg, ctx);
  if (!title) return { title: msg || "Something went wrong." };
  return msg && msg !== title ? { title, detail: msg } : { title };
}

/** One-line copy for inline error spots (snapshot_error, transcript_error, ai_runs.error). */
export function friendlyText(e: unknown, ctx: ErrorContext = {}): string {
  return friendlyError(e, ctx).title;
}
