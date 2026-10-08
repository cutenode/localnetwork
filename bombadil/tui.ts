// Bombadil specification for `locallibrary tui`.
//
// Run it through `./tui.sh`, which builds the CLI, copies a fixture drive into a temporary
// folder, hides the machine's real drives (LOCALLIBRARY_VOLUMES), points the store at an empty
// folder and gives the TUI a home and working folder of its own in there, so what random keys
// write stays in that folder.

import { always, eventually, next, now, type Cell } from "@antithesishq/bombadil";
import { CharSet } from "@antithesishq/bombadil/actions";
import { actions, extract, weighted, type State } from "@antithesishq/bombadil/terminal";
import { CharSets, lastAction } from "@antithesishq/bombadil/terminal/defaults/actions";
export { exitSuccess, noReplacementChars } from "@antithesishq/bombadil/terminal/defaults/properties";

const VIEWS = ["Library", "Drives", "Snapshots", "Profile", "Mixes"];

/** What the properties look at, read off the terminal grid once per state. */
type Screen = {
  rows: number;
  columns: number;
  lines: string[];
  header: string;
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
  VIEWS.forEach((view, i) => {
    const at = text.indexOf(` ${i + 1} ${view} `);
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

/** The key help (or the message log, which works the same way) is open. Known by its footer:
 *  the Profile view has a "Keys" pane and row. */
function helpOpen(s: Screen): boolean {
  return has(s, "any other key closes");
}

/** A dialog drawn over the screen: help (or a report), the edit menu, a confirmation, a
 *  question, or a list to choose from (the command palette, a drive's actions, a snapshot, a
 *  tag). */
function overlay(s: Screen): boolean {
  return (
    helpOpen(s) ||
    confirming(s) ||
    s.lines.some((l) => /╭ (Edit · |Sure\? )/.test(l)) ||
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
  return s.tabs === VIEWS.length && !overlay(s) && !typing(s) && !picking(s);
}

/** A confirmation ("Write to DEMO?", "Eject DEMO?"), known by its keys at the bottom: only `y`
 *  does it, and only once it has been up for 400 ms. */
function confirming(s: Screen): boolean {
  return s.lines.some((l) => / y (write|save|eject|sync|pack|undo) {2}n cancel /.test(l));
}

/** Planning or making a change (the header says so until the job is done). */
function editing(s: Screen): boolean {
  return /\b(Planning|Writing|Saving|Ejecting|Syncing|Packing|Undoing) /.test(s.header);
}

function typed(): string | null {
  const a = lastAction.current;
  return a && "TypeText" in a ? a.TypeText : null;
}

// ---------------------------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------------------------

/** The header (row 0) is always drawn, unless a dialog covers it or there's no room. */
export const headerShown = always(() => {
  const s = screen.current;
  return s.rows < 3 || s.columns < 16 || overlay(s) || s.header.includes("◉ locallibrary");
});

/** All five views are listed whenever the terminal is wide enough for them. */
export const allViewsListed = always(() => {
  const s = screen.current;
  return s.columns < 75 || overlay(s) || !s.header.includes("locallibrary") || s.tabs === VIEWS.length;
});

/** Exactly one view is active whenever the tabs are on screen. */
export const oneActiveView = always(() => {
  const s = screen.current;
  return s.tabs < VIEWS.length || s.activeCount === 1;
});

/** The footer always says something: key hints, a toast, a prompt or the search box. */
export const footerNeverBlank = always(() => {
  const s = screen.current;
  return s.rows < 2 || overlay(s) || s.footer.trim() !== "";
});

/** With the machine's drives hidden, the only drive is the fixture copy. */
export const onlyTheFixtureDrive = always(() => {
  const m = /(\d+) drives? \? help/.exec(screen.current.header);
  return !m || Number(m[1]) <= 1;
});

/** Wherever text is being typed, the cursor after it is on screen (if the terminal has room). */
export const typingShowsCursor = always(() => {
  const s = screen.current;
  return s.columns < 60 || s.rows < 12 || overlay(s) || (typingLine(s)?.includes("▏") ?? true);
});

/** Never a Rust panic message on screen (the process may survive one on another thread). */
export const noPanicMessage = always(() => !has(screen.current, "panicked at"));

/** Planning and writing edits always finish. */
export const editsFinish = always(
  now(() => editing(screen.current)).implies(eventually(() => !editing(screen.current)).within(30, "seconds")),
);

/** Pressing 1-5 in the base state switches to that view. */
export const digitsSwitchViews = always(() => {
  if (!base(screen.current)) return true;
  return next(() => {
    const t = typed();
    if (t === null || !/^[1-5]$/.test(t)) return true;
    const s = screen.current;
    return s.tabs < VIEWS.length || s.active === Number(t);
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
  ..."1234567890?!/abcdefghijklmnoprstuvwxyzABCDEGHJKMNPQRSTX*#[]+=-_ ',.<>{}()".split(""),
);

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
