<!--
  artists: every uploader behind the tracks somebody has added, and where else
  to find them.

  The list is the tool's own and is kept for good (./artists.svelte.ts), the
  way the Manager's is: nothing goes onto it by itself, and the bar at the top
  is the one place artists come in — from the tab in front, from the Manager's
  list, or from the session queue. Adding puts the artists on the list at once,
  from what the source itself says (a name and the tracks), and then asks
  SoundCloud about each one it has no answer for: the public profile behind
  the path, and the links the uploader put on it. The rows are up before the
  first answer and fill in as the answers land.

  A row is the artist's picture, name and a line about them, and then their
  socials as lettered badges — one per link, each opening that page. The
  chevron opens the row into everything else: every link as an address to
  copy, the bio, the place, the follower count, the tracks that brought the
  artist here — and, from Settings, the search badges that look for the
  artist on the shops the Manager searches.

  The socials are asked for through a SoundCloud tab (./lookup.ts), any page
  of it; the list itself is on screen on any tab at all, with whatever it
  already knows.
-->
<script lang="ts">
  import { untrack } from 'svelte';
  import BadgeCheck from '@lucide/svelte/icons/badge-check';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import Copy from '@lucide/svelte/icons/copy';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Search from '@lucide/svelte/icons/search';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import User from '@lucide/svelte/icons/user';
  import X from '@lucide/svelte/icons/x';
  import PanelShell from '$lib/components/PanelShell.svelte';
  import StatusNote from '$lib/components/StatusNote.svelte';
  import AddBar from './AddBar.svelte';
  import { Button } from '$lib/components/ui/button/index.ts';
  import { Input } from '$lib/components/ui/input/index.ts';
  import { Label } from '$lib/components/ui/label/index.ts';
  import { Progress } from '$lib/components/ui/progress/index.ts';
  import { Switch } from '$lib/components/ui/switch/index.ts';
  import * as Select from '$lib/components/ui/select/index.ts';
  import * as Tooltip from '$lib/components/ui/tooltip/index.ts';
  import { openTab } from '$lib/chrome.ts';
  import { copyText } from '$lib/clipboard.ts';
  import { matches } from '$lib/filter.ts';
  import { choicePref, flagPref } from '$lib/prefs.svelte.ts';
  import { queue } from '$lib/queue.svelte.ts';
  import { queryOf, searchable, searchUrl } from '$lib/search-sites.ts';
  import { searchSites } from '$lib/search-sites.svelte.ts';
  import { count, OPEN_A_PAGE, readPage } from '$lib/soundcloud.ts';
  import { toasts } from '$lib/toast.svelte.ts';
  import { remembered } from '$lib/view.svelte.ts';
  import { listed } from '../manager/listed.svelte.ts';
  import { artists, MAX } from './artists.svelte.ts';
  import { LOOKUP_ERRORS, lookupProfiles, onSoundCloud, OPEN_A_TAB } from './lookup.ts';
  import {
    artistsOf,
    asText,
    hasProfile,
    isUnavailable,
    isUnchecked,
    linksOf,
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
  import type {
    Artist,
    ArtistEntry,
    PageProgress,
    PageTrack,
    PageType,
    PanelProps,
    ProfileAnswer,
    ProfilesAnswer,
    ProfilesProgress,
  } from '$lib/types';

  let { tool, onback }: PanelProps = $props();

  const SORT_HINT = 'by name sorts the list. list order keeps the order you added them.';

  const BACKGROUND_HINT = 'opens links behind the popup instead of taking focus.';

  const OPEN_HINT = 'everything known about this artist. click a field to copy it.';

  const FACT_HINT = 'click to copy';

  const LINK_HINT = 'click to copy the address. the arrow opens it in a tab.';

  const LOOKUP_HINT = 'asks SoundCloud who’s behind each path, and for the links on their profile.';

  const AGAIN_HINT = 'asks SoundCloud about this artist again.';

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

  artists.start().then(() => (ready = true));

  // The Manager's list is a source, and read for the count on its button.
  listed.start();

  // Whether the tab in front can be asked: any SoundCloud page will do, since
  // the content script is in every one of them. Read once, as the page is.
  let onSc = $state(false);

  onSoundCloud().then((yes) => (onSc = yes));

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

  // --- adding, and looking up ------------------------------------------------

  let adding = $state(false);

  let lookingUp = $state(false);

  /** How far the running lookup has got. */
  let progress = $state<ProfilesProgress | null>(null);

  /** The paths the running lookup covers, so their rows can say so. */
  let checkingIds: Set<string> = $state(new Set());

  /** What the last add or lookup came to, when it is worth leaving on screen. */
  let note = $state<{ tone: 'bad' | 'info'; text: string } | null>(null);

  // Answers arriving from the lookup, an artist at a time, taken straight
  // into the list — the store coalesces the writes. A row stops saying it is
  // being looked up the moment its own answer lands, not when the whole
  // lookup ends.
  try {
    chrome.runtime.onMessage.addListener((message: PageProgress) => {
      if (!message || message.type !== 'ARTIST_PROFILES_PROGRESS') return;

      if (!lookingUp) return;
      progress = message;
      take(answersIn(message));
    });
  } catch (_e) {
    // No runtime to listen on: the lookup just finishes without a count.
  }

  /**
   * A lookup's answers as the store takes them: profiles, and failures. `as const` keeps
   * `unavailable: true` from widening to a boolean inside the map.
   * @param answer a progress message, or the lookup's whole answer
   */
  const answersIn = ({ profiles, failed }: Pick<ProfilesAnswer, 'profiles' | 'failed'>) => [
    ...(Array.isArray(profiles) ? profiles : []),
    ...(Array.isArray(failed) ? failed : []).map((permalink) => ({ permalink, unavailable: true as const })),
  ];

  function take(answers: ProfileAnswer[]) {
    artists.learn(answers);
    const answered = new Set(answers.map((answer) => answer?.permalink));
    checkingIds = new Set([...checkingIds].filter((permalink) => !answered.has(permalink)));
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

  /** Asks SoundCloud about these artists, folding the answers into the list. */
  async function lookUp(permalinks: string[]) {
    if (permalinks.length === 0) return;
    lookingUp = true;
    progress = null;
    checkingIds = new Set(permalinks);
    const result = await lookupProfiles(permalinks);

    if (result.status) {
      note = { tone: 'bad', text: `couldn’t look up their socials. ${result.status}` };
    } else {
      // The whole answer, answers already streamed included: the same
      // profile twice is the same profile.
      take(answersIn(result.data!));
      const why = LOOKUP_ERRORS[result.data!.error];

      if (why) note = { tone: 'bad', text: `some socials weren’t looked up. ${why}` };
    }

    await artists.flush();
    checkingIds = new Set();
    progress = null;
    lookingUp = false;
  }

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

    // Then the socials, for whichever of them nobody has asked about — where
    // there is a SoundCloud tab to ask. Without one the rows say so, and the
    // bar offers the lookup once there is.
    if (!onSc) return;
    const asking = new Set(unchecked.map((entry) => entry.permalink));
    await lookUp(list.map((artist) => artist.permalink).filter((one) => asking.has(one)));
  }

  const unchecked = $derived(artists.items.filter(isUnchecked));

  // --- the rows --------------------------------------------------------------

  const ordered = $derived(sorted(artists.items, sort.value));

  const visible = $derived(ordered.filter((entry) => matches(searchText(entry), query.value)));

  const withSocials = $derived(
    artists.items.filter((entry) => hasProfile(entry) && linksOf(entry.profile).length > 0).length
  );

  const meta = $derived(
    artists.count === 0
      ? ''
      : [
          count(artists.count, 'artist'),
          `${withSocials} with socials`,
          ...(unchecked.length > 0 ? [`${unchecked.length} not looked up`] : []),
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
  <Button
    size="icon-sm"
    variant="ghost"
    style={link.style}
    class="w-7 border-transparent text-[10px] font-bold hover:brightness-110"
    aria-label={`open ${link.name}: ${link.url}`}
    title={link.title ? `${link.title} — ${link.url}` : link.url}
    onclick={() => openLink(link.url)}
  >
    {link.badge}
  </Button>
{/snippet}

<!-- What a row opens into: every field of the artist, each a button that
     copies it; an address gets an arrow of its own. Then the search badges,
     the way to ask again, and the way off the list. -->
{#snippet rowDetails(entry: ArtistEntry)}
  {@const facts = rowFacts(entry)}
  {@const name = nameOf(entry)}
  <dl
    class="mt-1.5 flex flex-col gap-px border-t border-border/60 pt-1.5 text-[11.5px]"
    aria-label={`about ${name}`}
  >
    {#each facts as fact (fact.id)}
      <div class="flex items-baseline gap-2">
        <dt class="w-16 shrink-0 truncate text-[10.5px] text-muted-foreground" title={fact.label}>
          {fact.label}
        </dt>
        <dd class="flex min-w-0 flex-1 items-baseline gap-1">
          <button
            type="button"
            class="min-w-0 flex-1 rounded px-1 py-0.5 text-left
                   enabled:cursor-pointer enabled:hover:bg-accent
                   disabled:text-muted-foreground/70 disabled:italic
                   focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none
                   {fact.id === 'bio' ? 'line-clamp-4 whitespace-pre-line' : 'truncate'}"
            disabled={!fact.copy}
            aria-label={fact.copy ? `copy the ${fact.label}` : `${fact.label}: ${fact.value}`}
            title={fact.copy ? (fact.open ? LINK_HINT : FACT_HINT) : ''}
            onclick={() => onCopyFact(fact)}
          >
            {fact.value}
          </button>
          {#if fact.open}
            <Button
              size="icon-xs"
              variant="ghost"
              class="shrink-0 text-muted-foreground hover:text-foreground"
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
      <Button
        size="icon-sm"
        variant="ghost"
        style={site.style}
        class="w-7 border-transparent text-[10px] font-bold hover:brightness-110"
        disabled={!searchable(site, track)}
        aria-label={`search ${site.name} for ${search}`}
        title={`search ${site.name} for “${search}”`}
        onclick={() => openLink(searchUrl(site, track))}
      >
        {site.badge}
      </Button>
    {/each}
    <div class="ml-auto flex items-center gap-1">
      <Button
        size="xs"
        variant="ghost"
        class="text-muted-foreground"
        disabled={lookingUp || !onSc}
        aria-label={`look up ${name} again`}
        title={onSc ? AGAIN_HINT : OPEN_A_TAB}
        onclick={() => lookUp([entry.permalink])}
      >
        <RefreshCw />
        look up again
      </Button>
      <Button
        size="xs"
        variant="ghost"
        class="text-muted-foreground hover:text-destructive"
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

  {#snippet banner()}
    {#if lookingUp && progress}
      <div class="shrink-0 border-b border-border/70 px-3 py-2">
        <div class="mb-1 flex items-center justify-between text-[11px] text-muted-foreground">
          <span>looking up socials…</span>
          <span class="tabular">{progress.done} / {progress.total}</span>
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
        {#if unchecked.length > 0 || lookingUp}
          <!-- The artists nobody has asked SoundCloud about, and the button
               that asks — or, on a tab that cannot ask, why it is off. -->
          <div
            class="flex items-center gap-2 rounded-xl border border-border/70 bg-muted/40 px-3 py-2 text-[11.5px]"
            role="status"
          >
            <span class="min-w-0 flex-1 leading-relaxed">
              {#if lookingUp}
                looking up {count(checkingIds.size, 'artist')}…
              {:else if onSc}
                {count(unchecked.length, 'artist')} not looked up.
              {:else}
                {count(unchecked.length, 'artist')} not looked up — {OPEN_A_TAB}
              {/if}
            </span>
            <Button
              size="xs"
              disabled={lookingUp || !onSc}
              aria-label={`look up ${count(unchecked.length, 'artist')}`}
              title={onSc ? LOOKUP_HINT : OPEN_A_TAB}
              onclick={() => lookUp(unchecked.map((entry) => entry.permalink))}
            >
              {#if lookingUp}
                <LoaderCircle class="animate-spin" />
                looking up…
              {:else}
                <Search />
                look up
              {/if}
            </Button>
          </div>
        {/if}

        <div class="flex flex-col gap-1.5">
          <div class="relative min-w-0 flex-1">
            <Search
              class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground"
            />
            <Input
              type="search"
              class="h-8 pr-7 pl-8 text-[12.5px] [&::-webkit-search-cancel-button]:hidden"
              placeholder="search name, place, bio or link"
              autocomplete="off"
              spellcheck="false"
              aria-label="search the artists"
              bind:value={query.value}
            />
            {#if query.value}
              <button
                type="button"
                class="absolute top-1/2 right-1.5 grid size-5 -translate-y-1/2 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                aria-label="clear the search"
                onclick={() => (query.value = '')}
              >
                <X class="size-3" />
              </button>
            {/if}
          </div>
          {#if countLine}
            <p class="tabular text-[11.5px] text-muted-foreground">{countLine}</p>
          {/if}
        </div>

        <div class="flex items-center gap-2.5">
          <Tooltip.Root>
            <Tooltip.Trigger class="min-w-0 flex-1">
              {#snippet child({ props })}
                <div {...props} class="min-w-0 flex-1">
                  <Select.Root type="single" bind:value={sort.value}>
                    <Select.Trigger class="h-8 w-full text-[12.5px]" aria-label="sort order">
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
                    class="text-[11.5px] font-normal whitespace-nowrap text-muted-foreground"
                  >
                    background tabs
                  </Label>
                </div>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content class="max-w-72">{BACKGROUND_HINT}</Tooltip.Content>
          </Tooltip.Root>
        </div>

        <ul class="flex flex-col gap-1.5">
          {#each visible as entry (entry.permalink)}
            {@const name = nameOf(entry)}
            {@const links = linksOf(entry.profile)}
            {@const checking = checkingIds.has(entry.permalink)}
            {@const avatar = hasProfile(entry) ? entry.profile.avatarUrl : ''}
            <li
              data-permalink={entry.permalink}
              class="lift rounded-xl border border-border/70 bg-card px-2 py-1.5 hover:border-border"
            >
              <div class="flex items-center gap-2.5">
                {#if avatar}
                  <img
                    src={avatar}
                    alt=""
                    loading="lazy"
                    class="size-10 shrink-0 rounded-full bg-muted object-cover ring-1 ring-foreground/8"
                  />
                {:else}
                  <div
                    class="grid size-10 shrink-0 place-items-center rounded-full bg-muted text-muted-foreground ring-1 ring-foreground/8"
                    title="no picture"
                  >
                    <User class="size-4 opacity-60" />
                  </div>
                {/if}

                <div class="min-w-0 flex-1">
                  <p class="flex items-center gap-1 truncate text-[12.5px] font-semibold" title={name}>
                    <span class="truncate">{name}</span>
                    {#if hasProfile(entry) && entry.profile.verified}
                      <BadgeCheck class="size-3 shrink-0 text-brand" aria-label="verified" />
                    {/if}
                  </p>
                  <p
                    class="truncate text-[11.5px] {isUnavailable(entry)
                      ? 'text-muted-foreground/70 italic'
                      : 'text-muted-foreground'}"
                    title={profileLine(entry, { checking })}
                  >
                    {profileLine(entry, { checking })}
                  </p>
                </div>

                <div class="flex shrink-0 items-center gap-1">
                  {#if checking}
                    <LoaderCircle
                      class="size-3.5 shrink-0 animate-spin text-muted-foreground"
                      aria-label="looking up"
                    />
                  {:else}
                    {#each links.slice(0, ROW_BADGES) as link (link.key)}
                      {@render linkBadge(link)}
                    {/each}
                    {#if links.length > ROW_BADGES}
                      <span
                        class="tabular text-[10px] text-muted-foreground"
                        title={`${links.length - ROW_BADGES} more — open the row`}
                      >
                        +{links.length - ROW_BADGES}
                      </span>
                    {/if}
                  {/if}
                  <Tooltip.Root>
                    <Tooltip.Trigger>
                      {#snippet child({ props })}
                        <Button
                          {...props}
                          size="icon-sm"
                          variant="ghost"
                          class="text-muted-foreground"
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
