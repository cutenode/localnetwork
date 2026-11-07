// Bombadil specification for `locallibrary tui`.
//
// Run it through `node tui.mjs`, which builds the CLI, copies a fixture drive into a temporary
// folder, hides the machine's real drives (LOCALLIBRARY_VOLUMES), points the store at an empty
// folder and gives the TUI a home and working folder of its own in there, so what random keys
// write stays in that folder.

import { always, eventually, next, now, type Cell } from "@antithesishq/bombadil";
import { CharSet } from "@antithesishq/bombadil/actions";
import { actions, extract, weighted, type State } from "@antithesishq/bombadil/terminal";
import { CharSets, lastAction } from "@antithesishq/bombadil/terminal/defaults/actions";
export { exitSuccess, noReplacementChars } from "@antithesishq/bombadil/terminal/defaults/properties";

/** The views in tab order, each with the letter that follows `g` to go there. */
const VIEWS = [
  ["Library", "l"],
  ["Prepare", "p"],
  ["Drives", "d"],
  ["Recordings", "r"],
  ["Publish", "u"],
];

/** Below this, there's only the deck and the size it needs. */
const SMALLEST = { columns: 60, rows: 20 };

/** What the properties look at, read off the terminal grid once per state. */
type Screen = {
  rows: number;
  columns: number;
  lines: string[];
  header: string;
  /** What the player's doing, above the footer. */
  status: string;
  footer: string;
  /** Tab labels found in the header (all five, unless the terminal is narrow or covered). */
  tabs: number;
  /** The tab drawn as active (1-5), or 0 when there isn't exactly one. */
  active: number;
  /** Tabs drawn as active: exactly one whenever all five are on screen. */
  activeCount: number;
};

function screenOf(state: State): Screen {
  const { rows, columns } = state.grid.size;
  const lines: string[] = [];
  for (let i = 0; i < rows; i++) lines.push(state.grid.rowText(i));

  // Map the header's text back to cells, so a label's position finds its style.
  const cells = rows > 0 ? state.grid.row(0) : [];
  let text = "";
  const cellAt: number[] = [];
  cells.forEach((cell, i) => {
    for (let k = 0; k < cell.contents.length; k++) cellAt.push(i);
    text += cell.contents;
  });
  let tabs = 0;
  let active = 0;
  let activeCount = 0;
  VIEWS.forEach(([view, letter], i) => {
    const at = text.indexOf(` ${letter} ${view} `);
    if (at < 0) return;
    tabs++;
    // The active tab is bold on a coloured background; the others are dim.
    const style = cells[cellAt[at + 1]].style;
    if ((style.attributes & 0b1) !== 0 && style.backgroundColor !== "None") {
      active = i + 1;
      activeCount++;
    }
  });
  if (activeCount !== 1) active = 0;
  return {
    rows,
    columns,
    lines,
    header: lines[0] ?? "",
    status: lines[rows - 2] ?? "",
    footer: lines[rows - 1] ?? "",
    tabs,
    active,
    activeCount,
  };
}

const screen: Cell<Screen> = extract(screenOf);

function has(s: Screen, needle: string): boolean {
  return s.lines.some((l) => l.includes(needle));
}

/** Only the deck: the terminal is smaller than the views need. */
function deckOnly(s: Screen): boolean {
  return s.columns < SMALLEST.columns || s.rows < SMALLEST.rows;
}

/** `g` was pressed and the next key finishes it. */
function chord(s: Screen): boolean {
  return s.status.includes(" GO TO ");
}

/** The key help (or the message log, which works the same way) is open. Known by its footer:
 *  the Profile view has a "Keys" pane and row. */
function helpOpen(s: Screen): boolean {
  return has(s, "any other key closes");
}

/** A dialog drawn over the screen: help (or a report), the edit menu, a confirmation, a
 *  question, the outputs to play through, or a list to choose from (the command palette, a
 *  drive's actions, a snapshot, a tag). */
function overlay(s: Screen): boolean {
  return (
    helpOpen(s) ||
    confirming(s) ||
    s.lines.some((l) => /╭ (Edit · |Sure\? |Output )/.test(l)) ||
    s.lines.some((l) => l.includes("↑↓ choose · enter · esc close")) ||
    // Squeezed into a small terminal, a dialog may show only its border.
    s.header.trimStart().startsWith("╭")
  );
}

/** The footer is a prompt (a name for a playlist, folder, tag or rule, a path to write to). */
function prompting(s: Screen): boolean {
  return /^ (New playlist|New folder|Rename .* to|Save the crate|New My Tag|Bring back|Smart playlist at|Export to|rekordbox XML)[^:]*: /.test(
    s.footer,
  );
}

/** The line text is being typed on (search, a prompt, a path or a handle), or null: keys go
 *  there. Known by what's typed there, not by the cursor: the Profile view's bars and the Mixes
 *  view's playhead draw the same `▏`. */
function typingLine(s: Screen): string | null {
  if (s.footer.startsWith(" / ") || prompting(s)) return s.footer;
  const path = s.lines.findIndex((l) => l.includes("Open a recording or folder:"));
  if (path >= 0) return s.lines[path + 1] ?? "";
  return s.lines.find((l) => l.includes(" Atmosphere handle or server: ")) ?? null;
}

function typing(s: Screen): boolean {
  return typingLine(s) !== null;
}

/** Picking a playlist to add tracks to: most keys are ignored. */
function picking(s: Screen): boolean {
  return s.footer.startsWith(" ◆ ");
}

/** The base state: global keys (views, help, quit) do what they say. */
function base(s: Screen): boolean {
  return s.tabs === VIEWS.length && !overlay(s) && !typing(s) && !picking(s) && !chord(s);
}

/** The Deck layout, in place of the deck strip. */
function deckLayout(s: Screen): boolean {
  return s.status.includes(" DECK ");
}

/** A confirmation ("Write to DEMO?", "Eject DEMO?"), known by its keys at the bottom: only `y`
 *  does it, and only once it has been up for 400 ms. */
function confirming(s: Screen): boolean {
  return s.lines.some((l) => / y (write|save|eject|sync|pack|undo) {2}n cancel /.test(l));
}

/** Planning or making a change (the status line says so until the job is done). */
function editing(s: Screen): boolean {
  return /\b(Planning|Writing|Saving|Ejecting|Syncing|Packing|Undoing) /.test(s.status);
}

function typed(): string | null {
  const a = lastAction.current;
  return a && "TypeText" in a ? a.TypeText : null;
}

// ---------------------------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------------------------

/** The header (row 0) is always drawn, unless a dialog covers it or there's only the deck. */
export const headerShown = always(() => {
  const s = screen.current;
  return deckOnly(s) || overlay(s) || s.header.includes("◉");
});

/** All five views are listed whenever there's more than the deck. */
export const allViewsListed = always(() => {
  const s = screen.current;
  return deckOnly(s) || overlay(s) || !s.header.includes("◉") || s.tabs === VIEWS.length;
});

/** The deck strip is on screen in every view, whatever's open over the view: it says what's
 *  playing, or that nothing is. */
export const deckStripShown = always(() => {
  const s = screen.current;
  return s.rows < 3 || s.columns < 20 || deckLayout(s) || /Noth|▶|‖/.test(s.lines.join("\n"));
});

/** With only the deck, the screen names the size it needs, unless a dialog has what's under it. */
export const tooSmallNamesTheSize = always(() => {
  const s = screen.current;
  const boxed = s.lines.some((l) => /[╭┏]/.test(l));
  return !deckOnly(s) || boxed || s.rows < 2 || s.columns < 5 || has(s, `${SMALLEST.columns}x${SMALLEST.rows}`);
});

/** Exactly one view is active whenever the tabs are on screen. */
export const oneActiveView = always(() => {
  const s = screen.current;
  return s.tabs < VIEWS.length || s.activeCount === 1;
});

/** The footer always says something: key hints, a toast, a prompt or the search box. Each starts
 *  with a space, so a terminal one column wide shows none of it. */
export const footerNeverBlank = always(() => {
  const s = screen.current;
  return deckOnly(s) || overlay(s) || s.footer.trim() !== "";
});

/** Wherever text is being typed, the cursor after it is on screen (if the terminal has room). */
export const typingShowsCursor = always(() => {
  const s = screen.current;
  return deckOnly(s) || overlay(s) || (typingLine(s)?.includes("▏") ?? true);
});

/** Never a Rust panic message on screen (the process may survive one on another thread). */
export const noPanicMessage = always(() => !has(screen.current, "panicked at"));

/** Planning and writing edits always finish. */
export const editsFinish = always(
  now(() => editing(screen.current)).implies(eventually(() => !editing(screen.current)).within(30, "seconds")),
);

/** `g` and a view's letter in the base state switches to that view. */
export const gotoSwitchesViews = always(() => {
  if (!base(screen.current)) return true;
  return next(() => {
    const view = VIEWS.findIndex(([, letter]) => typed() === `g${letter}`);
    const s = screen.current;
    return view < 0 || s.tabs < VIEWS.length || s.active === view + 1;
  });
});

/** `?` in the base state opens the key help. */
export const questionMarkOpensHelp = always(() => {
  if (!base(screen.current)) return true;
  return next(() => typed() !== "?" || helpOpen(screen.current));
});

/** With the help open, a key other than the scrolling ones closes it. */
export const anyOtherKeyClosesHelp = always(() => {
  const s = screen.current;
  if (!helpOpen(s)) return true;
  return next(() => {
    const t = typed();
    if (t === null || t.length !== 1 || "jk ".includes(t) || t === "\x1b") return true;
    return !helpOpen(screen.current);
  });
});

/** The Deck layout shows the tempo, framed or in large digits on their own, at every size that has it. */
export const theDeckShowsItsTempo = always(
  () => !deckLayout(screen.current) || has(screen.current, "BPM") || has(screen.current, "▀"),
);

/** On the deck, `b` opens Browse over it, as the status line says. */
export const browseOpensOverTheDeck = always(() => {
  if (!deckLayout(screen.current) || !base(screen.current)) return true;
  return next(() => typed() !== "b" || !deckLayout(screen.current));
});

/** Esc in the search box closes it. */
export const escapeLeavesSearch = always(() => {
  if (!screen.current.footer.startsWith(" / ")) return true;
  return next(() => typed() !== "\x1b" || !screen.current.footer.startsWith(" / "));
});

// ---------------------------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------------------------

// Every key the TUI binds, except the ones that leave the run or the machine: `q`, Ctrl-C and
// Ctrl-Q quit (which ends the test), `L` signs in and out over the network, `:` opens the command
// palette, which reaches signing in too, and `O` opens any folder typed in the Mixes view. `y` and
// `u` stay in: they write edits (and undo them), and the only drive the TUI can see is a
// throwaway copy (which it won't eject: it's no volume). Text (`TEXT`) is typed only where a line
// is being typed, so `q`, `L` and `:` there are just letters.
const KEYS = CharSet.fromLiterals(
  ..."1234567890?!/abcdefghijklmnoprstuvwxyzABCDEGHJKMNPQRSTUX*#[]+=-_ ',.<>{}()".split(""),
);

/** `g` and each view's letter, typed at once. */
const GOTO = CharSet.fromLiterals(...VIEWS.map(([, letter]) => `g${letter}`));

/** Browse, the waveform's colours, the layouts, zooming and visualisers. */
const DECK_KEYS = CharSet.fromLiterals(..."bwf[]v".split(""));

const NAVIGATION = CharSet.union(
  CharSets.CONTROL_ARROWS,
  CharSets.CONTROL_NAVIGATION,
  CharSet.fromLiterals(
    "\r", // Enter
    "\t", // Tab
    "\x1b", // Esc
    "\x7f", // Backspace
    "\x15", // Ctrl-U: clear the line
    "\x17", // Ctrl-W: delete a word
    "\x1b[1;2A", // Shift-Up: move in the crate
    "\x1b[1;2B", // Shift-Down
    "\x1b[1;2C", // Shift-Right: seek 30 s
    "\x1b[1;2D", // Shift-Left
    "\x1b[15~", // F5: refresh
  ),
);

/** Names for playlists, folders and searches: plain and not so plain. */
const TEXT = CharSet.union(
  CharSets.ASCII_PRINTABLE,
  CharSets.UNICODE_LATIN_EXTENDED,
  CharSets.UNICODE_CJK,
  CharSets.UNICODE_EMOTICONS,
);

const size: Cell<{ rows: number; columns: number }> = extract((state) => state.grid.size);

export const keys = weighted([
  // Asked to write: look over the changes for a moment (only scrolling keys), then say yes.
  [
    200,
    actions(() =>
      confirming(screen.current)
        ? [
            { TypeText: { CharSet: CharSet.fromLiterals("j", "k", "\x1b[A", "\x1b[B") } },
            { TypeText: { CharSet: CharSet.fromLiterals("y") } },
          ]
        : [],
    ),
  ],
  [50, actions(() => [{ TypeText: { CharSet: KEYS } }])],
  [10, actions(() => [{ TypeText: { CharSet: GOTO } }])],
  // The deck's own keys, once it's showing.
  [10, actions(() => [{ TypeText: { CharSet: deckLayout(screen.current) ? DECK_KEYS : KEYS } }])],
  [30, actions(() => [{ TypeText: { CharSet: NAVIGATION } }])],
  // Typing goes into the search box and name prompts more than anywhere else.
  [10, actions(() => [{ TypeText: { CharSet: typing(screen.current) ? TEXT : KEYS } }])],
  [
    4,
    actions(() => {
      const s = size.current;
      return s ? [{ Click: { row: [0, s.rows - 1], column: [0, s.columns - 1] } }] : [];
    }),
  ],
  [2, actions(() => [{ ScrollUp: {} }, { ScrollDown: {} }])],
  // Mostly sizes a person would use, sometimes far too small.
  [2, actions(() => [{ Resize: { rows: [16, 60], columns: [60, 200] } }])],
  [1, actions(() => [{ Resize: { rows: [1, 15], columns: [1, 59] } }])],
]);
