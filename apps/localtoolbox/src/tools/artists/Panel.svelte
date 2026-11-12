<!--
  artists: every uploader behind the tracks somebody has added, and where else
  to find them.

  The list is the tool's own and is kept for good (./artists.svelte.ts), the
  way the Manager's is: nothing goes onto it by itself, and the bar at the top
  is the one place artists come in: from the tab in front, from the Manager's
  list, or from the session queue. Adding puts the artists on the list at once,
  from what the source itself says (a name and the tracks), and then fills in
  what the SoundCloud tab in front has loaded about each account: the public
  profile behind the path, and the links the uploader put on it. A track names
  its uploader in short; the bio and the links come with the profile page.

  A row is the artist's picture, name and a line about them, and then their
  socials as lettered badges, one per link, each opening that page. The
  chevron opens the row into everything else: every link as an address to
  copy, the bio, the place, the follower count, the tracks that brought the
  artist here, and the search badges from Settings that look for the
  artist on the shops the Manager searches.

  The socials are read from a SoundCloud tab (./lookup.ts), any page of it,
  each time the panel opens there; the list itself is on screen on any tab at
  all, with whatever it already knows. Profiles last the browser session
  (./artists.svelte.ts).
-->
<script lang="ts">
  import { untrack } from 'svelte';
  import BadgeCheck from '@lucide/svelte/icons/badge-check';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import Copy from '@lucide/svelte/icons/copy';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import Search from '@lucide/svelte/icons/search';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import User from '@lucide/svelte/icons/user';
  import X from '@lucide/svelte/icons/x';
  import Badge from '$lib/components/Badge.svelte';
  import HintSwitch from '$lib/components/HintSwitch.svelte';
  import PanelShell from '$lib/components/PanelShell.svelte';
  import StatusNote from '$lib/components/StatusNote.svelte';
  import AddBar from './AddBar.svelte';
  import { Button } from '$lib/components/ui/button/index.ts';
  import { Input } from '$lib/components/ui/input/index.ts';
  import * as Select from '$lib/components/ui/select/index.ts';
  import * as Tooltip from '$lib/components/ui/tooltip/index.ts';
  import { BACKGROUND_HINT, openTab } from '$lib/chrome.ts';
  import { copyText } from '$lib/clipboard.ts';
  import { matches } from '$lib/filter.ts';
  import { createPainting } from '$lib/painting.svelte.ts';
  import { choicePref, flagPref } from '$lib/prefs.svelte.ts';
  import { queue } from '$lib/queue.svelte.ts';
  import { queryOf, searchable, searchUrl } from '$lib/search-sites.ts';
  import { searchSites } from '$lib/search-sites.svelte.ts';
  import { count, OPEN_A_PAGE, readPage } from '$lib/soundcloud.ts';
  import { toasts } from '$lib/toast.svelte.ts';
  import { remembered } from '$lib/view.svelte.ts';
  import { listed } from '../manager/listed.svelte.ts';
  import { artists, MAX } from './artists.svelte.ts';
  import { lookupProfiles, onSoundCloud, OPEN_A_TAB } from './lookup.ts';
  import {
    artistsOf,
    asText,
    hasProfile,
    isIncomplete,
    isUnavailable,
    isUnchecked,
    linksOf,
    LOAD_HINT,
    nameOf,
    profileLine,
    ROW_BADGES,
    rowFacts,
    searchText,
    SORT_VALUES,
    SORTS,
    sorted,
  } from './profiles.ts';
  import type { Fact, Link } from './profiles.ts';
  import type { Artist, ArtistEntry, PageTrack, PageType, PanelProps } from '$lib/types';

  let { tool, onback }: PanelProps = $props();

  const SORT_HINT = 'by name sorts the list. list order keeps the order you added them.';

  const OPEN_HINT = 'everything known about this artist. click a field to copy it.';

  const FACT_HINT = 'click to copy';

  const LINK_HINT = 'click to copy the address. the arrow opens it in a tab.';

  const COPY_HINT = 'copies every artist shown — name, profile and links — as text.';

  const NO_PAGE = 'not a SoundCloud playlist, likes or track page. open one to add its artists.';

  const PAGE_KINDS: Partial<Record<PageType, string>> = {
    likes: 'a likes page',
    track: 'a track page',
    playlist: 'a playlist',
  };

  const LIST_SOURCE = 'the Manager’s list';

  const QUEUE_SOURCE = 'the session queue';

  const here = untrack(() => tool.id);

  const sort = choicePref('pref:artists-sort', 'name', SORT_VALUES);

  const background = flagPref('pref:background', true);

  const query = remembered(here, 'query', '');

  // --- the list --------------------------------------------------------------

  let ready = $state(false);

  const started = artists.start().then(() => (ready = true));

  // The Manager's list is a source, and read for the count on its button.
  listed.start();

  // Whether the tab in front can be asked: any SoundCloud page will do, since
  // the content script is in every one of them. Read once, as the page is,
  // and then the list is filled in from it.
  let onSc = $state(false);

  const fillOnOpen = async () => {
    onSc = await onSoundCloud();
    await started;
    await fillIn();
  };

  void fillOnOpen();

  // --- what there is to add --------------------------------------------------

  /** The tab in front, as a source to add from. */
  interface PageSource {
    reading: boolean;
    name: string;
    from: string;
    curator: string;
    tracks: PageTrack[];
    reason: string;
  }

  let page: PageSource = $state({ reading: true, name: '', from: '', curator: '', tracks: [], reason: '' });

  async function readTab() {
    const result = await readPage('EXTRACT_TRACKS');

    if (result.status) {
      page = {
        reading: false,
        name: '',
        from: '',
        curator: '',
        tracks: [],
        reason: result.status === OPEN_A_PAGE ? NO_PAGE : result.status,
      };

      return;
    }

    const { title, curator, pageType, tracks: found } = result.data!;
    page = {
      reading: false,
      name: title || 'this tab',
      from: title || PAGE_KINDS[pageType] || 'a playlist',
      curator,
      tracks: found,
      reason: '',
    };
  }

  void readTab();

  const tabArtists = $derived(artistsOf(page.tracks));

  const listArtists = $derived(artistsOf(listed.items));

  const queueArtists = $derived(artistsOf(queue.items));

  const freshOf = (list: Artist[]) =>
    list.filter((artist) => !artists.has(artist.permalink)).length;

  const pageFresh = $derived(freshOf(tabArtists));

  const pageHere = $derived(tabArtists.length - pageFresh);

  const listFresh = $derived(freshOf(listArtists));

  const queueFresh = $derived(freshOf(queueArtists));

  const offer = $derived({
    reading: page.reading,
    name: page.name,
    fresh: pageFresh,
    here: pageHere,
    reason: page.reason,
    meta: [
      page.curator ? `by ${page.curator}` : '',
      count(tabArtists.length, 'artist'),
      `${count(page.tracks.length, 'track')}`,
      pageHere > 0 ? `${pageHere} already here` : '',
    ]
      .filter(Boolean)
      .join(' · '),
  });

  // --- adding, and filling in ------------------------------------------------

  let adding = $state(false);

  /** What the last add or fill-in came to, when it is worth leaving on screen. */
  let note = $state<{ tone: 'bad' | 'info'; text: string } | null>(null);

  /**
   * Fills in every artist still missing a profile, or its bio and links, from
   * what the SoundCloud tab in front has loaded. The tab answers from memory,
   * so this costs nothing and runs whenever it might find something: as the
   * panel opens, and after an add.
   */
  async function fillIn() {
    const wanted = artists.items.filter(isIncomplete).map((entry) => entry.permalink);

    if (!onSc || wanted.length === 0) return;
    const result = await lookupProfiles(wanted);

    if (result.status) {
      note = { tone: 'bad', text: `couldn’t read their socials. ${result.status}` };

      return;
    }

    artists.learn(result.data!.profiles);
    await artists.flush();
  }

  const said = ({ added, refreshed }: { added: number; refreshed: number }, from: string) => {
    const parts = [];

    if (added > 0) parts.push(`added ${count(added, 'artist')}`);

    if (refreshed > 0) parts.push(`refreshed ${count(refreshed, 'artist')}`);

    if (parts.length === 0) parts.push('nothing to add');

    return `${parts.join(', ')} from ${from}`;
  };

  const refusal = (tally: { refused: number }) =>
    tally.refused > 0
      ? `${count(tally.refused, 'artist')} wouldn’t fit: the list holds ${MAX}. ` +
        'take some off and add again.'
      : '';

  async function addFrom(source: 'tab' | 'list' | 'queue') {
    adding = true;
    note = null;

    const [list, from] =
      source === 'tab'
        ? [tabArtists, page.from]
        : source === 'list'
          ? [listArtists, LIST_SOURCE]
          : [queueArtists, QUEUE_SOURCE];

    const tally = await artists.add(list, { from });
    adding = false;
    const extra = refusal(tally);

    if (extra) note = { tone: 'info', text: `${said(tally, from)}. ${extra}` };
    else toasts.ok(said(tally, from));

    // Then the socials, from the tab in front where it is SoundCloud. Without
    // one the rows say how to get them.
    await fillIn();
  }

  const unchecked = $derived(artists.items.filter(isUnchecked));

  const incomplete = $derived(artists.items.filter(isIncomplete));

  // --- the rows --------------------------------------------------------------

  const ordered = $derived(sorted(artists.items, sort.value));

  const visible = $derived(ordered.filter((entry) => matches(searchText(entry), query.value)));

  // What has been drawn of it so far: a long list, or a search being deleted,
  // goes up a frame's worth of rows at a time ($lib/painting.svelte.ts).
  const drawing = createPainting(() => visible.map((entry) => entry.permalink));

  const painted = $derived(visible.filter((entry) => drawing.has(entry.permalink)));

  const withSocials = $derived(
    artists.items.filter((entry) => hasProfile(entry) && linksOf(entry.profile).length > 0).length
  );

  const meta = $derived(
    artists.count === 0
      ? ''
      : [
          count(artists.count, 'artist'),
          `${withSocials} with socials`,
          ...(unchecked.length > 0 ? [`${unchecked.length} not loaded yet`] : []),
        ].join(' · ')
  );

  const countLine = $derived.by(() => {
    if (visible.length === ordered.length) return '';

    if (visible.length > 0) return `${visible.length} of ${ordered.length} shown`;

    return `nothing matches “${query.value.trim()}”.`;
  });

  // --- opening a row ---------------------------------------------------------

  /** The paths of the rows that are open. */
  const opened: { value: string[] } = remembered(here, 'opened', []);

  const isOpen = (permalink: string) => opened.value.includes(permalink);

  function toggleOpen(permalink: string) {
    opened.value = isOpen(permalink)
      ? opened.value.filter((one) => one !== permalink)
      : [...opened.value, permalink];
  }

  async function onCopyFact(fact: Fact) {
    const copied = await copyText(fact.copy);
    toasts.said(copied, `copied the ${fact.label}`, `couldn’t copy the ${fact.label}`);
  }

  const openLink = (url: string) => openTab(url, { background: background.value });

  async function onRemoveRow(entry: ArtistEntry) {
    await artists.removeMany([entry.permalink]);
    toasts.ok(`took ${nameOf(entry)} off the list`);
  }

  async function onCopyList() {
    const copied = await copyText(asText(visible));
    toasts.said(
      copied,
      `copied ${count(visible.length, 'artist')}`,
      'couldn’t copy the list'
    );
  }

  // The search badges from settings, for the opened row: the same sites the
  // Manager's link-less rows search, given the artist's name.
  const badgeSites = $derived(searchSites.enabled);
</script>

<!-- One link as a badge: the network's monogram in its colour, opening the
     page. The address is the tooltip, so a badge says where it goes before it
     is pressed. -->
{#snippet linkBadge(link: Link)}
  <Badge
    badge={link.badge}
    action={`open ${link.name}`}
    style={link.style}
    title={link.title ? `${link.title} · ${link.url}` : link.url}
    onclick={() => openLink(link.url)}
  />
{/snippet}

<!-- What a row opens into: every field of the artist, each a button that
     copies it; an address gets an arrow of its own. Then the search badges,
     the way to ask again, and the way off the list. -->
{#snippet rowDetails(entry: ArtistEntry)}
  {@const facts = rowFacts(entry)}
  {@const name = nameOf(entry)}
  <dl
    class="mt-1.5 flex flex-col gap-px border-t border-line pt-1.5 text-caption"
    aria-label={`about ${name}`}
  >
    {#each facts as fact (fact.id)}
      <div class="flex items-baseline gap-2">
        <dt class="w-16 shrink-0 truncate font-mono text-[10.5px] text-fg-subtle" title={fact.label}>
          {fact.label}
        </dt>
        <dd class="flex min-w-0 flex-1 items-baseline gap-1">
          <button
            type="button"
            class="min-w-0 flex-1 rounded-xs px-1 py-0.5 text-left
                   enabled:cursor-pointer enabled:hover:bg-raised
                   disabled:text-fg-subtle disabled:italic
                   {fact.id === 'bio' ? 'line-clamp-4 whitespace-pre-line' : 'truncate'}"
            disabled={!fact.copy}
            aria-label={fact.copy ? undefined : `${fact.label}: ${fact.value}`}
            title={fact.copy ? (fact.open ? LINK_HINT : FACT_HINT) : ''}
            onclick={() => onCopyFact(fact)}
          >
            {fact.value}
          </button>
          {#if fact.open}
            <Button
              size="icon-xs"
              variant="ghost"
              class="shrink-0 text-fg-muted hover:text-fg"
              aria-label={`open ${fact.label} in a new tab`}
              title="opens in a tab."
              onclick={() => openLink(fact.open)}
            >
              <ExternalLink />
            </Button>
          {/if}
        </dd>
      </div>
    {/each}
  </dl>
  <div class="mt-1 flex flex-wrap items-center gap-1">
    {#each badgeSites as site (site.id)}
      {@const track = { artist: name, title: '' }}
      {@const search = queryOf(site, track)}
      <Badge
        badge={site.badge}
        action={`search ${site.name}`}
        style={site.style}
        title={`search ${site.name} for “${search}”`}
        disabled={!searchable(site, track)}
        onclick={() => openLink(searchUrl(site, track))}
      />
    {/each}
    <div class="ml-auto flex items-center gap-1">
      <Button
        size="xs"
        variant="ghost"
        class="text-fg-muted hover:text-critical"
        aria-label={`take ${name} off the list`}
        title="off the list. the Manager’s tracks stay."
        onclick={() => onRemoveRow(entry)}
      >
        <Trash2 />
        off the list
      </Button>
    </div>
  </div>
{/snippet}

<PanelShell {tool} {onback} {meta}>
  {#snippet actions()}
    <!-- The list as text, for a message or a spreadsheet: every artist the
         search leaves showing, with their profile and every link. -->
    <Button
      size="sm"
      variant="outline"
      disabled={visible.length === 0}
      aria-label={`copy ${count(visible.length, 'artist')} as text`}
      title={COPY_HINT}
      onclick={onCopyList}
    >
      <Copy />
      copy
    </Button>
  {/snippet}

  {#if !ready}
    <StatusNote tone="busy">reading your list…</StatusNote>
  {:else}
    <div class="flex flex-col gap-2.5">
      <AddBar
        {tool}
        {offer}
        {adding}
        listed={listArtists.length}
        listedFresh={listFresh}
        queued={queueArtists.length}
        queuedFresh={queueFresh}
        big={artists.count === 0}
        onadd={addFrom}
      />

      {#if note}
        <StatusNote tone={note.tone}>{note.text}</StatusNote>
      {/if}

      {#if artists.count > 0}
        {#if incomplete.length > 0}
          <!-- The artists whose socials no SoundCloud tab has loaded yet, and
               how to get them: a track only names its uploader, and the bio
               and the links come with the profile page. -->
          <p
            class="rounded-lg border border-line bg-surface px-3 py-2 text-caption leading-relaxed text-fg-soft"
            role="status"
          >
            {count(incomplete.length, 'artist')} without socials yet. {onSc ? LOAD_HINT : OPEN_A_TAB}
          </p>
        {/if}

        <div class="flex flex-col gap-1.5">
          <div class="relative min-w-0 flex-1">
            <Search
              class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-fg-muted"
            />
            <Input
              type="search"
              class="h-8 pr-7 pl-8 text-label [&::-webkit-search-cancel-button]:hidden"
              placeholder="search name, place, bio or link"
              autocomplete="off"
              spellcheck="false"
              aria-label="search the artists"
              bind:value={query.value}
            />
            {#if query.value}
              <button
                type="button"
                class="absolute top-1/2 right-1.5 grid size-5 -translate-y-1/2 place-items-center rounded-md text-fg-muted transition-colors hover:bg-well hover:text-fg"
                aria-label="clear the search"
                onclick={() => (query.value = '')}
              >
                <X class="size-3" />
              </button>
            {/if}
          </div>
          {#if countLine}
            <p class="tabular text-caption text-fg-muted">{countLine}</p>
          {/if}
        </div>

        <div class="flex items-center gap-2.5">
          <!-- The hint is a tooltip for the pointer and the trigger's description
               for a screen reader. The wrapper is the tooltip's trigger rather
               than the select's own button, since a tooltip opened by focus would
               sit over the open list; -1 keeps it out of the tab order. -->
          <Tooltip.Root>
            <Tooltip.Trigger tabindex={-1}>
              {#snippet child({ props })}
                <div {...props} class="min-w-0 flex-1">
                  <Select.Root type="single" bind:value={sort.value}>
                    <Select.Trigger
                      class="h-8 w-full text-label"
                      aria-label="sort order"
                      aria-describedby={`${here}-sort-hint`}
                    >
                      {SORTS.find((s) => s.value === sort.value)?.label}
                    </Select.Trigger>
                    <Select.Content>
                      {#each SORTS as option (option.value)}
                        <Select.Item value={option.value} label={option.label} />
                      {/each}
                    </Select.Content>
                  </Select.Root>
                  <span id={`${here}-sort-hint`} class="sr-only">{SORT_HINT}</span>
                </div>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content class="max-w-72">{SORT_HINT}</Tooltip.Content>
          </Tooltip.Root>

          <HintSwitch
            id="background"
            label="background tabs"
            hint={BACKGROUND_HINT}
            bind:checked={background.value}
          />
        </div>

        <ul class="flex flex-col gap-1.5">
          {#each painted as entry (entry.permalink)}
            {@const name = nameOf(entry)}
            {@const links = linksOf(entry.profile)}
            {@const avatar = hasProfile(entry) ? entry.profile.avatarUrl : ''}
            <li
              data-permalink={entry.permalink}
              class="rounded-lg border border-line bg-surface px-2 py-1.5 transition-colors hover:border-line-strong"
            >
              <div class="flex items-center gap-2.5">
                {#if avatar}
                  <img
                    src={avatar}
                    alt=""
                    loading="lazy"
                    class="size-10 shrink-0 rounded-full bg-well object-cover ring-1 ring-line"
                  />
                {:else}
                  <div
                    class="grid size-10 shrink-0 place-items-center rounded-full bg-well text-fg-subtle ring-1 ring-line"
                    title="no picture"
                  >
                    <User class="size-4 opacity-60" />
                  </div>
                {/if}

                <div class="min-w-0 flex-1">
                  <p class="flex items-center gap-1 truncate text-label font-semibold" title={name}>
                    <span class="truncate">{name}</span>
                    {#if hasProfile(entry) && entry.profile.verified}
                      <BadgeCheck class="size-3 shrink-0 text-link" aria-label="verified" />
                    {/if}
                  </p>
                  <p
                    class="truncate text-caption {isUnavailable(entry) || isUnchecked(entry)
                      ? 'text-fg-subtle italic'
                      : 'text-fg-muted'}"
                    title={isIncomplete(entry) ? `${profileLine(entry)}. ${LOAD_HINT}` : profileLine(entry)}
                  >
                    {profileLine(entry)}
                  </p>
                </div>

                <div class="flex shrink-0 items-center gap-1">
                  {#each links.slice(0, ROW_BADGES) as link (link.key)}
                    {@render linkBadge(link)}
                  {/each}
                  {#if links.length > ROW_BADGES}
                    <span
                      class="font-mono text-[10px] text-fg-muted"
                      title={`${links.length - ROW_BADGES} more: open the row`}
                    >
                      +{links.length - ROW_BADGES}
                    </span>
                  {/if}
                  <Tooltip.Root>
                    <Tooltip.Trigger>
                      {#snippet child({ props })}
                        <Button
                          {...props}
                          size="icon-sm"
                          variant="ghost"
                          class="text-fg-muted"
                          aria-expanded={isOpen(entry.permalink)}
                          aria-label={`about ${name}`}
                          onclick={() => toggleOpen(entry.permalink)}
                        >
                          <ChevronDown
                            class={isOpen(entry.permalink)
                              ? 'rotate-180 transition-transform'
                              : 'transition-transform'}
                          />
                        </Button>
                      {/snippet}
                    </Tooltip.Trigger>
                    <Tooltip.Content class="max-w-64">{OPEN_HINT}</Tooltip.Content>
                  </Tooltip.Root>
                </div>
              </div>

              {#if isOpen(entry.permalink)}
                {@render rowDetails(entry)}
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}

</PanelShell>
