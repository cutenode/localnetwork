// Bombadil specification for the mix report `locallibrary mix --html` writes.
//
// Run it through `./report.sh`, which builds a drive with synthetic audio, records a "set"
// from it, writes the report and serves it with the set's audio.

import { always, eventually, now, type Cell } from "@antithesishq/bombadil";
import { actions, extract, getFingerprint, weighted } from "@antithesishq/bombadil/browser";
import { scroll, waitOnce } from "@antithesishq/bombadil/browser/defaults/actions";
export {
  noConsoleErrors,
  noHttpErrorCodes,
  noUncaughtExceptions,
  noUnhandledPromiseRejections,
} from "@antithesishq/bombadil/browser/defaults/properties";

/** What the properties look at, read off the page once per state. */
type Page = {
  text: string;
  tiles: Record<string, string>;
  tracks: number;
  selected: number;
  playerShown: boolean;
  paused: boolean | null;
  playLabel: string | null;
  clock: string | null;
  theme: string | null;
  themeAttribute: string | null;
  zoomPressed: number;
  /** Where the playhead is, as a share of the timeline. */
  playhead: number | null;
  source: string;
};

const page: Cell<Page> = extract((state) => {
  const d = state.document;
  const tiles: Record<string, string> = {};
  for (const t of d.querySelectorAll(".tile")) {
    tiles[t.querySelector(".label")?.textContent ?? ""] = t.querySelector(".value")?.textContent ?? "";
  }
  const audio = d.getElementById("audio") as HTMLAudioElement | null;
  const player = d.getElementById("player");
  const playhead = d.getElementById("playhead");
  return {
    // textContent, not innerText: laying the page out while Bombadil has paused it during
    // playback stalls Chrome. The scripts aren't in these.
    text: [".wrap", "#player"].map((s) => d.querySelector(s)?.textContent ?? "").join("\n"),
    tiles,
    tracks: d.querySelectorAll("#tracks .track").length,
    selected: d.querySelectorAll("#tracks .track.on").length,
    playerShown: !!player && !player.hidden,
    paused: audio ? audio.paused : null,
    playLabel: d.getElementById("pp")?.getAttribute("aria-label") ?? null,
    clock: d.getElementById("clock")?.textContent ?? null,
    theme: d.querySelector(".theme-toggle")?.textContent ?? null,
    themeAttribute: d.documentElement.getAttribute("data-theme"),
    zoomPressed: d.querySelectorAll('.zoom button[aria-pressed="true"]').length,
    playhead:
      playhead?.style.display === "block" && playhead.style.left.endsWith("%")
        ? parseFloat(playhead.style.left) / 100
        : null,
    source: d.getElementById("decksrc")?.textContent ?? "",
  };
});

const seconds = (clock: string) => clock.split(":").reduce((s, part) => s * 60 + Number(part), 0);

// ---------------------------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------------------------

/** The set's audio loads, so the player appears. */
export const playerAppears = eventually(() => page.current.playerShown).within(15, "seconds");

/** The audio is where the report expects it. */
export const audioFound = always(
  () =>
    !page.current.source.includes("isn't where this report expects it") &&
    !page.current.source.startsWith("To listen along"),
);

/** No number or label is ever drawn from a missing value. */
export const noMissingValuesShown = always(() => !/\b(NaN|undefined|Infinity)\b/.test(page.current.text));

/** The "Tracks identified" tile counts the tracklist. */
export const tileCountsTheTracklist = always(() => {
  const tile = page.current.tiles["Tracks identified"];
  return tile === undefined || Number(tile) === page.current.tracks;
});

/** Coverage is a share of the mix: 0-100%. */
export const coverageIsAShare = always(() => {
  const tile = page.current.tiles["Of the mix identified"];
  if (tile === undefined) return true;
  const n = parseFloat(tile);
  return n >= 0 && n <= 100;
});

/** At most one track is selected. */
export const oneSelectionAtMost = always(() => page.current.selected <= 1);

/** The clock never runs past the end of the mix. */
export const clockWithinTheMix = always(() => {
  const clock = page.current.clock;
  if (!page.current.playerShown || !clock || !clock.includes(" / ")) return true;
  const [at, of] = clock.split(" / ").map(seconds);
  return at <= of;
});

/** The playhead stays on the timeline. */
export const playheadOnTheTimeline = always(() => {
  const p = page.current.playhead;
  return p === null || (p >= 0 && p <= 1);
});

/** The play button says what it will do, soon after playback starts or stops (it changes on the
 * audio's play and pause events, which come a moment after the click). */
export const playButtonFollowsPlayback = always(
  now(() => {
    const p = page.current;
    return p.playerShown && p.paused !== null && p.playLabel !== (p.paused ? "Play" : "Pause");
  }).implies(
    eventually(() => {
      const p = page.current;
      return p.playLabel === (p.paused ? "Play" : "Pause");
    }).within(10, "seconds"),
  ),
);

/** The theme button names the theme the page is drawn in. */
export const themeLabelMatchesThePage = always(() => {
  const p = page.current;
  if (!p.theme) return true;
  const named = p.theme.replace("Theme: ", "");
  return named === "auto" ? p.themeAttribute === null : p.themeAttribute === named;
});

/** Exactly one zoom level is pressed. */
export const oneZoomLevel = always(() => page.current.zoomPressed === 1);

// ---------------------------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------------------------

// What a listener clicks: tracks, the timeline's blocks and waveform, zoom, theme and play.
// Not "Download the data", which would save a file.
const targets = extract((state) => {
  const out = [];
  const selector = "#tracks .track, .block, #wave, .zoom button, .theme-toggle, #pp";
  const { innerWidth, innerHeight } = state.window;
  for (const element of state.document.querySelectorAll(selector)) {
    const r = element.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) continue;
    // Somewhere inside the element that's on screen.
    const x = Math.min(Math.max(r.left + r.width / 2, 0), innerWidth - 1);
    const y = Math.min(Math.max(r.top + r.height / 2, 0), innerHeight - 1);
    if (x < r.left || x > r.right || y < r.top || y > r.bottom) continue;
    out.push({ fingerprint: getFingerprint(element), point: { x, y } });
  }
  return out;
});

export const listen = weighted([
  [60, actions(() => (targets.current ?? []).map((t) => ({ Click: t })))],
  // Space plays and pauses; the arrows skip 10 s.
  [20, actions(() => [32].map((code) => ({ PressKey: { code } })))],
  [15, scroll],
  [5, actions(() => [{ SetViewport: { width: [360, 1600], height: [500, 1000] } }])],
  [2, waitOnce],
]);
