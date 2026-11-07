// Shared by tui.mjs and report.mjs, and the AT Store screenshots (packages/atstore).

import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { copyFileSync, cpSync, rmSync } from "node:fs";
import { constants } from "node:os";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

/** The Cargo workspace: the repository root. */
export const REPO = join(import.meta.dirname, "../../..");

/** Builds the CLI, and returns its path. */
export function buildCli() {
  execFileSync("cargo", ["build", "--quiet", "--locked", "--manifest-path", join(REPO, "Cargo.toml"), "-p", "locallibrary-cli"], {
    stdio: "inherit",
  });
  return join(process.env.CARGO_TARGET_DIR ?? join(REPO, "target"), "debug/locallibrary");
}

/**
 * A drive at `dir` copied from the fixtures: 12 tracks and a playlist (bigger-export.pdb), with the
 * demo export's analysis and settings files.
 *
 * @param {string} dir
 */
export function fixtureDrive(dir) {
  const rekordbox = join(dir, "PIONEER/rekordbox");
  cpSync(join(REPO, "crates/locallibrary/tests/fixtures/rekordcrate/demo_tracks/PIONEER"), join(dir, "PIONEER"), { recursive: true });
  copyFileSync(join(REPO, "vendor/rekordbox-pdb/tests/data/bigger-export.pdb"), join(rekordbox, "export.pdb"));
  rmSync(join(rekordbox, "exportExt.pdb"), { force: true });
}

const UNIT_SECS = { s: 1, m: 60, h: 3600, d: 86400 };

/** Seconds in a Bombadil time limit: 90s, 2m, 1h, 1d, or a bare number of seconds. */
function limitSecs(limit) {
  const unit = UNIT_SECS[limit.at(-1)];
  return unit ? Number(limit.slice(0, -1)) * unit : Number(limit);
}

/**
 * Runs Bombadil (from this folder's npm install) with the given arguments, and resolves to its exit
 * code. It has hung waiting on Chrome long past --time-limit, so it gets two minutes' grace and is
 * then stopped (124).
 *
 * It runs in a process group of its own, and stopping it signals the whole group: the node wrapper
 * doesn't pass signals on to the native binary it starts, so signalling the wrapper alone would
 * leave Bombadil, and the TUI or Chrome it drives, running.
 */
export async function runBombadil(args) {
  const grace = limitSecs(process.env.TIME_LIMIT ?? "2m") + 120;
  const child = spawn(join(import.meta.dirname, "node_modules/.bin/bombadil"), args, {
    cwd: import.meta.dirname,
    stdio: ["ignore", "inherit", "inherit"],
    detached: true,
  });
  const exited = once(child, "exit");

  // INT, as Ctrl-C would send, on which Bombadil closes the Chrome or TUI it started (they're in
  // groups of their own), then KILL whatever is still in the group a few seconds later.
  const stop = async () => {
    try {
      process.kill(-child.pid, "SIGINT");
    } catch {
      return;
    }
    await sleep(3000);
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {
      // The group ended on INT.
    }
  };

  // The group isn't the terminal's, so Ctrl-C doesn't reach it: pass it on, and exit once it's
  // stopped rather than when Bombadil does.
  let interrupting;
  const interrupted = () => {
    interrupting ??= stop().then(() => process.exit(130));
  };
  process.once("SIGINT", interrupted).once("SIGTERM", interrupted);
  let timedOut = false;
  const timer = setTimeout(() => {
    console.error(`bombadil still running ${grace}s in; stopping it`);
    timedOut = true;
    stop();
  }, grace * 1000);

  const [code, signal] = await exited;
  await interrupting;
  clearTimeout(timer);
  process.off("SIGINT", interrupted).off("SIGTERM", interrupted);
  return timedOut ? 124 : (code ?? 128 + constants.signals[signal]);
}
