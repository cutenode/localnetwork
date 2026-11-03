// Explore the interactive mix report with Bombadil (report.ts) in headless Chrome.
//
//   node report.mjs                 # 2 minutes
//   TIME_LIMIT=10m node report.mjs  # longer; any of Bombadil's options can follow
//
// Builds a copy of a fixture drive whose tracks are synthetic audio, "records" a set from four
// of them (make-mix.mjs), has `locallibrary mix` identify it and write the report next to the
// set's audio (a page `--html` writes links it by its file name), then serves them. Needs Chrome
// or Chromium; set CHROME to pick one.

import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { accessSync, constants, copyFileSync, globSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { createInterface } from "node:readline";
import { buildCli, REPO, runBombadil } from "./lib.mjs";

const bin = buildCli();
const work = mkdtempSync(join(tmpdir(), "locallibrary-report."));
const out = process.env.OUTPUT_PATH ?? join(work, "out");
const drive = join(work, "MIXDRIVE");
const site = join(work, "site");
mkdirSync(join(drive, "PIONEER/rekordbox"), { recursive: true });
copyFileSync(join(REPO, "vendor/rekordbox-pdb/tests/data/bigger-export.pdb"), join(drive, "PIONEER/rekordbox/export.pdb"));

Object.assign(process.env, { LOCALLIBRARY_VOLUMES: join(work, "no-volumes"), LOCALLIBRARY_STORE: join(work, "store") });
execFileSync(process.execPath, [join(import.meta.dirname, "make-mix.mjs"), drive, join(site, "set.wav")], {
  input: execFileSync(bin, ["--json", "tracks", drive]),
  stdio: ["pipe", "inherit", "inherit"],
});
execFileSync(bin, ["--quiet", "mix", "--drive", drive, "--html", join(site, "set.html"), join(site, "set.wav")], { stdio: "inherit" });

const isExecutable = (path) => {
  try {
    accessSync(path, constants.X_OK);
    return true;
  } catch {
    return false;
  }
};
// The newest of the executables a pattern matches (they sort oldest first).
const newest = (patterns) => globSync(patterns).sort().filter(isExecutable).at(-1);
const caches = [join(homedir(), "Library/Caches/ms-playwright"), join(homedir(), ".cache/ms-playwright")];

// Chrome: $CHROME, else Playwright's headless shell (full Chrome has stalled Bombadil's state
// capture while the report plays audio), else an installed Chrome, else Playwright's Chromium.
const chrome =
  process.env.CHROME ||
  newest(caches.map((dir) => `${dir}/chromium_headless_shell-*/chrome-headless-shell-*/chrome-headless-shell`)) ||
  [
    ...["google-chrome-stable", "google-chrome", "chromium", "chromium-browser"].flatMap((name) =>
      (process.env.PATH ?? "").split(delimiter).map((dir) => join(dir, name)),
    ),
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  ].find(isExecutable) ||
  newest([`${caches[0]}/chromium-*/chrome-mac*/*.app/Contents/MacOS/*`, `${caches[1]}/chromium-*/chrome-linux*/chrome`]);
if (!chrome) {
  console.error("no Chrome or Chromium found; set CHROME (or: npx playwright install --only-shell chromium)");
  process.exit(1);
}
// Launched muted: the report plays the set when a track is clicked.
process.env.CHROME = join(work, "chrome");
writeFileSync(process.env.CHROME, `#!/bin/sh\nexec "${chrome}" --mute-audio "$@"\n`, { mode: 0o755 });

const server = spawn(process.execPath, [join(import.meta.dirname, "serve.mjs"), site], { stdio: ["ignore", "pipe", "inherit"] });
process.on("exit", () => server.kill());
const [url] = await once(createInterface({ input: server.stdout }), "line");

const status = await runBombadil([
  "browser",
  "test",
  "--headless",
  "--time-limit",
  process.env.TIME_LIMIT ?? "2m",
  "--exit-on-violation",
  "--output-path",
  out,
  "--output-path-overwrite",
  ...process.argv.slice(2),
  `${url}/set.html`,
  join(import.meta.dirname, "report.ts"),
]);
console.log(`trace: ${out} (bombadil browser inspect ${out})`);
process.exit(status);
