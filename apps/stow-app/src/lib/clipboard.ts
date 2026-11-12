import { writeText } from "@tauri-apps/plugin-clipboard-manager";

/** Copy text via the Tauri clipboard plugin, falling back to the web clipboard. Resolves true on success. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await writeText(text);
    return true;
  } catch (pluginErr) {
    const nav = typeof navigator !== "undefined" ? navigator.clipboard : undefined;
    if (!nav?.writeText) throw pluginErr;
    await nav.writeText(text);
    return true;
  }
}
