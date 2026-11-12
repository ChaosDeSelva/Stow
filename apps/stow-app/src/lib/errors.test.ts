import { describe, expect, it } from "vitest";
import { errorCode, friendlyError, friendlyText } from "./errors";

describe("friendlyError: structured CmdError codes", () => {
  const cases: [string, { code: string; message: string }, string][] = [
    ["db", { code: "db", message: "database error: locked" }, "Stow couldn't save that change. Try again; if it keeps happening, restart Stow."],
    ["io disk full", { code: "io", message: "io error: No space left on device" }, "Your Mac is out of disk space. Free up some space and try again."],
    ["io other", { code: "io", message: "io error: permission" }, "Stow couldn't read or write a file on this Mac. Try again."],
    ["not_found", { code: "not_found", message: "not found" }, "That item was deleted."],
    ["invalid_url", { code: "invalid_url", message: "invalid url: x" }, "That doesn't look like a web address (http:// or https://)."],
    ["http", { code: "http", message: "http error: dns" }, "Couldn't reach the site. Check your connection and try again."],
    ["keychain", { code: "keychain", message: "keychain error: denied" }, "macOS Keychain refused access. Allow Stow in the Keychain prompt and try again."],
    ["audio no input", { code: "audio", message: "audio error: no input device" }, "No microphone found. Connect one or check Sound settings."],
    ["audio already", { code: "audio", message: "audio error: already recording" }, "A recording is already running. Stop it first."],
    ["transcribe download", { code: "transcribe", message: "transcription error: model download failed" }, "Couldn't download the transcription model. Check your connection, then try Transcribe again."],
    ["provider no key", { code: "provider", message: "anthropic: no api key" }, "Add your Anthropic API key in Settings → AI."],
    ["provider 401", { code: "provider", message: "openai 401 Unauthorized" }, "OpenAI rejected the API key. Paste a new one in Settings → AI."],
    ["provider 429", { code: "provider", message: "anthropic 429 rate limited" }, "Anthropic is rate-limiting requests. Wait a minute and try again."],
    ["provider 5xx", { code: "provider", message: "anthropic 529 overloaded" }, "Anthropic is having trouble right now. Try again in a few minutes."],
    ["hotkey", { code: "hotkey", message: "could not register Ctrl+Cmd+S" }, "That shortcut can't be used. Try another combination."],
    ["busy", { code: "busy", message: "That page is already being fetched." }, "That page is already being fetched."],
    ["chrome_not_running", { code: "chrome_not_running", message: "chrome_not_running" }, "Chrome isn't running."],
    ["permission_denied", { code: "permission_denied", message: "permission_denied" }, "Stow isn't allowed to control Chrome. Open Privacy Settings and turn on Google Chrome under Stow."],
    ["no_tab", { code: "no_tab", message: "no_tab" }, "No Chrome tab is open."],
    ["script_error", { code: "script_error", message: "script_error: -1712" }, "Chrome didn't respond. Make sure a normal Chrome window is open."],
  ];
  it.each(cases)("%s", (_name, err, title) => {
    expect(friendlyError(err).title).toBe(title);
  });

  it("keeps the raw message as detail", () => {
    expect(friendlyError({ code: "db", message: "database error: locked" }).detail).toBe("database error: locked");
  });

  it("not_found names the context", () => {
    expect(friendlyText({ code: "not_found", message: "x" }, { what: "bucket" })).toBe("That bucket was deleted.");
    expect(friendlyText({ code: "not_found", message: "x" }, { what: "task" })).toBe("That task was deleted.");
  });

  it("provider falls back to the context provider when the message doesn't name one", () => {
    expect(friendlyText({ code: "provider", message: "no api key" }, { provider: "openai" })).toBe(
      "Add your OpenAI API key in Settings → AI.",
    );
  });
});

describe("friendlyError: legacy string errors", () => {
  const cases: [string, string][] = [
    ["database error: disk I/O", "Stow couldn't save that change. Try again; if it keeps happening, restart Stow."],
    ["io error: No space left on device (os error 28)", "Your Mac is out of disk space. Free up some space and try again."],
    ["not found: item 01H", "That item was deleted."],
    ["invalid url: chrome://newtab", "That doesn't look like a web address (http:// or https://)."],
    ["http error: timed out", "Couldn't reach the site. Check your connection and try again."],
    ["keychain error: user canceled", "macOS Keychain refused access. Allow Stow in the Keychain prompt and try again."],
    ["audio error: no input device", "No microphone found. Connect one or check Sound settings."],
    ["audio error: already recording", "A recording is already running. Stop it first."],
    ["transcription error: download failed: connection reset", "Couldn't download the transcription model. Check your connection, then try Transcribe again."],
    ["ai error: no api key for anthropic", "Add your Anthropic API key in Settings → AI."],
    ["ai error: anthropic 401", "Anthropic rejected the API key. Paste a new one in Settings → AI."],
    ["ai error: openai 429 Too Many Requests", "OpenAI is rate-limiting requests. Wait a minute and try again."],
    ["ai error: anthropic 503", "Anthropic is having trouble right now. Try again in a few minutes."],
    ["invalid hotkey: Foo", "That shortcut can't be used. Try another combination."],
    ["could not register shortcut", "That shortcut can't be used. Try another combination."],
    ["chrome_not_running", "Chrome isn't running."],
    ["permission_denied", "Stow isn't allowed to control Chrome. Open Privacy Settings and turn on Google Chrome under Stow."],
    ["no_tab", "No Chrome tab is open."],
    ["script_error: execution error", "Chrome didn't respond. Make sure a normal Chrome window is open."],
  ];
  it.each(cases)("%s", (raw, title) => {
    expect(friendlyError(raw).title).toBe(title);
    expect(friendlyError(new Error(raw)).title).toBe(title);
  });

  it("unknown strings are shown as-is without detail", () => {
    expect(friendlyError("Select an item first.")).toEqual({ title: "Select an item first." });
  });

  it("errorCode infers codes from legacy prefixes", () => {
    expect(errorCode("chrome_not_running")).toBe("chrome_not_running");
    expect(errorCode({ code: "io", message: "x" })).toBe("io");
    expect(errorCode("whatever")).toBeNull();
  });
});
