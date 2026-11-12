<script lang="ts">
  import { onMount } from "svelte";
  import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import * as api from "../api";
  import { copyText } from "../clipboard";
  import { friendlyText, PROVIDER_LABEL } from "../errors";
  import { rovingIndex } from "../focus";
  import { formatAbsolute, localDateStamp } from "../time";
  import { humanSize } from "../util";
  import { DEFAULT_MODELS, KNOWN_MODELS, settings } from "../stores/settings.svelte";
  import { permissionLabel, system } from "../stores/system.svelte";
  import { toast, ui } from "../stores/ui.svelte";
  import type { AiProvider, BuildInfo, PrivacyPane, ThemeSetting, WhisperModel } from "../types";
  import ConfirmInline from "./ConfirmInline.svelte";
  import HotkeyRecorder from "./HotkeyRecorder.svelte";
  import Icon from "./Icon.svelte";
  import ImportTabsDialog from "./ImportTabsDialog.svelte";

  const s = $derived(settings.value);
  let keyDrafts = $state<Record<AiProvider, string>>({ anthropic: "", openai: "" });
  let customModel = $state(false);
  let modelDraft = $state("");
  let importOpen = $state(false);
  let appVersion = $state("");
  let build = $state<BuildInfo | null>(null);
  /** Which Whisper models are on disk (null = the backend can't tell); drives "(N MB)". */
  let modelsOnDisk = $state<Record<WhisperModel, boolean | null>>({ "tiny.en": null, "base.en": null, "small.en": null });
  let confirmModels = $state(false);
  let resetOpen = $state(false);
  let resetDraft = $state("");
  /** reset_all_data was sent: the window goes away when the process restarts. */
  let resetting = $state(false);
  let extensionDir = $state<string | null>(null);
  let pairing = $state<string | null>(null);
  let confirmRotate = $state(false);
  let confirmRotateDanger = $state(false);
  let whisperOnDisk = $state<boolean | null>(null);
  let testResult = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let extensionIdDraft = $state("");
  let extensionIdError = $state<string | null>(null);
  /** allow_local_urls was changed this session: the fetcher reads it at startup. */
  let localUrlsChanged = $state(false);

  const PROVIDERS: AiProvider[] = ["anthropic", "openai"];
  const WHISPER: { id: WhisperModel; label: string; mb: number }[] = [
    { id: "tiny.en", label: "tiny.en — 75 MB · fastest", mb: 75 },
    { id: "base.en", label: "base.en — 142 MB · recommended", mb: 142 },
    { id: "small.en", label: "small.en — 466 MB · most accurate", mb: 466 },
  ];
  /** Nominal size of the downloaded models; null when the backend can't say which exist. */
  const modelsMb = $derived.by(() => {
    if (WHISPER.every((w) => modelsOnDisk[w.id] === null)) return null;
    return WHISPER.reduce((sum, w) => sum + (modelsOnDisk[w.id] ? w.mb : 0), 0);
  });
  const THEMES: { id: ThemeSetting; label: string }[] = [
    { id: "light", label: "Light" },
    { id: "dark", label: "Dark" },
    { id: "system", label: "System" },
  ];

  // One-shot initial load. Deliberately NOT a $effect: a synchronous failure inside a
  // tracked effect that also writes state it reads would re-run forever.
  onMount(() => {
    void settings.load().then((v) => {
      if (!v) return;
      modelDraft = v.ai_model;
      extensionIdDraft = v.extension_id ?? "";
      customModel = !!v.ai_model && !KNOWN_MODELS[v.ai_provider].includes(v.ai_model);
      void refreshPairing();
      void refreshModels(v.whisper_model);
    });
    void system.refreshKeys();
    void system.refreshPermissions();
    void system.refreshLoginItem();
    api
      .appInfo()
      .then((i) => {
        appVersion = i.version;
        extensionDir = i.extension_dir ?? null;
        build = i.build ?? null;
      })
      .catch(() => {});
  });

  /** One probe per model: the configured one feeds "Downloaded", all of them the danger-zone size. */
  async function refreshModels(configured: WhisperModel) {
    await Promise.all(
      WHISPER.map((w) =>
        api
          .whisperModelDownloaded(w.id)
          .then((d) => (modelsOnDisk[w.id] = d))
          .catch(() => (modelsOnDisk[w.id] = null)),
      ),
    );
    whisperOnDisk = modelsOnDisk[configured];
  }

  async function refreshPairing() {
    try {
      pairing = await api.pairingCodeOr(settings.value);
    } catch {
      pairing = null;
    }
  }

  // ---- General
  function onThemeKey(e: KeyboardEvent) {
    const cur = THEMES.findIndex((t) => t.id === (s?.theme ?? "light"));
    const n = rovingIndex(e.key, cur, THEMES.length, "horizontal") ?? rovingIndex(e.key, cur, THEMES.length, "vertical");
    if (n == null) return;
    e.preventDefault();
    void settings.update({ theme: THEMES[n].id });
    queueMicrotask(() => (e.currentTarget as HTMLElement | null)?.querySelectorAll<HTMLElement>("[role=radio]")[n]?.focus());
  }
  /** Turning the checklist back on also opens it (after the save, so the "dismissed" guard doesn't bounce it). */
  async function setShowSetup(on: boolean) {
    const r = await settings.update({ onboarding_dismissed: !on });
    if (on && r) ui.select({ kind: "setup" });
  }
  async function setLogin(on: boolean) {
    const r = await system.setLoginItem(on);
    if (r == null) toast.info("Couldn't change the login item. Add Stow in System Settings → General → Login Items.");
    else void settings.update({ open_at_login: r }, { quiet: true });
  }

  // ---- Capture
  /** A shortcut that fails to register rejects with code `hotkey`; nothing is saved. */
  async function applyHotkey(acc: string): Promise<string | null> {
    try {
      await settings.save({ hotkey: acc });
      return null;
    } catch (e) {
      return friendlyText(e);
    }
  }

  // ---- Permissions
  async function ask(kind: "automation" | "microphone" | "notifications") {
    if (kind === "automation") await system.askAutomation();
    else if (kind === "microphone") await system.askMicrophone();
    else await system.askNotifications();
  }
  function openPrivacy(pane: PrivacyPane) {
    api.openPrivacySettings(pane).catch((e) => toast.error(e));
  }
  async function testAutomation() {
    testResult = null;
    try {
      const tab = await api.captureFrontTab();
      testResult = `Read: ${tab.title || tab.url}`;
      void system.refreshPermissions();
    } catch (e) {
      toast.error(e);
    }
  }

  // ---- Extension
  function copy(text: string, what: string) {
    copyText(text)
      .then(() => toast.success(`${what} copied`))
      .catch((e) => toast.error(e));
  }
  async function rotate() {
    confirmRotate = false;
    confirmRotateDanger = false;
    try {
      pairing = await api.rotateExtensionToken();
      void settings.load();
      toast.success("New pairing code. Paste it into the extension's options.");
    } catch (e) {
      toast.error(e);
    }
  }
  function showExtensionFolder() {
    if (!extensionDir) return;
    revealItemInDir(extensionDir).catch((e) => toast.error(e));
  }
  /** Empty = any extension may call the local server; otherwise the 32-letter id pins CORS. */
  async function saveExtensionId() {
    const id = extensionIdDraft.trim();
    extensionIdError = null;
    if (id === (s?.extension_id ?? "")) return;
    if (id && !/^[a-p]{32}$/.test(id)) {
      extensionIdError = "An extension ID is 32 letters a–p (see chrome://extensions).";
      return;
    }
    const r = await settings.update({ extension_id: id });
    if (r) extensionIdDraft = r.extension_id ?? "";
  }

  // ---- AI
  async function setProvider(e: Event) {
    const p = (e.target as HTMLSelectElement).value as AiProvider;
    const r = await settings.update({ ai_provider: p, ai_model: DEFAULT_MODELS[p] });
    if (r) {
      modelDraft = r.ai_model;
      customModel = false;
    }
  }
  async function onModelSelect(e: Event) {
    const v = (e.target as HTMLSelectElement).value;
    if (v === "__custom__") {
      customModel = true;
      return;
    }
    customModel = false;
    modelDraft = v;
    await settings.update({ ai_model: v });
  }
  async function saveCustomModel() {
    const m = modelDraft.trim();
    if (m && m !== s?.ai_model) await settings.update({ ai_model: m });
  }
  async function saveKey(p: AiProvider) {
    const k = keyDrafts[p].trim();
    if (!k) return;
    try {
      await api.setApiKey(p, k);
      keyDrafts[p] = "";
      await system.refreshKey(p);
      toast.success(`${PROVIDER_LABEL[p]} key saved to Keychain`);
    } catch (e) {
      toast.error(e);
    }
  }
  async function removeKey(p: AiProvider) {
    try {
      await api.deleteApiKey(p);
      await system.refreshKey(p);
      toast.success(`${PROVIDER_LABEL[p]} key removed`);
    } catch (e) {
      toast.error(e);
    }
  }

  // ---- Transcription
  async function downloadModel() {
    if (!s) return;
    busy = "whisper";
    try {
      await api.downloadWhisperModel(s.whisper_model);
      whisperOnDisk = true;
      modelsOnDisk[s.whisper_model] = true;
    } catch (e) {
      toast.error(e);
    } finally {
      busy = null;
    }
  }

  // ---- Data
  async function setAllowLocalUrls(on: boolean) {
    const r = await settings.update({ allow_local_urls: on });
    if (r) localUrlsChanged = true;
  }
  function openDataDir() {
    if (s) api.revealDataDir().catch((e) => toast.error(e));
  }
  async function exportAs(format: "markdown" | "json") {
    try {
      const dir = await openDialog({ directory: true, multiple: false, title: "Choose a folder for the export" });
      const destDir = Array.isArray(dir) ? dir[0] : dir;
      if (!destDir) return;
      busy = format;
      const r = await api.exportLibrary(format, destDir);
      toast.success(`Exported ${r.items} items in ${r.buckets} buckets`, {
        actions: [{ label: "Show", run: () => void revealItemInDir(r.path).catch(() => {}) }],
      });
    } catch (e) {
      toast.error(e);
    } finally {
      busy = null;
    }
  }
  async function backup() {
    try {
      const dest = await saveDialog({ defaultPath: `stow-backup-${localDateStamp()}.sqlite`, title: "Back up database" });
      if (!dest) return;
      busy = "backup";
      const r = await api.backupDatabase(dest);
      toast.success(`Backed up ${Math.max(1, Math.round(r.bytes / 1024))} KB`, {
        actions: [{ label: "Show", run: () => void revealItemInDir(r.path).catch(() => {}) }],
      });
    } catch (e) {
      toast.error(e);
    } finally {
      busy = null;
    }
  }

  // ---- Danger zone
  async function deleteModels() {
    confirmModels = false;
    busy = "models";
    try {
      const r = await api.deleteWhisperModels();
      for (const w of WHISPER) modelsOnDisk[w.id] = false;
      whisperOnDisk = false;
      toast.success(`Deleted the downloaded models (${humanSize(r.deleted_bytes)} freed). They download again when needed.`);
    } catch (e) {
      toast.error(e);
    } finally {
      busy = null;
    }
  }
  const resetConfirmed = $derived(resetDraft.trim() === "delete");
  function closeReset() {
    resetOpen = false;
    resetDraft = "";
  }
  /** Rust refuses anything but exactly "delete"; the overlay stays until the process restarts. */
  async function resetAll() {
    if (!resetConfirmed || resetting) return;
    resetting = true;
    try {
      await api.resetAllData("delete");
      closeReset();
    } catch (e) {
      resetting = false;
      toast.error(e);
    }
  }

  const perms = $derived([
    {
      id: "automation" as const,
      name: "Chrome Automation",
      purpose: "Reads the address and title of Chrome's current tab when you save, and closes it if you ask.",
      status: system.automation,
    },
    {
      id: "notifications" as const,
      name: "Notifications",
      purpose: "Shows your reminders, even when the Stow window is closed.",
      status: system.notifications,
    },
    {
      id: "microphone" as const,
      name: "Microphone",
      purpose: "Records voice notes. Transcripts are made on this Mac; audio never leaves it.",
      status: system.microphone,
    },
  ]);
</script>

<div class="settings">
  <h1 class="page-title">Settings</h1>

  {#if !s}
    <div class="muted small" role="status">Loading…</div>
  {:else}
    <section aria-labelledby="set-general">
      <h2 class="section-title" id="set-general">General</h2>
      <div class="grid">
        <span class="label-like" id="theme-label">Appearance</span>
        <div class="stack">
          <!-- svelte-ignore a11y_interactive_supports_focus -->
          <div class="product-tabs" role="radiogroup" aria-labelledby="theme-label" onkeydown={onThemeKey}>
            {#each THEMES as t (t.id)}
              {@const on = (s.theme ?? "light") === t.id}
              <button type="button" role="radio" aria-checked={on} tabindex={on ? 0 : -1} class:active={on} onclick={() => settings.update({ theme: t.id })}>{t.label}</button>
            {/each}
          </div>
          <span class="muted small">Light is the default. System follows your Mac's appearance.</span>
        </div>

        <span></span>
        <label class="check">
          <input type="checkbox" checked={!!(system.loginItem ?? s.open_at_login)} onchange={(e) => setLogin((e.target as HTMLInputElement).checked)} />
          Open Stow at login
        </label>

        <span class="label-like" id="close-label">When I close the window</span>
        <div class="stack" role="radiogroup" aria-labelledby="close-label">
          <label class="check"><input type="radio" name="quit" checked={!s.quit_on_close} onchange={() => settings.update({ quit_on_close: false })} /> Keep Stow in the menu bar</label>
          <label class="check"><input type="radio" name="quit" checked={!!s.quit_on_close} onchange={() => settings.update({ quit_on_close: true })} /> Quit Stow</label>
          <span class="muted small">Reminders only fire while Stow is running.</span>
        </div>

        <span></span>
        <label class="check">
          <input type="checkbox" checked={!s.onboarding_dismissed} onchange={(e) => setShowSetup((e.target as HTMLInputElement).checked)} />
          Show setup checklist
        </label>
      </div>
    </section>

    <section aria-labelledby="set-capture">
      <h2 class="section-title" id="set-capture">Capture</h2>
      <div class="grid">
        <span class="label-like">Quick capture shortcut</span>
        <HotkeyRecorder value={s.hotkey} onApply={applyHotkey} registered={s.hotkey_registered ?? null} error={s.hotkey_error ?? null} />

        <span></span>
        <label class="check">
          <input
            type="checkbox"
            checked={s.close_tab_after_capture}
            onchange={(e) => settings.update({ close_tab_after_capture: (e.target as HTMLInputElement).checked })}
          />
          Close the tab after saving
        </label>

        <span></span>
        <div class="row">
          <button type="button" onclick={() => (importOpen = true)}><Icon name="chrome" style="brands" size={13} /> Import all Chrome tabs…</button>
          <span class="muted small">Saves every open tab into a bucket.</span>
        </div>
      </div>
    </section>

    <section aria-labelledby="set-perms">
      <h2 class="section-title" id="set-perms">Permissions</h2>
      <ul class="perms">
        {#each perms as p (p.id)}
          {@const st = permissionLabel(p.status)}
          <li>
            <div class="pinfo">
              <div class="pname">{p.name}</div>
              <div class="muted small">{p.purpose}</div>
            </div>
            <span class="pstatus small"><span class="status-dot {st.tone}" role="img" aria-label={st.text}></span> <span aria-hidden="true">{st.text}</span></span>
            <div class="pbtns">
              {#if st.tone === "unknown"}<button type="button" class="small" onclick={() => ask(p.id)}>Ask now</button>{/if}
              <button type="button" class="small ghost" onclick={() => openPrivacy(p.id)}>Open Privacy Settings</button>
              {#if p.id === "automation"}<button type="button" class="small ghost" onclick={testAutomation}>Test</button>{/if}
            </div>
            {#if p.id === "automation" && testResult}<div class="small ok-text full" role="status"><Icon name="check" size={11} /> {testResult}</div>{/if}
          </li>
        {/each}
      </ul>
    </section>

    <section aria-labelledby="set-ext">
      <h2 class="section-title" id="set-ext">Chrome extension</h2>
      <div class="grid">
        <label for="pairing">Pairing code</label>
        <div class="row">
          <input id="pairing" type="text" class="mono grow" readonly value={pairing ?? ""} />
          <button type="button" class="small" disabled={!pairing} onclick={() => pairing && copy(pairing, "Pairing code")}><Icon name="copy" style="regular" size={11} /> Copy pairing code</button>
          <button type="button" class="small ghost" onclick={() => (confirmRotate = true)}>Reset pairing code</button>
        </div>
        {#if confirmRotate}
          <span></span>
          <ConfirmInline message="Reset the pairing code? The extension stops working until you paste the new code." confirmLabel="Reset" onConfirm={rotate} onCancel={() => (confirmRotate = false)} />
        {/if}
        <span class="label-like">Install</span>
        <ol class="steps small">
          <li>Open <span class="mono">chrome://extensions</span> and turn on <strong>Developer mode</strong> (top right).</li>
          <li>Click <strong>Load unpacked</strong> and choose the Stow extension folder.</li>
          <li>Open the extension's options and paste the pairing code — or just click Allow when Stow asks.</li>
        </ol>
        <span></span>
        {#if extensionDir}
          <div><button type="button" class="small" onclick={showExtensionFolder}><Icon name="folder-open" size={11} /> Show extension folder</button></div>
        {:else}
          <div class="muted small">The extension is the <span class="mono">extension/</span> folder of the Stow source code.</div>
        {/if}
        <span></span>
        <details class="advanced">
          <summary class="small">Advanced</summary>
          <div class="stack">
            <label for="extension-id" class="small">Extension ID (pins CORS)</label>
            <input
              id="extension-id"
              type="text"
              class="mono grow"
              placeholder="any extension"
              spellcheck="false"
              autocomplete="off"
              aria-invalid={!!extensionIdError}
              bind:value={extensionIdDraft}
              onblur={saveExtensionId}
              onkeydown={(e) => e.key === "Enter" && saveExtensionId()}
            />
            {#if extensionIdError}
              <span class="small warn" role="alert">{extensionIdError}</span>
            {:else}
              <span class="muted small">Only this extension may talk to Stow's local server. Leave empty to allow any extension that knows the pairing code.</span>
            {/if}
          </div>
        </details>
      </div>
    </section>

    <section aria-labelledby="set-ai">
      <h2 class="section-title" id="set-ai">AI</h2>
      <div class="grid">
        <label for="ai-provider">Provider</label>
        <select id="ai-provider" value={s.ai_provider} onchange={setProvider}>
          {#each PROVIDERS as p}<option value={p}>{PROVIDER_LABEL[p]}</option>{/each}
        </select>

        <label for="ai-model">Model</label>
        <div class="row">
          <select id="ai-model" value={customModel ? "__custom__" : s.ai_model || DEFAULT_MODELS[s.ai_provider]} onchange={onModelSelect}>
            {#each KNOWN_MODELS[s.ai_provider] as m}<option value={m}>{m}</option>{/each}
            <option value="__custom__">Custom…</option>
          </select>
          {#if customModel}
            <input
              type="text"
              class="grow mono"
              aria-label="Custom model name"
              placeholder="model id"
              bind:value={modelDraft}
              onblur={saveCustomModel}
              onkeydown={(e) => e.key === "Enter" && saveCustomModel()}
            />
          {/if}
        </div>

        {#each PROVIDERS as p (p)}
          <label for="key-{p}">{PROVIDER_LABEL[p]} API key</label>
          <div class="row">
            <input
              id="key-{p}"
              type="password"
              class="grow"
              placeholder={system.hasKey[p] ? "•••••••• (stored in Keychain)" : `Paste your ${PROVIDER_LABEL[p]} key`}
              autocomplete="off"
              bind:value={keyDrafts[p]}
              onblur={() => saveKey(p)}
              onkeydown={(e) => e.key === "Enter" && saveKey(p)}
            />
            {#if system.hasKey[p]}
              <span class="small ok-text"><Icon name="circle-check" size={11} /> Saved</span>
              <button type="button" class="small ghost danger" onclick={() => removeKey(p)}>Remove</button>
            {:else if system.hasKey[p] === false}
              <span class="small muted">No key</span>
            {/if}
          </div>
        {/each}

        <span></span>
        <div class="consent small">
          <Icon name="shield-halved" size={12} />
          <span>
            AI actions send the text they need (page text, notes or a transcript) to {PROVIDER_LABEL[s.ai_provider]} with your key — only when you click one.
            {#if s.ai_consent_given}
              <span class="ok-text"><Icon name="check" size={11} /> You've agreed to this.</span>
            {:else}
              <button type="button" class="small" onclick={() => settings.update({ ai_consent_given: true })}>I understand</button>
            {/if}
          </span>
        </div>
      </div>
    </section>

    <section aria-labelledby="set-whisper">
      <h2 class="section-title" id="set-whisper">Transcription</h2>
      <div class="grid">
        <label for="whisper">Transcription model</label>
        <div class="row">
          <select
            id="whisper"
            value={s.whisper_model}
            onchange={(e) => {
              whisperOnDisk = null;
              void settings.update({ whisper_model: (e.target as HTMLSelectElement).value as WhisperModel });
            }}
          >
            {#each WHISPER as w}<option value={w.id}>{w.label}</option>{/each}
          </select>
          {#if whisperOnDisk === true}
            <span class="small ok-text"><Icon name="check" size={11} /> Downloaded</span>
          {:else if whisperOnDisk === false}
            <button type="button" class="small" disabled={busy === "whisper"} onclick={downloadModel}>{busy === "whisper" ? "Downloading…" : "Download now"}</button>
          {/if}
        </div>
        <span></span>
        <span class="muted small">Transcripts are made on this Mac. The model downloads once, the first time you transcribe.</span>
      </div>
    </section>

    <section aria-labelledby="set-data">
      <h2 class="section-title" id="set-data">Data</h2>
      <div class="grid">
        <label for="data-dir">Data folder</label>
        <div class="row">
          <input id="data-dir" type="text" class="mono grow" readonly value={s.data_dir} />
          <button type="button" class="small" onclick={openDataDir}><Icon name="folder-open" size={11} /> Open in Finder</button>
        </div>
        <span class="label-like">Export</span>
        <div class="row wrap">
          <button type="button" class="small" disabled={!!busy} onclick={() => exportAs("markdown")}><Icon name="file-export" size={11} /> Export as Markdown…</button>
          <button type="button" class="small" disabled={!!busy} onclick={() => exportAs("json")}>Export as JSON…</button>
          <button type="button" class="small" disabled={!!busy} onclick={backup}><Icon name="database" size={11} /> Back up database…</button>
        </div>
        <span class="label-like">Advanced</span>
        <div class="stack">
          <label class="check">
            <input type="checkbox" checked={!!s.allow_local_urls} onchange={(e) => setAllowLocalUrls((e.target as HTMLInputElement).checked)} />
            Fetch pages on private or local addresses (e.g. http://localhost)
          </label>
          <span class="muted small">Off by default for safety. Takes effect after restarting Stow.</span>
          {#if localUrlsChanged}<span class="small warn" role="status">Restart Stow to apply.</span>{/if}
        </div>
        <span></span>
        <div class="muted small">Everything is stored on this Mac; API keys live in the macOS Keychain.</div>
      </div>

      <div class="danger-zone">
        <h3 class="section-title">Danger zone</h3>
        <div class="row">
          <span class="grow small">Reset the Chrome extension's pairing code. The extension must be paired again.</span>
          <button type="button" class="small outline-danger" onclick={() => (confirmRotateDanger = true)}>Reset pairing code</button>
        </div>
        {#if confirmRotateDanger}
          <ConfirmInline message="Reset the pairing code? The extension stops working until you paste the new code." confirmLabel="Reset" onConfirm={rotate} onCancel={() => (confirmRotateDanger = false)} />
        {/if}

        <div class="row">
          <span class="grow small">Remove the transcription models from this Mac. The next transcription downloads one again.</span>
          <button type="button" class="small outline-danger" disabled={!!busy || modelsMb === 0} onclick={() => (confirmModels = true)}>
            {busy === "models" ? "Deleting…" : modelsMb == null ? "Delete downloaded models" : modelsMb === 0 ? "No downloaded models" : `Delete downloaded models (${modelsMb} MB)`}
          </button>
        </div>
        {#if confirmModels}
          <ConfirmInline message="Delete the downloaded transcription models?" onConfirm={deleteModels} onCancel={() => (confirmModels = false)} />
        {/if}

        <div class="row">
          <span class="grow small">Delete every bucket, link, note, recording and setting, then start Stow over. This cannot be undone.</span>
          <button type="button" class="small outline-danger" disabled={resetOpen} onclick={() => (resetOpen = true)}>Delete all Stow data…</button>
        </div>
        {#if resetOpen}
          <div class="reset-confirm" role="alertdialog" aria-labelledby="reset-msg">
            <span id="reset-msg" class="small">Type <strong class="mono">delete</strong> to confirm. Stow restarts with an empty library.</span>
            <div class="row">
              <input
                type="text"
                class="mono grow"
                aria-label="Type delete to confirm"
                placeholder="delete"
                autocomplete="off"
                spellcheck="false"
                bind:value={resetDraft}
                onkeydown={(e) => {
                  if (e.key === "Escape") closeReset();
                  else if (e.key === "Enter") void resetAll();
                }}
              />
              <button type="button" class="small ghost" onclick={closeReset}>Cancel</button>
              <button type="button" class="small outline-danger" disabled={!resetConfirmed || resetting} onclick={resetAll}>Delete everything</button>
            </div>
          </div>
        {/if}
      </div>
    </section>

    <footer class="about muted small" aria-label="About">
      {#if build}
        Stow {build.version}{#if build.commit}&nbsp;· <span class="mono">{build.commit.slice(0, 9)}</span>{/if}
        {#if build.built_at && formatAbsolute(build.built_at)}&nbsp;· built {formatAbsolute(build.built_at)}{/if}
      {:else if appVersion}
        Stow {appVersion}
      {/if}
    </footer>
  {/if}
</div>

{#if resetting}
  <div class="resetting" role="alertdialog" aria-busy="true" aria-labelledby="resetting-msg">
    <div class="resetting-card">
      <div class="big" id="resetting-msg">Resetting… Stow will restart.</div>
      <div class="muted small">Deleting the data folder. Don't quit Stow.</div>
    </div>
  </div>
{/if}

{#if importOpen}
  <ImportTabsDialog onClose={() => (importOpen = false)} />
{/if}

<style>
  .settings {
    height: 100%;
    overflow: auto;
    padding: 20px 24px 32px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    max-width: 860px;
    background: var(--surface-card);
  }
  .page-title {
    margin: 0;
    font-size: 21px;
    letter-spacing: -0.5px;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 20px 24px 24px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: var(--surface-doc);
  }
  .grid {
    display: grid;
    grid-template-columns: 170px 1fr;
    align-items: center;
    row-gap: 12px;
    column-gap: 12px;
  }
  .grid > label:not(.check),
  .grid > .label-like {
    color: var(--mute);
    font-weight: 500;
    text-align: right;
    align-self: start;
    padding-top: 6px;
  }
  .stack {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .wrap {
    flex-wrap: wrap;
  }
  .perms {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .perms li {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 6px 12px;
    padding: 10px 0;
    border-top: 1px solid var(--hairline-soft);
  }
  .perms li:first-child {
    border-top: 0;
  }
  .pname {
    color: var(--ink);
    font-weight: 600;
  }
  .pstatus {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    justify-self: end;
  }
  .pbtns {
    grid-column: 1 / 3;
    display: flex;
    gap: 6px;
  }
  .full {
    grid-column: 1 / 3;
  }
  .steps {
    margin: 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .advanced > summary {
    cursor: default;
    color: var(--mute);
  }
  .advanced[open] > summary {
    margin-bottom: 8px;
  }
  .advanced .grow {
    width: 100%;
  }
  .warn {
    color: var(--warning-text);
  }
  .consent {
    display: flex;
    gap: 8px;
    align-items: flex-start;
  }
  .consent button {
    margin-left: 6px;
  }
  .danger-zone {
    margin-top: 8px;
    padding: 14px 16px;
    border: 1px solid var(--accent-red-soft);
    border-radius: var(--radius);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .reset-confirm {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border-radius: var(--radius);
    border: 1px solid var(--hairline);
    background: var(--accent-red-soft);
    color: var(--ink);
  }
  .about {
    padding: 0 4px;
  }
  .resetting {
    position: fixed;
    inset: 0;
    z-index: 1200;
    background: var(--scrim);
    display: grid;
    place-items: center;
    cursor: progress;
  }
  .resetting-card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: center;
    padding: 24px 32px;
    border-radius: var(--radius);
    border: 1px solid var(--stone);
    background: var(--surface-card);
    color: var(--ink);
  }
  .big {
    font-size: 15px;
    font-weight: 600;
  }
</style>
