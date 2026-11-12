// Renders THIRD_PARTY_LICENSES.md from (a) cargo-about's JSON, (b) the production npm packages
// under apps/stow-app/node_modules, and (c) a few hand-listed components whose license text lives
// outside both package managers (whisper.cpp is vendored inside whisper-rs-sys).
//
// Invoked by scripts/collect-licenses.sh; do not run directly. Output must be deterministic
// (sorted, no timestamps) because CI diffs the committed file against a fresh run.
//
// usage: node render.mjs <about.json> <repo-root> <app-dir> <out.md>

import { readFileSync, writeFileSync, existsSync, readdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, dirname } from "node:path";

const [aboutJsonPath, repoRoot, appDir, outPath] = process.argv.slice(2);
if (!outPath) {
  console.error("usage: node render.mjs <about.json> <repo-root> <app-dir> <out.md>");
  process.exit(2);
}

const byName = (a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version);
const fence = (text) => {
  // A license text containing ``` would break the fence; use a longer one.
  const longest = Math.max(3, ...[...text.matchAll(/`+/g)].map((m) => m[0].length + 1));
  const f = "`".repeat(longest);
  return `${f}text\n${text.replace(/\s+$/, "")}\n${f}\n`;
};
const readFirst = (dir, candidates) => {
  for (const c of candidates) {
    const p = join(dir, c);
    if (existsSync(p)) return readFileSync(p, "utf8");
  }
  // Case-insensitive fallback: LICENSE, LICENSE.md, LICENCE, COPYING, ...
  const hit = readdirSync(dir).find((f) => /^(licen[cs]e|copying)(\..+)?$/i.test(f));
  return hit ? readFileSync(join(dir, hit), "utf8") : null;
};

// ---------------------------------------------------------------------------------------------
// Rust crates (cargo-about)
// ---------------------------------------------------------------------------------------------
const about = JSON.parse(readFileSync(aboutJsonPath, "utf8"));
const crates = new Map(); // "name@version" -> { name, version, license, repository, ids:Set }
for (const lic of about.licenses) {
  for (const u of lic.used_by) {
    const key = `${u.crate.name}@${u.crate.version}`;
    const c = crates.get(key) ?? {
      name: u.crate.name,
      version: u.crate.version,
      license: u.crate.license ?? "(see license file)",
      repository: u.crate.repository ?? "",
      ids: new Set(),
    };
    c.ids.add(lic.id);
    crates.set(key, c);
  }
}
const crateList = [...crates.values()].sort(byName);

// Group license texts: cargo-about already dedupes identical texts; sort for determinism.
const licenseTexts = about.licenses
  .map((l) => ({
    id: l.id,
    name: l.name,
    text: l.text,
    usedBy: [...new Set(l.used_by.map((u) => `${u.crate.name} ${u.crate.version}`))].sort(),
  }))
  .sort((a, b) => a.id.localeCompare(b.id) || a.usedBy.join().localeCompare(b.usedBy.join()));

// ---------------------------------------------------------------------------------------------
// npm packages shipped in the frontend bundle
// ---------------------------------------------------------------------------------------------
// `npm ls --omit=dev` gives the runtime dependency closure. Svelte is a devDependency because the
// compiler runs at build time, but its runtime is compiled into the bundle, so it is added here.
const BUNDLED_DEV_DEPS = ["svelte"];
const npmTree = JSON.parse(
  execFileSync("npm", ["ls", "--omit=dev", "--all", "--json", "--long"], {
    cwd: appDir,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
  }),
);
const npmPkgs = new Map(); // "name@version" -> { name, version, license, path }
const walk = (deps) => {
  for (const [name, d] of Object.entries(deps ?? {})) {
    if (!d.version) continue;
    npmPkgs.set(`${name}@${d.version}`, { name, version: d.version, license: d.license ?? "", path: d.path });
    walk(d.dependencies);
  }
};
walk(npmTree.dependencies);
for (const name of BUNDLED_DEV_DEPS) {
  const dir = join(appDir, "node_modules", name);
  const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
  npmPkgs.set(`${name}@${pkg.version}`, { name, version: pkg.version, license: pkg.license ?? "", path: dir });
}
const npmList = [...npmPkgs.values()]
  .map((p) => {
    const pkg = JSON.parse(readFileSync(join(p.path, "package.json"), "utf8"));
    const license = p.license || pkg.license || "";
    const text = readFirst(p.path, ["LICENSE", "LICENSE.txt", "LICENSE.md", "LICENSE-MIT", "LICENSE-APACHE-2.0"]);
    return { ...p, license, text, repository: pkg.repository?.url ?? pkg.repository ?? "" };
  })
  .sort(byName);

// ---------------------------------------------------------------------------------------------
// Components outside both package managers
// ---------------------------------------------------------------------------------------------
const whisperSys = crateList.find((c) => c.name === "whisper-rs-sys");
let whisperCppLicense = null;
if (whisperSys) {
  const meta = JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1", "--locked", "--offline"], {
      cwd: repoRoot,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      maxBuffer: 64 * 1024 * 1024,
    }),
  );
  const pkg = meta.packages.find((p) => p.name === "whisper-rs-sys" && p.version === whisperSys.version);
  if (pkg) whisperCppLicense = readFirst(join(dirname(pkg.manifest_path), "whisper.cpp"), ["LICENSE"]);
}
if (!whisperCppLicense) {
  console.error("render.mjs: could not find the vendored whisper.cpp LICENSE inside whisper-rs-sys");
  process.exit(1);
}

// ---------------------------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------------------------
const out = [];
const h = (s) => out.push(s, "");
const row = (cells) => out.push(`| ${cells.map((c) => String(c).replace(/\|/g, "\\|")).join(" | ")} |`);

h("# Third-party licenses");
h(
  "Generated by `scripts/collect-licenses.sh` (`just licenses`) from `Cargo.lock` via cargo-about and from " +
    "`apps/stow-app/node_modules` via `npm ls --omit=dev`. Do not edit by hand; CI fails if this file is stale. " +
    "The short, human-readable summary and the rules for how these notices are delivered live in " +
    "[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).",
);
h("Contents: [Components with their own notices](#components-with-their-own-notices) · " +
    "[npm packages in the app bundle](#npm-packages-in-the-app-bundle) · " +
    "[Rust crates](#rust-crates) · [Rust crate license texts](#rust-crate-license-texts)");

h("## Components with their own notices");
h(`### whisper.cpp / ggml (vendored in whisper-rs-sys ${whisperSys.version})`);
h(
  "MIT. Statically linked into `Stow.app` and `stw` through `whisper-rs-sys`, which vendors the whisper.cpp " +
    "and ggml sources. The Rust bindings (`whisper-rs`, `whisper-rs-sys`) are Unlicense and are listed under Rust crates.",
);
out.push(fence(whisperCppLicense));
h("### SQLite");
h(
  "Public domain. Bundled through `rusqlite` / `libsqlite3-sys` (`bundled` feature); see https://sqlite.org/copyright.html.",
);
h("### Whisper model weights (downloaded at runtime, not redistributed)");
h(
  "`ggml-*.en.bin` files are fetched from `huggingface.co/ggerganov/whisper.cpp` on first transcription and " +
    "verified by SHA-256. They are MIT (© OpenAI, converted to ggml format by the whisper.cpp project) and are " +
    "not part of any Stow download.",
);

h("## npm packages in the app bundle");
h(
  "Production dependencies of `apps/stow-app` (plus the Svelte runtime, which Vite compiles into the bundle). " +
    "Font Awesome Free ships **icons under CC BY 4.0**; this file is the attribution the license requires " +
    "(the SVG icon packages carry no CSS header). IBM Plex Sans is under the SIL Open Font License 1.1 with " +
    "Reserved Font Name \"Plex\".",
);
row(["Package", "Version", "License", "Repository"]);
row(["---", "---", "---", "---"]);
for (const p of npmList) row([`\`${p.name}\``, p.version, p.license || "(see text)", p.repository]);
out.push("");
for (const p of npmList) {
  h(`### ${p.name} ${p.version}`);
  if (p.text) out.push(fence(p.text));
  else h(`_No license file in the package; declared license: ${p.license || "unknown"}._`);
}

h("## Rust crates");
h(
  `${crateList.length} crates from \`Cargo.lock\` reachable from the shipped binaries (\`Stow.app\`, \`stw\`, ` +
    "`stow-mcp`) on macOS targets. Build-script and dev dependencies are excluded. The License column is the " +
    "crate's own SPDX expression; the texts below are the license files found in each crate, deduplicated.",
);
row(["Crate", "Version", "License", "Repository"]);
row(["---", "---", "---", "---"]);
for (const c of crateList) row([`\`${c.name}\``, c.version, c.license, c.repository]);
out.push("");

h("## Rust crate license texts");
for (const l of licenseTexts) {
  h(`### ${l.id} — ${l.name}`);
  h(`Used by: ${l.usedBy.map((s) => `\`${s}\``).join(", ")}`);
  out.push(fence(l.text));
}

writeFileSync(outPath, out.join("\n").replace(/\n{3,}/g, "\n\n") + "\n");
console.error(
  `render.mjs: ${crateList.length} crates, ${licenseTexts.length} crate license texts, ${npmList.length} npm packages -> ${outPath}`,
);
