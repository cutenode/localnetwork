// Explore `locallibrary tui` with Bombadil (tui.ts) on a throwaway copy of a fixture drive.
//
//   node tui.mjs                 # 2 minutes
//   TIME_LIMIT=30m node tui.mjs  # longer; any of Bombadil's options can follow, e.g. --columns 80
//
// The machine's own drives are hidden (LOCALLIBRARY_VOLUMES), the store is an empty folder,
// `open`/`xdg-open` are stubbed out and the TUI's home and working folder are in the run's own
// folder, so random keys can't reach a real drive, a real store or a browser, and anything they
// write (an export to `~/Music`, say) stays in that folder. A path typed in the Recordings view (with
// no recordings listed, enter asks for one) can still name a folder elsewhere to read recordings
// from. Afterwards the drive copy has to still read: every edit the run made is checked.

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { buildCli, fixtureDrive, runBombadil } from "./lib.mjs";

const bin = buildCli();
const work = mkdtempSync(join(tmpdir(), "locallibrary-bombadil."));
const out = process.env.OUTPUT_PATH ?? join(work, "out");
const drive = join(work, "volumes/BOMBADIL");

fixtureDrive(drive);
for (const dir of ["store", "bin", "home"]) mkdirSync(join(work, dir));

const opened = join(work, "opened.log");
for (const cmd of ["open", "xdg-open"]) {
  writeFileSync(join(work, "bin", cmd), `#!/bin/sh\necho "${cmd} $*" >> "${opened}"\n`, { mode: 0o755 });
}

Object.assign(process.env, {
  LOCALLIBRARY_VOLUMES: join(work, "volumes"),
  LOCALLIBRARY_STORE: join(work, "store"),
  PATH: `${join(work, "bin")}${delimiter}${process.env.PATH}`,
  COLORTERM: "truecolor",
});

// The run must see the copy and nothing else.
const seen = execFileSync(bin, ["--json", "drives"], { encoding: "utf8" }).match(/"root"/g)?.length ?? 0;
if (seen !== 1) {
  console.error(`expected to see only ${drive}, saw ${seen} drives`);
  process.exit(1);
}

let status = await runBombadil([
  "terminal",
  "test",
  "--specification",
  join(import.meta.dirname, "tui.ts"),
  "--time-limit",
  process.env.TIME_LIMIT ?? "2m",
  "--quiescence-timeout-ms",
  "50",
  "--exit-on-violation",
  "--output-path",
  out,
  "--output-path-overwrite",
  ...process.argv.slice(2),
  "/bin/sh",
  "-c",
  'cd "$1" && HOME="$1/home" exec "$2" tui "$3"',
  "sh",
  work,
  bin,
  drive,
]);

// Whatever the run wrote to the drive must still read. `doctor` reads it and exits 4 for the
// problems it finds, which it always does here: the copy has no audio files.
for (const command of ["info", "tracks", "playlists", "doctor"]) {
  const { status: code, stderr } = spawnSync(bin, ["--json", command, drive], { encoding: "utf8", stdio: ["ignore", "ignore", "pipe"] });
  if (code !== 0 && !(command === "doctor" && code === 4)) {
    console.error(`after the run, \`locallibrary ${command} ${drive}\` failed:\n${stderr}`);
    status = 1;
  }
}
const edits = join(drive, ".locallibrary/edits.json");
if (existsSync(edits)) {
  console.log(`edits journal: ${readFileSync(edits, "utf8").match(/"op"/g)?.length ?? 0} edit(s)`);
}
if (existsSync(opened)) console.log(`would have opened:\n${readFileSync(opened, "utf8")}`);
console.log(`trace and drive copy: ${work}`);
process.exit(status);
