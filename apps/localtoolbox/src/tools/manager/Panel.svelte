<!--
  Manager: a list of tracks, and where each of them can be downloaded from.

  The list is the tool's own and is kept for good (./listed.svelte.ts).
  Nothing goes onto it by itself: the bar at the top says what the tab in
  front is offering and what the session queue holds, and **add** is what
  brings either in. That is the difference from what this panel used to be —
  it *was* whichever page was in front, read and looked up in full the moment
  it was opened, thrown away the moment the popup closed, and blank on any tab
  that was not a SoundCloud playlist. Which is to say: it had nothing to show
  on the Hypeddit tab a row had just sent you to, and opening it was a few
  hundred requests to SoundCloud about tracks nobody had asked about.

  Now it opens on any page, with whatever is on the list, and asking SoundCloud
  anything is something somebody pressed a button for. Adding a page puts its
  tracks on the list straight away, from what the page itself renders, and then
  asks SoundCloud where each one can be had — the rows are up before the first
  answer, and settle where they already are as the answers land.

  The list is then grouped by the site its links point at, or shown in the
  order it was built. Opening a link puts the track in the session queue: at
  that moment nothing has been downloaded yet — the gate behind the link has
  not even been walked — so the queue is where it waits until a whole batch of
  them is marked. The list button queues a track without opening anything; the
  check mark marks one by hand. Each works on the one row it is on.

  A batch is a separate gesture: **Select** in the header puts a tick box on
  every row, and the bar above the list then puts a marker on everything ticked
  — into the queue or out of it, marked as downloaded or unmarked — or takes
  the lot off the list. So what is about to be marked can be looked at first,
  and one selection can take two markers.

  SoundCloud's own downloads have no URL of their own ("Download file" is an
  item in the track's More menu), so those entries open the track page, where
  the content script sees that button being used and marks the track itself.
  A track offering both kinds of link leads with one of them and shows the
  other as a small icon beside it, so a row is a title and one thing to click
  rather than two buttons and no room; links.ts decides which leads. Both are
  the same download icon on every row rather than the uploader's own wording
  for their link ("Free Download", "Buy", "🎧 Stream / Download"): that is a
  label on somebody else's page, it is a different length on every row, and
  the room it took is the room the title wanted. It is still said — in the
  button's tooltip, and in the row's detail line, which names the site.

  The chevron opens a row downwards into the rest of what is known about the
  track: its title, artist and genre, its address and the page it came in
  from. Each is a button that copies that one field, because these are the
  words somebody is about to type into a spreadsheet, a message or a search
  box. Fields nobody has an answer for say so and cannot be copied. Whether it
  has been liked or reposted is down there too, as two small icons rather than
  two lines of text — it is a yes or a no, and nobody pastes it anywhere. The
  row's way off the list is on that same last line, since the four buttons on
  the row itself are as many as 400px will take.

  The footer points at settings, where all of that goes out to a JSON file and
  reads back in ($lib/components/StateTransfer.svelte) — so a reinstalled
  extension is one import away from where it left off, half-worked-through
  batch included. It used to be here, which put a backup of the marks, the
  looked-up details, the session queue and three panels' options behind one
  tool's footer; the file's shape is specified in
  docs/manager-state.schema.json, and state-transfer.ts reads and writes it.
-->
<script lang="ts">
  import { untrack } from 'svelte';
  import Check from '@lucide/svelte/icons/check';
  import Archive from '@lucide/svelte/icons/archive';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import Download from '@lucide/svelte/icons/download';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import Eraser from '@lucide/svelte/icons/eraser';
  import FileMusic from '@lucide/svelte/icons/file-music';
  import Heart from '@lucide/svelte/icons/heart';
  import Repeat2 from '@lucide/svelte/icons/repeat-2';
  import SquareCheckBig from '@lucide/svelte/icons/square-check-big';
  import ListPlus from '@lucide/svelte/icons/list-plus';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import ListX from '@lucide/svelte/icons/list-x';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import X from '@lucide/svelte/icons/x';
  import PanelShell from '$lib/components/PanelShell.svelte';
  import StatusNote from '$lib/components/StatusNote.svelte';
  import FilterBar from '$lib/components/FilterBar.svelte';
  import TrackRow from '$lib/components/TrackRow.svelte';
  import AddBar from './AddBar.svelte';
  import { Button } from '$lib/components/ui/button/index.ts';
  import { Checkbox } from '$lib/components/ui/checkbox/index.ts';
  import { Label } from '$lib/components/ui/label/index.ts';
  import { Progress } from '$lib/components/ui/progress/index.ts';
  import { Switch } from '$lib/components/ui/switch/index.ts';
  import * as Select from '$lib/components/ui/select/index.ts';
  import * as Tooltip from '$lib/components/ui/tooltip/index.ts';
  import { openTab } from '$lib/chrome.ts';
  import { artworkOf, fullSizeArtwork } from '$lib/artwork.ts';
  import { copyImage, copyText } from '$lib/clipboard.ts';
  import { downloaded } from '$lib/downloaded.svelte.ts';
  import { queue } from '$lib/queue.svelte.ts';
  import { createSelection } from '$lib/selection.svelte.ts';
  import { createFilter } from '$lib/list-filter.svelte.ts';
  import { choicePref, flagPref } from '$lib/prefs.svelte.ts';
  import { remembered, view, type Remembered } from '$lib/view.svelte.ts';
  import { count, OPEN_A_PAGE, readPage } from '$lib/soundcloud.ts';
  import { queryOf, searchable, searchUrl, siteNames } from '$lib/search-sites.ts';
  import { searchSites } from '$lib/search-sites.svelte.ts';
  import { titleCasing } from '$lib/title-case.svelte.ts';
  import { toasts } from '$lib/toast.svelte.ts';
  import { betterDetails, listed, MAX } from './listed.svelte.ts';
  import {
    buildGroups,
    buildListOrder,
    downloadLabel,
    entriesFor,
    isWithoutLinks,
    lookupNotes,
    LOOKUP_ERRORS,
    rowFacts,
    rowMarks,
    searchText,
    SOUNDCLOUD_HINT,
    splitEntries,
    tracksWithoutLinks,
    uncheckedTracks,
  } from './links.ts';
  import type { LinkEntry } from './links.ts';
  import type {
    DetailsProgress,
    KnownDetails,
    ListedEntry,
    PageProgress,
    PageTrack,
    PageType,
    PanelProps,
  } from '$lib/types';
  import type { LucideIcon } from '@lucide/svelte';

  /** One row of the list: a track, and the links its buttons open. */
  interface Row {
    key: string;
    track: ListedEntry;
    /** Best first. */
    entries: LinkEntry[];
    /** The one its download button opens; null for none. */
    primary: LinkEntry | null;
    secondary: LinkEntry[];
    /** Whether a running lookup covers it. */
    pending?: boolean;
    /** The line under its title. */
    detail: string;
  }

  interface Section {
    key: string;
    /** '' for none. */
    heading: string;
    hint: string;
    rows: Row[];
  }

  /** One of the two things a row's toggles put a track in or take it out of. */
  interface ToggleSet {
    id: string;
    icon: LucideIcon;
    label: string;
    hint: string;
    onClass: string;
    has: (permalink: string) => boolean;
    toggle: (track: ListedEntry) => void;
  }

  /** A marker the selection bar puts on the whole batch. */
  interface BulkAction {
    id: string;
    icon: LucideIcon;
    label: string;
    hint: string;
    /** How many it really touched, where that can differ from the batch. */
    run: (picked: { batch: ListedEntry[]; permalinks: string[] }) => Promise<number | void>;
    said: (n: number) => string;
  }

  let { tool, onback }: PanelProps = $props();

  // The stored value stays 'page' though the words changed: it is part of the
  // exported state file's format (state-transfer.js, and the schema in docs/),
  // and renaming it would make every file already written say nothing about
  // the sort order. What it named was the page's order and is now the list's,
  // which is the same idea — the order things came in — over a thing that
  // outlives one page.
  const SORTS = [
    { value: 'site', label: 'by site' },
    { value: 'page', label: 'list order' },
  ];

  const SORT_HINT =
    'by site groups rows by where their links point. list order keeps the order you added them.';

  const BACKGROUND_HINT = 'opens links behind the popup instead of taking focus.';

  // Both row toggles work on the one row they are on; a batch goes through
  // Select instead, which says so itself.
  const MARK_HINT = 'mark this one downloaded, or take it off again.';

  const QUEUE_HINT = 'drops this in the session queue. opening a download link queues it too.';

  const SELECT_HINT =
    'puts a tick box on every row, and a bar above the list to act on the lot. shift-click to ' +
    'take in a run.';

  const ART_HINT =
    'click to copy the cover at 1080 × 1080. ⌘-click (ctrl on Windows/Linux) opens it in a tab.';

  const OPEN_HINT = 'the rest of what’s known about this track. click a field to copy it.';

  const FACT_HINT = 'click to copy';

  // The pair of marks an opened row draws instead of spelling out, and the
  // three ways one is drawn — a filled chip for yes, the outline alone for no,
  // and a dashed box around a faint one for a row that never drew the button,
  // which is what "nobody said" looks like.
  const MARK_ICONS = { liked: Heart, reposted: Repeat2 };

  /** Keyed by the state as a string: 'true', 'false' or 'null'. */
  const MARK_STYLES = {
    true: 'border-transparent bg-well text-fg',
    false: 'border-transparent text-fg-subtle',
    null: 'border-dashed border-line-strong text-fg-subtle/60',
  };

  const LINK_HINT = 'click to copy the address. the arrow opens the track in a tab.';

  const UNMARK_HINT = 'takes the downloaded mark off every track. the list stays.';

  // What a page that cannot be added says instead of a count. The shared
  // sentence tells you to click the extension again, which was true when the
  // panel was the page; this one stays open and waits.
  const NO_PAGE = 'not a SoundCloud playlist, likes or track page. open one to add it.';

  // What to call a page that never rendered a title, by the kind of page it
  // is. Only used for `from`, which is what a row says it came in from.
  const PAGE_KINDS: Partial<Record<PageType, string>> = {
    likes: 'a likes page',
    track: 'a track page',
    playlist: 'a playlist',
  };

  const NOT_CHECKED_HINT = 'nobody has asked SoundCloud about these. add the page they’re on.';

  const CHECKING_HINT = 'asking SoundCloud about these.';

  const here = untrack(() => tool.id);

  const sort = choicePref('pref:sort', 'site', ['site', 'page']);

  const background = flagPref('pref:background', true);

  const filter = createFilter(here);

  // --- the search badges -----------------------------------------------------
  //
  // Which sites a link-less row offers is a setting rather than a fact about
  // the list ($lib/search-sites.svelte.js), so it can change while this panel
  // is on screen — the sheet is thrown over it and the rows behind redraw. The
  // two sentences that name the sites have to answer for all of them being
  // switched off, which is a row with nothing on it but its toggles: better
  // said than left looking like a row the tool forgot to finish.
  const badgeSites = $derived(searchSites.enabled);

  const LISTED_BADGES_HINT = $derived(
    badgeSites.length === 0
      ? 'tracks with no link carry no search badges — turn some on in settings.'
      : `tracks with no link carry badges that search the artist on ${siteNames(badgeSites)}.`
  );

  const NO_LINKS_HINT = $derived(
    badgeSites.length === 0
      ? 'no SoundCloud download and no purchase link, and no search badges switched on to ' +
          'try elsewhere with.'
      : 'no SoundCloud download and no purchase link. the badges search the artist ' +
          `on ${siteNames(badgeSites)}.`
  );

  // --- the list --------------------------------------------------------------
  //
  // What is drawn is what the list holds, plus — while an add is running — the
  // answers that have arrived for it but not yet been written back. Those
  // arrive a few at a time and are folded in here rather than written one
  // batch at a time, so a hundred-track page costs the list one write at the
  // end instead of one every few hundred milliseconds.

  let ready = $state(false);

  listed.start().then(() => (ready = true));

  /** Details learned so far, while a lookup runs. */
  let streamed: Map<string, KnownDetails> = $state(new Map());

  /** The permalinks the running lookup covers, so their rows can say so. */
  let checkingIds: Set<string> = $state(new Set());

  const tracks = $derived.by(() => {
    const items = listed.items;

    if (streamed.size === 0) return items;

    return items.map((item) => {
      const best = betterDetails(item.details, streamed.get(item.permalink));

      return best === item.details ? item : { ...item, details: best };
    });
  });

  // --- what there is to add --------------------------------------------------
  //
  // One read of the active tab, started before this panel existed (see `warm`
  // in $lib/tools.js) and used only to describe the button: the page's own
  // rows, which is a few milliseconds of reading its DOM and asks SoundCloud
  // nothing. Pressing the button is what asks.

  /** The tab in front, as a source to add from. */
  interface PageSource {
    reading: boolean;
    name: string;
    from: string;
    curator: string;
    skipped: number;
    tracks: PageTrack[];
    reason: string;
  }

  let page: PageSource = $state({
    reading: true,
    name: '',
    from: '',
    curator: '',
    skipped: 0,
    tracks: [],
    reason: '',
  });

  async function readTab() {
    const result = await readPage('EXTRACT_TRACKS');

    if (result.status) {
      page = {
        reading: false,
        name: '',
        from: '',
        curator: '',
        skipped: 0,
        tracks: [],
        reason: result.status === OPEN_A_PAGE ? NO_PAGE : result.status,
      };

      return;
    }

    const { title, curator, pageType, skippedPlaylists, tracks: found } = result.data!;
    page = {
      reading: false,
      name: title || 'this tab',
      // What the tracks are filed under. A page with no title of its own —
      // a public likes page with no heading, a track page whose hero has not
      // drawn its title — renders none at all, and "unknown" on every row of
      // it would be a worse answer than the kind of page it was.
      from: title || PAGE_KINDS[pageType] || 'a playlist',
      curator,
      // Likes pages can mix liked tracks with liked playlists, and the content
      // script skips the playlists. Said here rather than in the list's own
      // summary, because it is a fact about the page being offered — the list
      // spans pages and has no business repeating one of them.
      skipped: skippedPlaylists,
      tracks: found,
      reason: '',
    };
  }

  void readTab();

  // How much of what the tab offers is new, which is what its button counts.
  // A track with no permalink is not counted: the list cannot hold one (there
  // would be nothing to tell it from another), so calling it new would leave a
  // page for ever offering to add something adding never adds.
  const pageFresh = $derived(
    page.tracks.filter((track) => track.permalink && !listed.has(track.permalink)).length
  );

  const pageHere = $derived(
    page.tracks.filter((track) => track.permalink && listed.has(track.permalink)).length
  );

  const queuedFresh = $derived(queue.items.filter((item) => !listed.has(item.permalink)).length);

  const offer = $derived({
    reading: page.reading,
    name: page.name,
    fresh: pageFresh,
    here: pageHere,
    reason: page.reason,
    // What the page holds, not what the list would take: "13 tracks" is what
    // somebody can count on the page in front of them, and a row of it this
    // list cannot hold is not something to argue with them about.
    meta: [
      page.curator ? `by ${page.curator}` : '',
      count(page.tracks.length, 'track'),
      pageHere > 0 ? `${pageHere} already here` : '',
      page.skipped > 0 ? `${count(page.skipped, 'liked playlist')} skipped` : '',
    ]
      .filter(Boolean)
      .join(' · '),
  });

  // --- adding ----------------------------------------------------------------

  let adding = $state(false);

  let progress: DetailsProgress | null = $state(null);

  // What an add came to, when that is something to leave on screen rather than
  // to slide past: a page that could not be read, tracks SoundCloud would not
  // describe, tracks that did not fit. An add with nothing to add to that gets
  // a toast instead — the list itself is the report.
  let note: { tone: 'info' | 'bad'; text: string } | null = $state(null);

  // Answers arriving from the lookup, a few at a time, so a long page fills in
  // as it goes rather than all at once at the end.
  try {
    chrome.runtime.onMessage.addListener((message: PageProgress) => {
      if (!message || message.type !== 'DOWNLOAD_DETAILS_PROGRESS') return;

      if (!adding) return;
      progress = message;

      if (!message.details?.length) return;
      const next = new Map(streamed);

      for (const details of message.details) next.set(details.permalink, details);
      streamed = next;
    });
  } catch (_e) {
    // No runtime to listen on: the lookup just finishes without a count.
  }

  // What an add came to, as one sentence. Written from what the list reports
  // rather than from what was handed to it, so "12 added, 30 refreshed" is
  // always the truth about rows rather than about intentions.
  const said = ({ added, refreshed }: { added: number; refreshed: number }, from: string) => {
    const parts = [];

    if (added > 0) parts.push(`added ${count(added, 'track')}`);

    if (refreshed > 0) parts.push(`refreshed ${count(refreshed, 'track')}`);

    if (parts.length === 0) parts.push('nothing to add');

    return `${parts.join(', ')} from ${from}`;
  };

  // Said only when it happened: the list is full, and some of what was handed
  // to it is not on it. That is the one outcome of an add that has to stay on
  // screen, since nothing in the list itself shows what is missing from it.
  const refusal = (tally: { refused: number }) =>
    tally.refused > 0
      ? `${count(tally.refused, 'track')} wouldn’t fit: the list holds ${MAX}. ` +
        'take some off and add again.'
      : '';

  // How an add reports itself. Anything that has to be read — a page that
  // could not be reached, tracks SoundCloud would not describe, tracks that
  // did not fit — is a note and stays; a clean add is a toast, because the
  // rows that just appeared are the report.
  /** @param extra '' for a clean add. */
  function reported(text: string, extra: string, tone: 'info' | 'bad' = 'info') {
    if (extra) note = { tone, text: `${text}. ${extra}` };
    else toasts.ok(text);
  }

  async function addFromTab() {
    adding = true;
    note = null;
    progress = null;
    const from = page.from;
    const found = page.tracks;
    // The rows go up now, from what the page itself rendered — on a Likes page
    // that already carries the purchase links — and are marked as being looked
    // up, so the list is on screen before SoundCloud has been asked a thing.
    checkingIds = new Set(found.map((track) => track.permalink));
    const first = await listed.add(found, { from });

    const result = await readPage('GET_DOWNLOAD_DETAILS');

    if (result.status || LOOKUP_ERRORS[result.data!.error]) {
      // The rows are on the list either way — they came off the page, not out
      // of the lookup — so this says what is missing from them rather than
      // that nothing happened.
      const why = result.status || LOOKUP_ERRORS[result.data!.error];
      note = { tone: 'bad', text: `${said(first, from)}, but their links weren’t checked. ${why}` };
    } else {
      // Everything the page holds, answers and all. A track already on the
      // list keeps its place and takes the better of the two answers.
      const second = await listed.add(result.data!.tracks, { from });
      // The two reads are mostly the same tracks, so a track neither could fit
      // was refused twice: what did not fit is what is still not on the list.
      const offered = new Set([...found, ...result.data!.tracks].map((track) => track?.permalink));

      const tally = {
        added: first.added + second.added,
        refreshed: first.refreshed,
        refused: [...offered].filter((permalink) => permalink && !listed.has(permalink)).length,
      };

      reported(said(tally, from), [refusal(tally), lookupNotes(result.data!)].filter(Boolean).join(' '));
    }

    // The list has the answers now; the streamed copies would only shadow them.
    streamed = new Map();
    checkingIds = new Set();
    progress = null;
    adding = false;
  }

  async function addFromQueue() {
    adding = true;
    note = null;
    // Queued tracks carry whatever was known when they were queued, which is
    // often a full answer and sometimes none at all. Nothing is looked up
    // here: the lookup runs on the page a track is on, and the whole point of
    // this source is that that page need not be open.
    const tally = await listed.add(queue.items, { from: 'the session queue' });
    adding = false;
    reported(said(tally, 'the session queue'), refusal(tally));
  }

  const onadd = (source: 'tab' | 'queue') => (source === 'queue' ? addFromQueue() : addFromTab());

  // --- the sections ----------------------------------------------------------

  // A row's identity for `{#each}`. The list holds a track once, but the
  // by-site layout can draw one twice — a SoundCloud download and a purchase
  // link are two rows — and Svelte insists a key be unique.
  const rowKey = (index: number, ...parts: string[]) => [index, ...parts].join(':');

  // One flat list of rows, whichever way it is sorted, so the filter and the
  // counts have a single thing to work on. `heading` starts a new section.
  const sections = $derived.by(() => {
    if (tracks.length === 0) return [];

    if (sort.value === 'page') {
      const rows = buildListOrder(tracks, { checking: checkingIds });
      const hints = [];

      if (rows.some((row) => row.entries.some((entry) => entry.kind === 'soundcloud'))) {
        hints.push(SOUNDCLOUD_HINT);
      }

      if (rows.some((row) => !row.pending && row.entries.length === 0)) {
        hints.push(LISTED_BADGES_HINT);
      }

      return [
        {
          key: 'page',
          heading: '',
          hint: hints.join(' '),
          rows: rows.map(({ track, entries, pending }, index) => {
            // Best link first, so the detail line reads in the same order as
            // the buttons beside it: the one that leads, then the one that
            // trails.
            const { ordered, primary, secondary } = splitEntries(entries);

            return {
              key: rowKey(index, track.permalink),
              track,
              entries: ordered,
              primary,
              secondary,
              pending,
              detail: pending
                ? 'checking…'
                : ordered.length === 0
                  ? isWithoutLinks(track)
                    ? 'no download links'
                    : 'not checked'
                  : ordered
                      .map((entry) => (entry.note ? `${entry.host} (${entry.note})` : entry.host))
                      .join(' · '),
            };
          }),
        },
      ];
    }

    const bySite: Section[] = buildGroups(tracks).map(([domain, entries]) => ({
      key: domain,
      heading: domain,
      hint: entries.some((entry) => entry.kind === 'soundcloud') ? SOUNDCLOUD_HINT : '',
      rows: entries.map((entry, index): Row => ({
        key: rowKey(index, domain, entry.track.permalink, entry.url),
        track: entry.track,
        entries: [entry],
        primary: entry,
        secondary: [],
        // The host, only when the section heading does not already say it,
        // plus SoundCloud's own download state.
        detail: [entry.host === domain ? '' : entry.host, entry.note].filter(Boolean).join(' · '),
      })),
    }));

    const noLinks = tracksWithoutLinks(tracks);

    if (noLinks.length > 0) {
      bySite.push({
        key: 'no-links',
        heading: 'no download links',
        hint: NO_LINKS_HINT,
        rows: noLinks.map((track, index): Row => ({
          key: rowKey(index, track.permalink),
          track,
          entries: [],
          primary: null,
          secondary: [],
          detail: '',
        })),
      });
    }

    // Sorted by site, a track cannot be filed until its site is known, so the
    // ones nobody has an answer about wait here — last, since they are the
    // part of the list that is going somewhere else. During an add they leave
    // a few at a time as the answers come in; the rest sit until the page they
    // are on is added.
    const unchecked = uncheckedTracks(tracks);

    if (unchecked.length > 0) {
      bySite.push({
        key: 'unchecked',
        heading: 'not checked',
        hint: adding ? CHECKING_HINT : NOT_CHECKED_HINT,
        rows: unchecked.map((track, index): Row => ({
          key: rowKey(index, 'unchecked', track.permalink),
          track,
          entries: [],
          primary: null,
          secondary: [],
          pending: checkingIds.has(track.permalink),
          detail: '',
        })),
      });
    }

    return bySite;
  });

  // The filter hides rows, and with them any section left empty. It counts
  // tracks rather than rows, since a track listed in two sections is still
  // one track.
  const visible = $derived(
    sections
      .map((section) => {
        const rows = section.rows.filter((row) =>
          filter.keeps(searchText(row.track, row.entries), downloaded.has(row.track.permalink))
        );

        // `matched` is what the filter left, which is what the heading counts;
        // `rows` is what gets drawn, and painting cuts that down further while
        // a long list is still going up (see `painted`).
        return { ...section, total: section.rows.length, matched: rows.length, rows };
      })
      .filter((section) => section.rows.length > 0)
  );

  // --- painting --------------------------------------------------------------
  //
  // A row is a picture, a title, three or four buttons and their tooltips, and
  // five hundred of them are the best part of a second of DOM work. The popup
  // shows about eight at a time. So the first frame draws a screenful and the
  // rest arrive over the following few: the same work, done behind the list
  // rather than in front of it.
  const FIRST_PAINT = 60;

  const PER_FRAME = 200;

  let drawn = $state(FIRST_PAINT);

  const rowCount = $derived(visible.reduce((n, section) => n + section.rows.length, 0));

  // A frame if there is one, a timeout if there is not. `requestAnimationFrame`
  // is what says "after the paint", which is the whole point of chunking — but
  // it does not run at all while the page is hidden, and a list that stopped
  // growing there would still be sixty rows long when it came back.
  $effect(() => {
    if (drawn >= rowCount) return;
    const grow = () => (drawn += PER_FRAME);
    const frame = requestAnimationFrame(grow);
    const timer = setTimeout(grow, 50);

    return () => {
      cancelAnimationFrame(frame);
      clearTimeout(timer);
    };
  });

  // `visible`, cut off at what has been drawn so far. Sections keep their own
  // counts: those are about the filter, not about how much of the list has
  // made it into the DOM yet.
  const painted = $derived.by(() => {
    if (drawn >= rowCount) return visible;
    let left = drawn;
    const out = [];

    for (const section of visible) {
      if (left <= 0) break;
      out.push({ ...section, rows: section.rows.slice(0, left) });
      left -= Math.min(left, section.rows.length);
    }

    return out;
  });

  const listedTracks = $derived(
    new Set(sections.flatMap((s) => s.rows.map((row) => row.track.permalink))).size
  );

  const shownTracks = $derived(
    new Set(visible.flatMap((s) => s.rows.map((row) => row.track.permalink))).size
  );

  const linkCount = $derived(
    tracks.reduce((sum, track) => sum + entriesFor(track).length, 0)
  );

  const noLinkCount = $derived(tracksWithoutLinks(tracks).length);

  const uncheckedCount = $derived(uncheckedTracks(tracks).length);

  const sourceCount = $derived(listed.sources.length);

  // The line under the title: what the list is, and what is in it. Unlike the
  // page summary it replaced, this says nothing about a tab — the list outlives
  // every one of them.
  const meta = $derived(
    tracks.length === 0
      ? ''
      : [
          count(tracks.length, 'track'),
          count(linkCount, 'download link'),
          ...(noLinkCount > 0 ? [`${noLinkCount} without`] : []),
          ...(uncheckedCount > 0 ? [`${uncheckedCount} not checked`] : []),
          ...(sourceCount > 1 ? [count(sourceCount, 'source')] : []),
        ].join(' · ')
  );

  // --- the two row toggles ---------------------------------------------------
  //
  // A row carries two of them — the list button that queues a track, the check
  // button that marks it as downloaded — and they behave alike, because there
  // is one click here and two sets for it to work on. A set says how to read it
  // and how to write it; everything else, the button included, is shared.
  //
  // Each works on the one row it is on. A batch goes through the selection
  // below instead, which is also what the sets' `add` and `remove` are for.

  const permalinksOf = (list: ListedEntry[]) => list.map((track) => track.permalink);

  const QUEUE_SET: ToggleSet = {
    id: 'queue',
    icon: ListPlus,
    label: 'add to the session queue',
    hint: QUEUE_HINT,
    // What the button looks like when the track is in the set; `variant`
    // carries the rest.
    onClass: '',
    has: (permalink) => queue.has(permalink),
    toggle: (track) => queue.toggle(track),
  };

  const MARK_SET: ToggleSet = {
    id: 'mark',
    icon: Check,
    label: 'mark downloaded',
    hint: MARK_HINT,
    onClass: 'bg-good text-fg-inverse hover:bg-good/85',
    has: (permalink) => downloaded.has(permalink),
    toggle: (track) => downloaded.toggle(track.permalink),
  };

  const TOGGLES = [QUEUE_SET, MARK_SET];

  // --- selecting a batch -----------------------------------------------------
  //
  // Select turns the list into something to pick out of: every row grows a tick
  // box, and the bar above the list puts a marker on everything ticked — one
  // write per marker, however many tracks. Ticking is nothing but ticking, so
  // the batch can be looked at before anything is done to it, and the same
  // batch can be queued *and* marked.
  //
  // Which tracks: the ones ticked, in list order — the order they were added
  // in, not the order they were ticked in. A track the by-site layout draws
  // twice is one tick and one track.

  const picks = createSelection(here);

  // The rows on screen, in the order they are shown, which is what a
  // Shift-click reaches between and what Select all takes in. A track listed in
  // two sections appears once: it is one tick either way.
  const shownOrder = $derived([
    ...new Set(visible.flatMap((section) => section.rows.map((row) => row.track.permalink))),
  ]);

  // The rows the tab in front is showing, in list order: what **this page**
  // ticks. The list spans every page it was built from, and the one somebody
  // is looking at is rarely all of it — so "everything on this page" is a
  // gesture of its own, above the box that takes in the lot. It is the same
  // read of the tab the add bar is describing (`page`), so it costs nothing:
  // no request goes out for it and none is needed.
  //
  // Which rows: the page's tracks that are on the list *and* on screen, in the
  // order the list is showing them. A row the filter hides is not one of them,
  // for the reason the box at the left of the bar leaves it out too — a tick
  // on a row nobody can see is a track in the batch nobody can see in it.
  const offered = $derived(new Set(page.tracks.map((one) => one.permalink).filter(Boolean)));

  const pageOrder = $derived(shownOrder.filter((permalink) => offered.has(permalink)));

  const pageTicked = $derived(
    pageOrder.length > 0 && pageOrder.every((permalink) => picks.has(permalink))
  );

  // Why the button is off, where it is — and what it would take, where it is
  // not. A page whose rows are all hidden says so rather than "nothing here is
  // on the list": the filter is a thing somebody can undo, and being told the
  // page was never added when it was would send them to add it again.
  const pageTickHint = $derived.by(() => {
    if (page.reading) return 'still looking at this tab.';

    if (pageOrder.length > 0)
      return `ticks the ${count(pageOrder.length, 'track')} showing from ${page.name || 'this tab'}.`;

    if (pageHere > 0) return 'every track from this tab is hidden by the filter.';

    return page.reason || 'nothing this tab is showing is on the list yet.';
  });

  const ticked = $derived(tracks.filter((track) => picks.has(track.permalink)));

  function onTick(event: MouseEvent, permalink: string) {
    if (event.shiftKey) return picks.extend(shownOrder, permalink);
    picks.toggle(permalink);
  }

  // What the bar can do to the batch. Each is one write: the stores take a
  // whole set at a time, so a hundred ticked tracks cost what one does.
  const BULK: BulkAction[] = [
    {
      id: 'queue',
      icon: ListPlus,
      label: 'queue',
      hint: 'drops every ticked track in the session queue.',
      // How many were really added: the queue refuses a track it already holds.
      run: ({ batch }) => queue.add(batch),
      said: (n) => `queued ${count(n, 'track')}`,
    },
    {
      id: 'unqueue',
      icon: ListX,
      label: 'unqueue',
      hint: 'pulls every ticked track out of the queue. marks stay.',
      run: ({ permalinks }) => queue.removeMany(permalinks),
      said: (n) => `unqueued ${count(n, 'track')}`,
    },
    {
      id: 'mark',
      icon: Check,
      label: 'mark',
      hint: 'marks every ticked track downloaded, here and on the SoundCloud page.',
      run: ({ permalinks }) => downloaded.setMany(permalinks, true),
      said: (n) => `marked ${count(n, 'track')} downloaded`,
    },
    {
      id: 'unmark',
      icon: X,
      label: 'unmark',
      hint: 'takes the downloaded mark off every ticked track.',
      run: ({ permalinks }) => downloaded.setMany(permalinks, false),
      said: (n) => `unmarked ${count(n, 'track')}`,
    },
    {
      id: 'remove',
      icon: Trash2,
      label: 'remove',
      hint: 'takes every ticked track off the list. marks and queue places stay.',
      // The ticks go with them: what they pointed at is not on the list any
      // more, and a selection of rows that are gone is not a selection.
      run: async ({ permalinks }) => {
        await listed.removeMany(permalinks);
        picks.pick(permalinks, false);
      },
      said: (n) => `took ${count(n, 'track')} off the list`,
    },
  ];

  // The ticks stay after a marker is put on: a batch worth queueing is often
  // the same batch to mark later, and unticking it is one click away. (Remove
  // is the exception, and unticks its own, since the rows are gone.)
  async function onBulk(action: BulkAction) {
    const batch = ticked;
    const permalinks = permalinksOf(batch);
    const done = await action.run({ batch, permalinks });
    toasts.ok(action.said(done ?? batch.length));
  }

  async function onRemoveRow(track: ListedEntry) {
    await listed.removeMany([track.permalink]);
    picks.pick([track.permalink], false);
    toasts.ok(`took ${track.title || 'the track'} off the list`);
  }

  // --- cover art -------------------------------------------------------------
  //
  // The thumbnail each row already shows is also the handle on the full-size
  // artwork: a click copies that image, a ⌘-click opens it in a tab. The URL
  // is derived from the thumbnail's own address (see lib/artwork.js), so
  // nothing is fetched to find it — and nothing is fetched at all until a
  // plain click asks for the image itself, exactly one per click.

  /** For the thumbnail last clicked. */
  let art: Map<string, 'copying' | 'copied' | ''> = $state(new Map());

  function flashArt(permalink: string, state: 'copying' | 'copied' | '') {
    art = new Map(art).set(permalink, state);

    if (state !== 'copying') setTimeout(() => (art = new Map(art).set(permalink, '')), 1400);
  }

  async function onArtwork(event: MouseEvent, track: ListedEntry) {
    const url = fullSizeArtwork(artworkOf(track));

    if (event.metaKey || event.ctrlKey) {
      await openTab(url, { background: background.value });

      return;
    }

    // A second click while the first is still fetching would copy the same
    // image twice over; the row is busy until its own fetch is done.
    if (art.get(track.permalink) === 'copying') return;
    flashArt(track.permalink, 'copying');
    const copied = await copyImage(url);
    flashArt(track.permalink, copied ? 'copied' : '');
    toasts.said(copied, 'copied the cover at 1080 × 1080', 'couldn’t copy the cover');
  }

  // --- opening a row ---------------------------------------------------------
  //
  // A row is a thumbnail, a title and four buttons wide, and a popup is 400px
  // wide, so everything else the extension knows about a track had nowhere to
  // be said. The chevron opens the row downwards into it — the track as
  // SoundCloud describes it, one field a line, each of them a button that puts
  // that field on the clipboard. Which is the point: these are the words
  // somebody is about to type into a spreadsheet, a message or a search box,
  // and they are already on screen.
  //
  // Which rows are open is a place rather than a setting — kept for the
  // browsing session, like the search above the list and a half-made selection
  // (see $lib/view.svelte.js). Opening a download link closes the popup, and a
  // row opened to read an address off is exactly the row somebody comes back
  // to.
  //
  // Open by permalink, not by row: a track the by-site layout lists twice is
  // one track, and it opens in both places at once — the same rule the tick
  // boxes follow.
  const opened: Remembered<string[]> = remembered(here, 'opened', []);

  const isOpen = (permalink: string) => opened.value.includes(permalink);

  function toggleOpen(permalink: string) {
    opened.value = isOpen(permalink)
      ? opened.value.filter((one) => one !== permalink)
      : [...opened.value, permalink];
  }

  /** @param fact one of rowFacts' fields. */
  async function onCopyFact(fact: { label: string; copy: string }) {
    const copied = await copyText(fact.copy);
    toasts.said(copied, `copied the ${fact.label}`, `couldn’t copy the ${fact.label}`);
  }

  // What a link button says it will do. The same sentence wherever the link
  // is shown, leading or trailing.
  const linkHint = (entry: LinkEntry) =>
    entry.kind === 'soundcloud'
      ? `opens the track page — grab it from More → Download file` +
        (entry.spent ? ' (no downloads left)' : '')
      : // The uploader's own word for the link ("Free Download", "Buy", "🎧
        // Stream / Download") is worth having, but it is their wording and not
        // a description of a button, so it is said here rather than written
        // across the button itself.
        `“${entry.label}” — opens ${entry.host}, and marks this downloaded`;

  async function open(entry: LinkEntry) {
    // Opening the link is where the work starts, not where it ends: the track
    // goes into the session queue, and is marked as downloaded from there once
    // the file has really been saved. (A SoundCloud download is marked without
    // any of this when its "Download file" button is used — the content script
    // sees that click.)
    await queue.add([entry.track]);
    await openTab(entry.url, { background: background.value });
  }

  // "Unmark all" asks once more: the first click arms the button for a few
  // seconds, the second click takes every mark off every page. It says
  // "unmark" rather than "clear" because there is now a list it could be read
  // as clearing, and it has never touched that.
  let armed = $state(false);

  let armedTimer: ReturnType<typeof setTimeout> | 0 = 0;

  function onUnmarkAll() {
    if (!armed) {
      armed = true;
      armedTimer = setTimeout(() => (armed = false), 4000);

      return;
    }

    clearTimeout(armedTimer);
    armed = false;
    downloaded.clear();
    toasts.ok('every mark cleared');
  }

</script>

<!-- One button per set, so both are the same button: the same states, the
     same click, the same tooltip shape. -->
{#snippet toggleButton(track: ListedEntry, set: ToggleSet)}
  {@const on = set.has(track.permalink)}
  {@const Icon = set.icon}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Button
          {...props}
          size="icon-sm"
          variant={on ? 'default' : 'outline'}
          class={on ? set.onClass : 'text-fg-muted'}
          aria-pressed={on}
          aria-label={set.label}
          onclick={() => set.toggle(track)}
        >
          <Icon />
        </Button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content class="max-w-64">{set.hint}</Tooltip.Content>
  </Tooltip.Root>
{/snippet}

<!-- What a row opens into: every field of the track, each one a button that
     copies it. A field nothing is known about ("unknown", "none") is drawn the
     same but dead — there is nothing to put on a clipboard, and copying the
     word "unknown" into a spreadsheet would put a claim there that nobody
     made about the track. The address gets an arrow of its own, since a link
     whose only gesture is "copy" is a link that does not open; and the way off
     the list is here, where there is room to say what it does, rather than as
     a fifth button on a row that has no room for a fourth.

     Liked and reposted are not fields down there. They are a yes or a no
     rather than words anybody pastes anywhere, so they are a pair of icons on
     the last line instead, beside the way off the list. -->
{#snippet rowDetails(track: ListedEntry)}
  {@const facts = rowFacts(track, { titleCased: titleCasing.value })}
  <dl
    class="mt-1.5 flex flex-col gap-px border-t border-line pt-1.5 text-caption"
    aria-label={`about ${track.title}`}
  >
    {#each facts as fact (fact.id)}
      <div class="flex items-baseline gap-2">
        <dt class="w-14 shrink-0 font-mono text-[10.5px] text-fg-subtle uppercase">{fact.label}</dt>
        <dd class="flex min-w-0 flex-1 items-baseline gap-1">
          <button
            type="button"
            class="min-w-0 flex-1 truncate rounded-xs px-1 py-0.5 text-left text-fg-soft
                   enabled:cursor-pointer enabled:hover:bg-well enabled:hover:text-fg
                   disabled:text-fg-subtle disabled:italic"
            disabled={!fact.copy}
            aria-label={fact.copy ? `copy the ${fact.label}` : `${fact.label}: ${fact.value}`}
            title={fact.copy ? (fact.id === 'link' ? LINK_HINT : FACT_HINT) : ''}
            onclick={() => onCopyFact(fact)}
          >
            {fact.value}
          </button>
          {#if fact.id === 'link' && fact.copy}
            <Button
              size="icon-xs"
              variant="ghost"
              class="shrink-0 text-fg-muted hover:text-fg"
              aria-label="open the track in a new tab"
              title="opens in a tab — queues nothing, marks nothing."
              onclick={() => openTab(fact.value, { background: background.value })}
            >
              <ExternalLink />
            </Button>
          {/if}
        </dd>
      </div>
    {/each}
  </dl>
  <div class="mt-1 flex items-center justify-between">
    <div class="flex items-center gap-1">
      {#each rowMarks(track) as mark (mark.id)}
        {@const Icon = MARK_ICONS[mark.id]}
        <!-- Three states, drawn as three different things rather than two
             shades of one: yes is filled in, no is the outline on its own, and
             "nobody said" is the dashed box — an empty frame, which is what
             happened. -->
        <span
          role="img"
          aria-label={mark.label}
          title={mark.hint}
          class={`flex size-5 items-center justify-center rounded-xs border ${MARK_STYLES[`${mark.state}` as const]}`}
        >
          <!-- Only the heart fills: a heart with something in it is what a
               like looks like everywhere, and the repost arrows at 12px turn
               into a blot when they are filled. The chip behind it is what
               says "yes" for that one. -->
          <Icon class={mark.state === true && mark.id === 'liked' ? 'size-3 fill-current' : 'size-3'} />
        </span>
      {/each}
    </div>
    <Button
      size="xs"
      variant="ghost"
      class="text-fg-muted hover:text-critical"
      aria-label={`take ${track.title} off the list`}
      title="off the list. its mark and queue place stay."
      onclick={() => onRemoveRow(track)}
    >
      <Trash2 />
      off the list
    </Button>
  </div>
{/snippet}

<!-- The chevron that opens a row. On every row, whatever its links turn out
     to be: the title and the artist are known before the lookup has said a
     word, so there is always something under it. -->
{#snippet openButton(track: ListedEntry)}
  {@const on = isOpen(track.permalink)}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Button
          {...props}
          size="icon-sm"
          variant="ghost"
          class="text-fg-muted"
          aria-expanded={on}
          aria-label={`about ${track.title}`}
          onclick={() => toggleOpen(track.permalink)}
        >
          <ChevronDown class={on ? 'rotate-180 transition-transform' : 'transition-transform'} />
        </Button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content class="max-w-64">{OPEN_HINT}</Tooltip.Content>
  </Tooltip.Root>
{/snippet}

<!-- The link a row does not lead with: the same button, quieter and without
     its label, since the row's detail line already says where it goes. -->
{#snippet otherLink(entry: LinkEntry)}
  <Button
    size="icon-sm"
    variant="ghost"
    class="text-fg-muted hover:text-fg"
    aria-label={`also — ${downloadLabel(entry)}`}
    title={linkHint(entry)}
    onclick={() => open(entry)}
  >
    <ExternalLink />
  </Button>
{/snippet}

<!-- The badges a row with no link carries: the sites switched on in settings,
     in the order the file lists them. What each searches for is its own
     template's doing — the artist, on every site this ships with — so the
     button quotes the words rather than naming the field. -->
{#snippet searchBadges(track: ListedEntry)}
  {#each badgeSites as site (site.id)}
    {@const query = queryOf(site, track)}
    <Button
      size="icon-sm"
      variant="ghost"
      style={site.style}
      class="w-7 border-transparent text-[10px] font-bold hover:brightness-110"
      disabled={!searchable(site, track)}
      aria-label={`search ${site.name} for ${query}`}
      title={`search ${site.name} for “${query}”`}
      onclick={() => openTab(searchUrl(site, track), { background: background.value })}
    >
      {site.badge}
    </Button>
  {/each}
{/snippet}

<PanelShell {tool} {onback} {meta}>
  {#snippet actions()}
    <!-- No words on it: the header is 400px wide and the tool's own name wants
         them. What it does is in the tooltip, and turning it on says the rest —
         a tick box on every row, and the bar that goes with them. A tick box
         is also what it is drawn as: the queue button two along carries the
         list-with-ticks icon, and two of those in one header were two buttons
         nobody could tell apart. -->
    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button
            {...props}
            size="icon-sm"
            variant={picks.active ? 'default' : 'outline'}
            class={picks.active ? '' : 'text-fg-muted'}
            aria-pressed={picks.active}
            aria-label="select several tracks"
            disabled={tracks.length === 0}
            onclick={() => (picks.active = !picks.active)}
          >
            <SquareCheckBig />
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content class="max-w-72">{SELECT_HINT}</Tooltip.Content>
    </Tooltip.Root>

    <Button
      size="sm"
      variant={armed ? 'default' : 'outline'}
      class={armed ? 'bg-critical text-fg-inverse hover:bg-critical/90' : ''}
      disabled={downloaded.count === 0 && !armed}
      title={UNMARK_HINT}
      onclick={onUnmarkAll}
    >
      <Eraser />
      <!-- "unmark" rather than "clear": there is a list on screen now, and a
           button called clear beside it would read as the thing that empties
           it. The count is the marks it takes off. -->
      {#if armed}
        unmark {downloaded.count}?
      {:else if downloaded.count > 0}
        unmark ({downloaded.count})
      {:else}
        unmark
      {/if}
    </Button>
  {/snippet}

  {#snippet banner()}
    {#if adding && progress}
      <div class="shrink-0 border-b border-line px-3 py-2">
        <div class="mb-1 flex items-center justify-between text-caption text-fg-muted">
          <span>
            checking download details…
            {#if progress.resumed}<span>(already running)</span>{/if}
          </span>
          <span class="font-mono">{progress.done} / {progress.total}</span>
        </div>
        <Progress value={progress.done} max={progress.total || 1} class="h-1" />
      </div>
    {/if}
  {/snippet}

  {#if !ready}
    <StatusNote tone="busy">reading your list…</StatusNote>
  {:else}
    <div class="flex flex-col gap-2.5">
      <AddBar
        {offer}
        {adding}
        queued={queue.count}
        {queuedFresh}
        big={tracks.length === 0}
        tone={tool.tone}
        {onadd}
      />

      {#if note}
        <StatusNote tone={note.tone}>{note.text}</StatusNote>
      {/if}

      {#if tracks.length > 0}
        <FilterBar {filter} shown={shownTracks} total={listedTracks} />

        <div class="flex items-center gap-2.5">
          <Tooltip.Root>
            <Tooltip.Trigger class="min-w-0 flex-1">
              {#snippet child({ props })}
                <div {...props} class="min-w-0 flex-1">
                  <Select.Root type="single" bind:value={sort.value}>
                    <Select.Trigger class="h-8 w-full text-label" aria-label="sort order">
                      {SORTS.find((s) => s.value === sort.value)?.label}
                    </Select.Trigger>
                    <Select.Content>
                      {#each SORTS as option (option.value)}
                        <Select.Item value={option.value} label={option.label}>
                          {option.label}
                        </Select.Item>
                      {/each}
                    </Select.Content>
                  </Select.Root>
                </div>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content class="max-w-72">{SORT_HINT}</Tooltip.Content>
          </Tooltip.Root>

          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <div {...props} class="flex shrink-0 items-center gap-1.5">
                  <Switch id="background" bind:checked={background.value} />
                  <Label
                    for="background"
                    class="text-caption font-normal whitespace-nowrap text-fg-muted"
                  >
                    background tabs
                  </Label>
                </div>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content class="max-w-72">{BACKGROUND_HINT}</Tooltip.Content>
          </Tooltip.Root>
        </div>

        {#if picks.active}
          <!-- What is about to be marked, and the markers to put on it. Sticky, so
               working down a long list never leaves the batch's buttons behind. -->
          <div
            class="sticky -top-3 z-20 -mx-3 flex flex-col gap-2 border-b border-line
                   bg-canvas px-3 pt-2 pb-2.5"
          >
            <div class="flex items-center gap-2">
              <!-- Shows what the selection says and never decides anything itself
                   (as the rows' boxes do not either), so both setters are empty:
                   the click below is what ticks or unticks the list. -->
              <Checkbox
                id="select-all"
                class="ml-0.5"
                bind:checked={() => picks.count > 0 && picks.count === shownOrder.length, () => {}}
                bind:indeterminate={() =>
                  picks.count > 0 && picks.count < shownOrder.length,
                () => {}}
                aria-label="select every track shown"
                onclick={() => picks.pick(shownOrder, picks.count < shownOrder.length)}
              />
              <Label for="select-all" class="tabular text-caption font-semibold">
                {picks.count === 0 ? 'nothing ticked' : `${count(picks.count, 'track')} ticked`}
              </Label>
              <Button
                size="xs"
                variant="ghost"
                class="ml-auto text-fg-muted"
                disabled={picks.count === 0}
                onclick={() => picks.clear()}
              >
                untick all
              </Button>
              <Button
                size="xs"
                variant="outline"
                title="hides the tick boxes"
                onclick={() => (picks.active = false)}
              >
                done
              </Button>
            </div>

            <!-- Building the batch, as against acting on it (the row below):
                 the box above ticks the whole list, this ticks the part of it
                 the tab in front is showing. -->
            <div class="flex items-center">
              <Button
                size="xs"
                variant="outline"
                class="text-fg-muted"
                disabled={pageOrder.length === 0}
                title={pageTickHint}
                onclick={() => picks.pick(pageOrder, !pageTicked)}
              >
                <FileMusic />
                {pageTicked ? 'untick this page' : 'tick this page'}
                {#if pageOrder.length > 0}
                  <span class="font-mono">({pageOrder.length})</span>
                {/if}
              </Button>
            </div>

            <div class="flex flex-wrap gap-1.5">
              {#each BULK as action (action.id)}
                {@const Icon = action.icon}
                <Tooltip.Root>
                  <Tooltip.Trigger>
                    {#snippet child({ props })}
                      <Button
                        {...props}
                        size="xs"
                        variant="outline"
                        class={action.id === 'remove'
                          ? 'text-fg-muted hover:text-critical'
                          : ''}
                        disabled={picks.count === 0}
                        onclick={() => onBulk(action)}
                      >
                        <Icon />
                        {action.label}
                      </Button>
                    {/snippet}
                  </Tooltip.Trigger>
                  <Tooltip.Content class="max-w-64">{action.hint}</Tooltip.Content>
                </Tooltip.Root>
              {/each}
            </div>

            <p class="text-caption leading-relaxed text-fg-muted">
              shift-click to take in a run. ticks stay after a marker is put on.
            </p>
          </div>
        {/if}

        <p class="text-caption text-fg-subtle">
          opening a link queues the track.
        </p>

        <div class="flex flex-col gap-3">
          {#each painted as section (section.key)}
            <section class="flex flex-col gap-1.5">
              {#if section.heading}
                <!-- Sticky, so a long site keeps saying which site it is. The
                     negative offsets undo the scrolling body's own padding. A
                     mono label, as the website heads a group. -->
                <h2
                  class="eyebrow sticky -top-3 z-10 -mx-3 m-0 flex items-baseline gap-1.5 border-b
                         border-line bg-canvas px-3 pt-2.5 pb-1.5 font-normal text-fg-soft"
                >
                  {section.heading}
                  <span class="text-fg-subtle">
                    {#if section.matched === section.total}
                      ({section.total})
                    {:else}
                      ({section.matched} of {section.total})
                    {/if}
                  </span>
                </h2>
              {/if}
              {#if section.hint}
                <p class="text-caption text-fg-muted">{section.hint}</p>
              {/if}
              <ul class="flex flex-col gap-1.5">
                {#each section.rows as row (row.key)}
                  <TrackRow
                    permalink={row.track.permalink}
                    title={row.track.title}
                    artist={row.track.artist}
                    detail={row.detail}
                    artworkUrl={artworkOf(row.track)}
                    isDownloaded={downloaded.has(row.track.permalink)}
                    selectable={picks.active}
                    selected={picks.has(row.track.permalink)}
                    onselect={(event) => onTick(event, row.track.permalink)}
                    artworkState={art.get(row.track.permalink) ?? ''}
                    artworkHint={ART_HINT}
                    onartwork={(event) => onArtwork(event, row.track)}
                  >
                    {#snippet actions()}
                      {#if row.pending}
                        <!-- A lookup is running for this one. The badges say
                             "there is nowhere to get this, try searching",
                             which is not something to tell anybody before
                             SoundCloud has answered. -->
                        <LoaderCircle
                          class="size-3.5 shrink-0 animate-spin text-fg-muted"
                          aria-label="checking download details"
                        />
                      {:else if !row.primary}
                        {@render searchBadges(row.track)}
                      {:else}
                        <!-- One icon, the same width on every row, whatever
                             the uploader called their link. What it opens is in
                             the tooltip and in the row's detail line; the row
                             itself keeps the space for the title. -->
                        <Button
                          size="icon-sm"
                          aria-label={downloadLabel(row.primary)}
                          title={linkHint(row.primary)}
                          onclick={() => open(row.primary!)}
                        >
                          <Download />
                        </Button>
                        {#each row.secondary as entry (entry.kind)}
                          {@render otherLink(entry)}
                        {/each}
                      {/if}
                      {#each TOGGLES as set (set.id)}
                        {@render toggleButton(row.track, set)}
                      {/each}
                      {@render openButton(row.track)}
                    {/snippet}

                    {#snippet details()}
                      {#if isOpen(row.track.permalink)}
                        {@render rowDetails(row.track)}
                      {/if}
                    {/snippet}
                  </TrackRow>
                {/each}
              </ul>
            </section>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  <!-- The one line left of what was a whole footer: the list is the biggest
       thing in the backup, so this is where somebody looks for it, but the
       file carries four other tools' state and the sheet is where it is taken
       now. A button rather than a sentence, because it is one press away. -->
  {#snippet footer()}
    <div class="flex items-center gap-2">
      <span class="section-title mr-auto">saved state</span>
      <Button
        size="xs"
        variant="outline"
        title="in settings: everything the extension has saved, out to a file and back."
        onclick={() => (view.settingsOpen = true)}
      >
        <Archive />
        back up or restore…
      </Button>
    </div>
  {/snippet}
</PanelShell>
