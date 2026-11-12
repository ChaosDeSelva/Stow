import { mount } from "svelte";
// IBM Plex Sans is bundled (no Google Fonts request at runtime); Latin + Latin Extended only.
import "@fontsource/ibm-plex-sans/latin-400.css";
import "@fontsource/ibm-plex-sans/latin-ext-400.css";
import "@fontsource/ibm-plex-sans/latin-500.css";
import "@fontsource/ibm-plex-sans/latin-ext-500.css";
import "@fontsource/ibm-plex-sans/latin-600.css";
import "@fontsource/ibm-plex-sans/latin-ext-600.css";
import "@fontsource/ibm-plex-sans/latin-700.css";
import "@fontsource/ibm-plex-sans/latin-ext-700.css";
import "./app.css";
import { initTheme } from "./lib/theme";
import App from "./App.svelte";

// Light by default (or the last resolved value) before settings load: no flash of dark.
initTheme();
// The capture window is transparent (tauri.conf.json): only CapturePanel's card may paint.
if (window.location.hash === "#/capture") document.documentElement.classList.add("capture-window");

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;
