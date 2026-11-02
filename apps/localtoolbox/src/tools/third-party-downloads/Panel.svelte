<!--
  Third-Party Downloader: takes the download links behind a playlist's tracks —
  Hypeddit, ToneDen, Droploud, Pump Your Sound, Laylo, Feature.fm, FanLink —
  walks each one as far as it goes, and saves what comes back.

  Where the links come from: the active tab, the session queue, or the drawer
  at the bottom, which is a paste box and nothing else — what a gate asks for
  is typed in the settings sheet instead (see $lib/downloader.svelte.ts for
  why an address is not one of this panel's options). A tab on one of those
  sites contributes its own link; a SoundCloud playlist or Likes tab
  contributes every link on it that one of the sites claims, asking the
  content script for the same download details the Manager tool lists; the
  queue contributes the links of the tracks picked out there, which is the way
  to work through a batch without the page it came from being open at all.

  Not every site ends in a file, and the panel is straight about which: a row
  that resolves to somewhere to go rather than something to save says so and
  offers to open it, instead of reporting a failure it did not have.

  A row that really did fail is not a dead end either. Most of these gates fail
  because they want something a fetch cannot give them — a connected account, a
  CAPTCHA, a step done on the page — so a failed row carries the next step
  rather than only the reason: Open takes you to the gate itself (or, if the
  walk got as far as the file, to the file), queueing the track on the way out
  so a link left half-done is still on a list somewhere once the popup has
  closed behind the new tab; a gate that failed for want of an email address
  opens the settings sheet, where that is typed, instead. Get stays, for the
  failures that are worth another go.

  One kind of "something a fetch cannot give them" the panel can give anyway: a
  sign-in. Hypeddit's Spotify step is enforced on Spotify's side, so the panel
  sends a tab out through the step's own authorize URL and waits for it to come
  back — a second, and no clicking, for anyone who has allowed the app once.
  The first time, Spotify has a login or a consent screen to put up, which a
  tab opened behind a popup cannot ask anybody to answer: the row says to press
  Connect on the gate once, and says it once for the whole run rather than
  waiting out the same trip on every row after it.

  Links that came from SoundCloud know their track, so a row says when that
  track is already marked as downloaded, or waiting in the queue, saving one
  marks it, and the two toggles above the list drop the rows in either state —
  leaving the links still to walk.

  The check button on such a row makes that mark by hand, and takes it off
  again. Plenty of tracks are had without this tool saving them — a row that
  only ever resolved to a page you went to yourself, a gate you finished in a
  tab, a file that was already on the disk — and the mark is what the rest of
  the extension goes by, so it has to be reachable from the row rather than
  only from a walk that worked.
-->
<script lang="ts">
  import { untrack } from 'svelte';
  import { fade, slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Download from '@lucide/svelte/icons/download';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import Check from '@lucide/svelte/icons/check';
  import MailCheck from '@lucide/svelte/icons/mail-check';
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import Plus from '@lucide/svelte/icons/plus';
  import Settings2 from '@lucide/svelte/icons/settings-2';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import HintSwitch from '$lib/components/HintSwitch.svelte';
  import PanelShell from '$lib/components/PanelShell.svelte';
  import StatusNote from '$lib/components/StatusNote.svelte';
  import SourcePicker from '$lib/components/SourcePicker.svelte';
  import { Button } from '$lib/components/ui/button/index.ts';
  import { Label } from '$lib/components/ui/label/index.ts';
  import { Textarea } from '$lib/components/ui/textarea/index.ts';
  import * as Collapsible from '$lib/components/ui/collapsible/index.ts';
  import * as Tooltip from '$lib/components/ui/tooltip/index.ts';
  import { activeTab, askPage, isSoundCloudUrl, openTab, roundTrip } from '$lib/chrome.ts';
  import { downloaded } from '$lib/downloaded.svelte.ts';
  import { downloader } from '$lib/downloader.svelte.ts';
  import { motion } from '$lib/motion.ts';
  import { HIDE_DOWNLOADED_PREF } from '$lib/filter.ts';
  import { queue } from '$lib/queue.svelte.ts';
  import { choicePref, flagPref } from '$lib/prefs.svelte.ts';
  import { remembered, view } from '$lib/view.svelte.ts';
  import { count, EXTRACTION_FAILED, RELOAD_THE_TAB } from '$lib/soundcloud.ts';
  import { PAGE, QUEUE, SOURCE_PREF, SOURCES } from '$lib/source.ts';
  import { toasts } from '$lib/toast.svelte.ts';
  import type { DetailedTrack, PageProgress, PanelProps, Track } from '$lib/types';
  import { extractGateUrls, labelOfUrl, siteFor, SITES, unwrapGate } from './sites/index.ts';
  import type { UnlockDetails } from './sites/index.ts';
  import { EMAIL, NAME, Wanting } from './next-step.ts';
  import { save } from './save.ts';
  import { walked, type Row } from './walked.svelte.ts';

  let { tool, onback }: PanelProps = $props();

  // The address a gate is given, the name beside it, and whether saving a file
  // marks its track: settings rather than this list's options, so they are set
  // in the settings sheet and shared from one place (see
  // $lib/downloader.svelte.js).
  const source = choicePref(SOURCE_PREF, PAGE, SOURCES);

  // "Hide downloaded" is the one the Cover Art and Manager tools have, under
  // the key they share; "Hide queued" is this list's own.
  const hideDownloaded = flagPref(HIDE_DOWNLOADED_PREF, false);

  const hideQueued = flagPref('pref:hide-queued', false);

  const SITE_NAMES = SITES.map((site) => site.name).join(', ');

  // What the line under the title says when the links came from a gate open in
  // the active tab rather than from a page listing several.
  const THIS_TAB = 'this tab';

  const HIDE_DOWNLOADED_HINT =
    'drops links whose track is already marked. pasted links have no track, and stay.';

  const HIDE_QUEUED_HINT = 'drops links whose track is sitting in the queue.';

  const OPEN_HINT = 'opens the gate in a tab so you can finish it yourself.';

  const MARK_HINT =
    'marks this one downloaded — for tracks this tool didn’t save itself.';

  const ADD_EMAIL_HINT = 'this gate wants an email. opens settings, then run the row again.';

  const ADD_NAME_HINT = 'this one wants a name too. opens settings, then run the row again.';

  // One row per link, in the order they were added, and where they were taken
  // from — the session's rather than this panel's, because a row's whole point
  // is the tab it sends you to, and opening a tab closes the popup. See
  // ./walked.svelte.js.
  const rows = $derived(walked.rows);

  const origin = $derived(walked.origin);

  let running = $state(false);

  let status = $state('');

  let loading = $state(true);

  // The drawer and the text in it are where someone was, not what they chose,
  // so they are kept for the session rather than for good: pasting a page of
  // links, closing the popup to deal with one of them and coming back should
  // not mean pasting them again. (The settings a gate asks for are the other
  // way about — a choice, kept for good, and set elsewhere entirely.)
  //
  // The drawer is remembered as three states rather than two: a panel with no
  // links to show opens it, since it is the only other place links come from,
  // but a drawer the user has closed since stays closed however often the
  // popup comes back to nothing.
  const here = untrack(() => tool.id);

  /** Null until it has been opened or closed. */
  const drawer: { value: boolean | null } = remembered(here, 'drawer', null);

  const drawerOpen = {
    get value() {
      return drawer.value === true;
    },
    set value(open) {
      drawer.value = open;
    },
  };

  const pasted = remembered(here, 'paste', '');

  // A plain Map on purpose: a lookup table the markup never reads, so making
  // it reactive would only cost a proxy on every write.
  const byDownloadId = new Map<number, Row>();

  const STATES = {
    queued: { text: '', tone: '' },
    working: { text: 'unlocking…', tone: 'text-fg-soft' },
    saving: { text: 'saving…', tone: 'text-fg-soft' },
    done: { text: 'saved', tone: 'text-good' },
    sent: { text: 'signed up', tone: 'text-good' },
    link: { text: '', tone: 'text-fg-muted' },
    failed: { text: 'failed', tone: 'text-critical' },
  };

  // A row is finished when there is nothing more this tool can do for it: it
  // has been saved, or it has resolved to a page only a person can go to.
  const settled = (row: Row) =>
    row.state === 'done' || row.state === 'link' || row.state === 'sent';

  // What the toggles leave. A row with no track of its own (a pasted link, the
  // gate in a tab) is in neither state, so it always stays.
  const visible = $derived(
    rows.filter(
      (row) =>
        !(hideDownloaded.value && downloaded.has(row.permalink)) &&
        !(hideQueued.value && queue.has(row.permalink))
    )
  );

  // "Download all" runs what is listed, so hiding is also how a batch is
  // chosen; the counts in the header are of everything collected.
  const left = $derived(visible.filter((row) => !settled(row)).length);

  const saved = $derived(rows.filter((row) => row.state === 'done').length);

  const links = $derived(rows.filter((row) => row.state === 'link').length);

  const failed = $derived(rows.filter((row) => row.state === 'failed').length);

  const meta = $derived(
    rows.length === 0
      ? ''
      : [
          count(rows.length, 'link'),
          ...(origin ? [origin] : []),
          ...(saved ? [`${saved} saved`] : []),
          ...(links ? [`${links} to open`] : []),
          ...(failed ? [`${failed} failed`] : []),
        ].join(' · ')
  );

  const TOGGLES = [
    {
      id: 'hide-downloaded',
      label: 'hide downloaded',
      hint: HIDE_DOWNLOADED_HINT,
      pref: hideDownloaded,
    },
    { id: 'hide-queued', label: 'hide queued', hint: HIDE_QUEUED_HINT, pref: hideQueued },
  ];

  // What a row that has not run yet says about the track behind it, which is
  // also what the toggles above go by.
  const waiting = (row: Row) => {
    if (downloaded.has(row.permalink)) return 'already marked';

    if (queue.has(row.permalink)) return 'in the queue';

    return '';
  };

  const hiddenLine = $derived.by(() => {
    if (visible.length === rows.length) return '';

    if (visible.length > 0) return `${visible.length} of ${count(rows.length, 'link')} shown`;

    return `all ${count(rows.length, 'link')} hidden.`;
  });

  // --- collecting links ------------------------------------------------------

  function addRows(
    urls: string[],
    {
      permalink = '',
      label = '',
      track = null,
      byHand = false,
    }: { permalink?: string; label?: string; track?: Track | null; byHand?: boolean } = {}
  ) {
    let added = 0;

    for (const url of urls) {
      if (walked.listed(url)) continue;
      const site = siteFor(url);

      if (!site) continue;
      // A link this session has walked already goes back on as the row it was
      // rather than as a fresh one; the store sees to that.
      walked.add({
        url,
        site,
        permalink,
        // The whole track, for the rows that have one: opening a failed gate
        // by hand queues it, which needs more than the permalink.
        track,
        label: label || labelOfUrl(url),
        // Pasted into the drawer, which is the one source that cannot be read
        // again — so the list puts those back itself.
        byHand,
        state: 'queued',
        detail: '',
        // Where a resolved row leads, once it has been run: the page it turned
        // out to point at, or — for one that failed — the thing to open to
        // carry on by hand.
        openUrl: '',
        // What a failed row offers to do about it: EMAIL, or 'open'.
        next: '',
      });
      added += 1;
    }

    return added;
  }

  // Every link on the SoundCloud page one of the sites claims, with the track
  // each one belongs to so saving it can mark that track.
  function gatesFromTracks(tracks: Track[]) {
    let added = 0;

    for (const track of tracks) {
      const url = unwrapGate((track.details && track.details.purchaseUrl) || '');
      added += addRows([url], {
        permalink: track.permalink,
        track,
        label: [track.artist, track.title].filter(Boolean).join(' — '),
      });
    }

    return added;
  }

  // The links of the tracks in the session queue.
  function collectFromQueue() {
    walked.origin = 'session queue';

    if (gatesFromTracks(queue.items) > 0) return;
    walked.origin = '';
    status =
      queue.count === 0
        ? 'nothing queued yet. queue tracks from Manager.'
        : `none of the ${count(queue.count, 'track')} in the queue point at a site this knows.`;
  }

  /**
   * The tracks the SoundCloud page lists, as the lookup fills their details
   * in. Every row this tool has comes out of a track's purchase link, so this
   * is what the list is built from — twice over: once from what the page
   * itself renders, and again from each answer as it arrives.
   */
  let listed: DetailedTrack[] = [];

  // Answers arriving from the lookup, a few at a time. `addRows` skips a link
  // it has already listed, so building the rows again as each batch lands adds
  // only what is new.
  try {
    chrome.runtime.onMessage.addListener((message: PageProgress) => {
      if (!message || message.type !== 'DOWNLOAD_DETAILS_PROGRESS') return;

      if (!message.details?.length || listed.length === 0) return;
      const learned = new Map(message.details.map((details) => [details.permalink, details]));
      let filled = false;
      listed = listed.map((track) => {
        const details = learned.get(track.permalink);

        if (!details || track.details) return track;
        filled = true;

        return { ...track, details };
      });

      if (filled) gatesFromTracks(listed);
    });
  } catch (_e) {
    // No runtime to listen on: every row arrives with the answer instead.
  }

  /** Only ever handed a tab whose URL could be read (see collect). */
  async function collectFromTab(tab: chrome.tabs.Tab) {
    if (siteFor(tab.url!)) {
      walked.origin = THIS_TAB;
      addRows([tab.url!]);

      return;
    }

    if (!isSoundCloudUrl(tab.url)) return;

    // The details are the slow half of this — the content script asks
    // SoundCloud about every track the page shows, which on a scrolled page is
    // seconds — and they are started first so they run underneath everything
    // below rather than after it.
    const answer = askPage(tab.id!, 'GET_DOWNLOAD_DETAILS');

    // The page's own tracks come back in a few milliseconds, and on Likes
    // pages they carry the purchase links the page itself renders: those rows
    // are listed before SoundCloud has been asked anything at all, and the
    // rest arrive as the answers do.
    const preview = await askPage(tab.id!, 'EXTRACT_TRACKS');

    if (preview.reached && preview.response?.ok && Array.isArray(preview.response.tracks)) {
      listed = preview.response.tracks;
      walked.origin = preview.response.title || 'SoundCloud';
      gatesFromTracks(listed);
    }

    const { reached, response } = await answer;

    if (!reached || !response) {
      status = RELOAD_THE_TAB;

      return;
    }

    // Not just `ok`: a content script left in the tab from before an update
    // answers in the shape it knows, which need not carry tracks at all.
    if (!response.ok || !Array.isArray(response.tracks)) {
      status = EXTRACTION_FAILED;

      return;
    }

    walked.origin = response.title || 'SoundCloud';
    listed = response.tracks;
    gatesFromTracks(listed);

    // What the whole list came to, rather than what this last pass added:
    // rows put up from the page itself, or as the answers came in, are rows.
    if (rows.length === 0) {
      status = `nothing on ${origin} points at a site this knows (${SITE_NAMES}).`;
      walked.origin = '';
    }
  }

  /**
   * Builds the list from one source or the other.
   *
   * Switching the source is a choice, and a choice empties the list first: what
   * is listed is then always what the chosen source holds. (What became of each
   * row is not forgotten — a link the other source offers too comes back as the
   * row it was.)
   *
   * The popup coming back is not a choice anybody made, so `keep` is how it
   * collects: the list is the one that was there, and the source is read again
   * only for what is new to it. It matters most when the tab in front is the
   * gate a row has just sent you to — a link this tool claims, and one that
   * would otherwise be the whole list.
   */
  async function collect({ keep = false } = {}) {
    loading = true;
    status = '';
    const held = keep ? walked.origin : '';

    if (!keep) walked.clear();
    walked.origin = '';

    if (source.value === QUEUE) {
      collectFromQueue();
    } else {
      const tab = await activeTab();

      if (tab && tab.url) await collectFromTab(tab);
    }

    // Links pasted into the drawer belong to no source, so nothing would offer
    // them again: they go back on after whatever the source had.
    addRows(walked.byHand, { byHand: true });

    // A list that is still showing something is not a list to complain about:
    // whatever the source had to say about finding nothing is not about it.
    if (rows.length > 0) status = '';

    // Nor is it renamed by a source that did not name itself, or by the one
    // loose gate in the active tab — a link appended to twelve does not say
    // where the twelve came from.
    if (held && (walked.origin === '' || walked.origin === THIS_TAB)) walked.origin = held;

    if (rows.length === 0) {
      // The drawer is the only other place a link can come from, so a panel
      // with nothing to show opens it — unless it has been closed since.
      if (drawer.value === null) drawer.value = true;

      if (!status) {
        status =
          'open a gate link or a SoundCloud page in this tab, or paste links below. it walks ' +
          `${SITE_NAMES}.`;
      }
    }

    loading = false;
  }

  walked.start().then(() => collect({ keep: true }));

  function onAdd() {
    const urls = extractGateUrls(pasted.value);

    if (urls.length === 0) {
      toasts.bad('no links this tool knows in that text');

      return;
    }

    const added = addRows(urls, { byHand: true });
    pasted.value = '';

    if (added === 0) {
      toasts.info(`already listed: ${urls.length === 1 ? 'that link' : 'all of those'}`);

      return;
    }

    status = '';
    toasts.ok(`added ${count(added, 'link')}`);
  }

  // --- running ---------------------------------------------------------------

  async function run(pending: Row[]) {
    const todo = pending.filter((row) => !settled(row));

    if (running || todo.length === 0) return;

    running = true;
    // A trip that needed a person needed one for the account, not for the gate
    // — so the next twenty rows would wait out the same timeout for the same
    // answer. The first no is taken as the answer for the rest of this run;
    // pressing Get again after connecting asks afresh.
    let connectable = true;

    const details: UnlockDetails = {
      email: downloader.email.value.trim(),
      name: downloader.name.value.trim(),
      // A sign-in an adapter cannot fetch its way through. It is handed over
      // rather than reached for, so the adapters stay things that only talk to
      // their site: what comes back is a yes or a no, and a no is a sentence
      // the row shows like any other.
      connect: async (url, options) => {
        if (!connectable) return false;
        connectable = await roundTrip(url, options);

        return connectable;
      },
    };

    // One at a time: a gate is a handful of round trips and several of these
    // sites hand out a per-visit token, so there is nothing to gain from
    // overlapping them — and every one of them is somebody's small server.
    for (const row of todo) {
      // However the row ends — saved, somewhere to open, failed — the list is
      // written back with it, so the popup closing on the tab it sends you to
      // does not take the answer with it.
      try {
        row.state = 'working';
        row.detail = '';
        row.next = '';
        row.openUrl = '';
        const result = await row.site.unlock(row.url, details);

        if (result.title) row.label = result.title;

        if (result.kind !== 'file') {
          // A signup is not a file and not a page left to visit: the address
          // has been left with the artist, and what comes back comes by mail.
          row.state = result.kind === 'signup' ? 'sent' : 'link';
          row.openUrl = result.url;
          row.detail = result.why;
          continue;
        }

        row.state = 'saving';
        // Past this point the gate has been walked and the file is known, so
        // anything that goes wrong is about the transfer: the next step is the
        // file itself rather than another trip through the gate.
        row.openUrl = result.url;

        const { id, name: filename, renamed } = await save(result.url, {
          subfolder: row.site.subfolder,
          filename: result.filename,
        });

        byDownloadId.set(id, row);

        if (filename) row.label = filename;

        // Chrome would not take the name the file came with, so it was saved
        // without one. It is in the download folder rather than the site's,
        // under whatever Chrome called it — worth a line, not a failure.
        if (renamed) row.detail = 'saved, but Chrome renamed it — check your downloads folder';
        // The transfer is the browser's from here, and outlives this popup, so
        // the row calls it saved as soon as it starts; onDownloadChanged below
        // corrects that if it does not finish while the panel is open.
        row.state = 'done';

        if (row.permalink && downloader.mark.value) await downloaded.set(row.permalink, true);
      } catch (error) {
        row.state = 'failed';
        row.detail = error instanceof Error ? error.message : String(error);
        // A gate that wants an email or a name wants it here, not in a tab;
        // everything else is a trip to the page — to the file, if the walk got
        // that far.
        row.next = error instanceof Wanting ? error.next : 'open';

        if (!row.openUrl) row.openUrl = row.url;
      } finally {
        walked.save();
      }
    }

    running = false;
    const files = todo.filter((row) => row.state === 'done').length;
    const pages = todo.filter((row) => row.state === 'link').length;
    const signups = todo.filter((row) => row.state === 'sent').length;
    const rest = todo.length - files - pages - signups;

    // What a run is worth saying is what it did: files saved, addresses left
    // with an artist, pages that are still yours to open.
    const also = [
      signups > 0 ? `${signups} signed up for` : '',
      pages > 0 ? `${pages} to open by hand` : '',
    ].filter(Boolean);

    toasts.said(
      rest === 0,
      [`saved ${count(files, 'file')}`, ...also].join(', '),
      `saved ${files} of ${todo.length} — check the rows that failed`
    );
  }

  // Carrying on by hand, which is how most of these failures end. The track
  // goes into the session queue first, the way the Manager tool opens a gate:
  // the popup closes as the tab opens, and a link left half-done is then still
  // on a list rather than only in a panel nobody is looking at.
  async function openByHand(row: Row) {
    if (row.track) await queue.add([row.track]);
    await openTab(row.openUrl || row.url);
  }

  // A transfer this panel started has ended. Downloads from an earlier opening
  // of the panel are not listed any more, so unknown ids are normal.
  try {
    chrome.downloads.onChanged.addListener((delta) => {
      const row = byDownloadId.get(delta.id);

      if (!row || !delta.state) return;

      if (delta.state.current === 'interrupted') {
        row.state = 'failed';
        row.detail = 'the transfer was interrupted.';
        // `openUrl` is the file the gate handed over, so Open is another go at
        // the transfer itself rather than a walk back through the gate.
        row.next = 'open';
        walked.save();
      }
    });
  } catch (_e) {
    // No downloads API to follow: a row stays "Saved" either way.
  }
</script>

<!-- The Downloaded mark, on the rows that have a track to put it on: the same
     button the Manager and Session Queue panels carry, so it looks and reads
     the same wherever the mark is made. -->
{#snippet markButton(row: Row)}
  {@const on = downloaded.has(row.permalink)}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Button
          {...props}
          size="icon-sm"
          variant={on ? 'default' : 'outline'}
          class={on
            ? 'bg-good text-fg-inverse hover:bg-good/85'
            : 'text-fg-muted'}
          aria-pressed={on}
          aria-label="mark downloaded"
          onclick={() => downloaded.toggle(row.permalink)}
        >
          <Check />
        </Button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content class="max-w-64">{MARK_HINT}</Tooltip.Content>
  </Tooltip.Root>
{/snippet}

<PanelShell {tool} {onback} {meta}>
  {#snippet actions()}
    {#if visible.length > 0}
      <Button
        size="sm"
        disabled={running || left === 0}
        title="walks every link and saves the files"
        onclick={() => run(visible)}
      >
        {#if running}
          <LoaderCircle class="animate-spin" />downloading…
        {:else if left === 0}
          <Check />all done
        {:else}
          <Download />get all ({left})
        {/if}
      </Button>
    {/if}
  {/snippet}

  <div class="flex flex-col gap-2.5">
    <SourcePicker {source} queued={queue.count} onchange={() => collect()} />

    {#if rows.length > 0}
      <div class="flex flex-col gap-1.5">
        <div class="flex items-center gap-4">
          {#each TOGGLES as toggle (toggle.id)}
            <HintSwitch
              id={toggle.id}
              label={toggle.label}
              hint={toggle.hint}
              bind:checked={toggle.pref.value}
            />
          {/each}
        </div>
        {#if hiddenLine}
          <p class="tabular text-caption text-fg-muted">{hiddenLine}</p>
        {/if}
      </div>
    {/if}

    {#if loading}
      <StatusNote tone="busy">looking for download links…</StatusNote>
    {:else if status}
      <StatusNote tone="info">{status}</StatusNote>
    {/if}

    {#if visible.length > 0}
      <ul class="flex flex-col gap-1.5">
        {#each visible as row (row.url)}
          {@const state = STATES[row.state]}
          {@const fault = row.state === 'failed' ? row.detail : ''}
          {@const line = fault
            ? ''
            : row.detail || (row.state === 'queued' ? waiting(row) : state.text)}
          <li
            class="overflow-hidden rounded-lg border transition-colors
                   {row.state === 'done' || row.state === 'sent'
              ? 'border-good/40 bg-good/5'
              : row.state === 'failed'
                ? 'border-critical/40 bg-surface'
                : row.state === 'queued' || row.state === 'link'
                  ? 'border-line bg-surface'
                  : 'border-line-strong bg-surface'}"
          >
            <div class="flex items-center gap-2.5 px-2.5 py-2">
              <div class="min-w-0 flex-1">
                <p class="truncate text-label font-semibold" title={row.url}>{row.label}</p>
                <p class="flex items-start gap-1 text-caption {state.tone}">
                  <span
                    class="shrink-0 rounded-full border border-line-strong px-1.5 font-mono
                           text-[10px] leading-4 text-fg-muted"
                  >
                    {row.site.name}
                  </span>
                  {#if row.state === 'working' || row.state === 'saving'}
                    <LoaderCircle class="mt-0.5 size-3 shrink-0 animate-spin" />
                  {:else if row.state === 'done'}
                    <Check class="mt-0.5 size-3 shrink-0" />
                  {:else if row.state === 'sent'}
                    <MailCheck class="mt-0.5 size-3 shrink-0" />
                  {:else if row.state === 'failed'}
                    <CircleAlert class="mt-0.5 size-3 shrink-0" />
                  {/if}
                  {#if line}<span class="break-words">{line}</span>{/if}
                </p>
              </div>
              <div class="flex shrink-0 items-center gap-1.5">
                <!--
                  The mark comes first on every row that has a track, whatever
                  state the walk left it in: a row that ended on a page, one that
                  failed and was finished in a tab, one that was never worth
                  running because the file was already on the disk — all of them
                  are marked the same way, and a mark made in error is taken off
                  the same way.
                -->
                {#if row.permalink}{@render markButton(row)}{/if}
                {#if row.state === 'link' || row.state === 'sent'}
                  <Button
                    size="sm"
                    variant="outline"
                    title={`opens ${row.openUrl}`}
                    onclick={() => openTab(row.openUrl)}
                  >
                    <ExternalLink />
                    open
                  </Button>
                {:else}
                  <!--
                    A failed row leads with what will actually get the track: the
                    gate itself, or the settings sheet when what is missing is
                    something typed there — the email address, the name beside
                    it. Get is still there beside it, for the failures worth
                    another go on their own.
                  -->
                  {#if row.state === 'failed' && (row.next === EMAIL || row.next === NAME)}
                    <Button
                      size="sm"
                      title={row.next === NAME ? ADD_NAME_HINT : ADD_EMAIL_HINT}
                      onclick={() => (view.settingsOpen = true)}
                    >
                      <Settings2 />
                      add {row.next === NAME ? 'name' : 'email'}
                    </Button>
                  {:else if row.state === 'failed'}
                    <Button size="sm" title={OPEN_HINT} onclick={() => openByHand(row)}>
                      <ExternalLink />
                      open
                    </Button>
                  {/if}
                  <Button
                    size="sm"
                    variant={row.state === 'done' || row.state === 'failed' ? 'outline' : 'default'}
                    disabled={running || row.state === 'done'}
                    title="walks this link and saves the file"
                    onclick={() => run([row])}
                  >
                    get
                  </Button>
                {/if}
              </div>
            </div>

            <!--
              What went wrong gets the whole width of the card rather than the
              sliver of column left beside the buttons. These messages carry a
              URL, or a gate's own words, and one that has to wrap five times
              down a third of the row is one nobody reads — so the card opens
              underneath itself and says it in full. It opens with the failure
              and closes with it, which is what makes a retry that works look
              like the card healing over.
            -->
            {#if fault}
              <p
                transition:slide={motion({ duration: 240, easing: cubicOut })}
                class="border-t border-critical/40 bg-critical/5 px-2.5 py-2 text-caption
                       leading-relaxed text-critical"
              >
                <span class="min-w-0 break-words" in:fade={motion({ duration: 200, delay: 110 })}>
                  {fault}
                </span>
              </p>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}

    <Collapsible.Root bind:open={drawerOpen.value}>
      <Collapsible.Trigger
        class="flex w-full items-center gap-1.5 rounded-md px-1 py-1 text-caption text-fg-muted transition-colors hover:text-fg"
      >
        <Plus class="size-3.5" />
        <span>add links by hand</span>
        <ChevronRight
          class="ml-auto size-3.5 transition-transform duration-150 {drawerOpen.value
            ? 'rotate-90'
            : ''}"
        />
      </Collapsible.Trigger>

      <Collapsible.Content>
        <div class="mt-1.5 flex flex-col gap-2.5 rounded-lg border border-line bg-surface p-2.5">
          <div class="flex flex-col gap-1.5">
            <Label for="paste" class="text-caption text-fg-muted">
              paste download links
            </Label>
            <Textarea
              id="paste"
              rows={2}
              spellcheck="false"
              placeholder="https://hypeddit.com/…"
              class="text-caption"
              bind:value={pasted.value}
            />
            <Button size="xs" variant="outline" class="self-start" onclick={onAdd}>
              <Plus />
              add links
            </Button>
            <p class="text-caption leading-relaxed text-fg-muted">
              anything pointing at {SITE_NAMES} is picked out.
            </p>
          </div>
        </div>
      </Collapsible.Content>
    </Collapsible.Root>
  </div>
</PanelShell>
