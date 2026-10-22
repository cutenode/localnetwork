<!--
  What the artists tool can add right now, and the buttons that do it.

  The list is the tool's own and nothing goes onto it by itself (see
  ./artists.svelte.ts), so this is the one place artists come in — from three
  sources rather than the Manager's two. The tab in front is the first, as it
  is there; the Manager's list is the second and the reason this bar is worth
  having on a tab that is not SoundCloud at all, since everything ever added
  to that list has an uploader; and the session queue is the third, for the
  batch picked out an hour ago.

  Each button counts what it would add and is off when that is nothing: a
  source wholly on the list is a button that would do nothing visible. The
  tab's says **refresh** then instead, since it still brings the names and
  the tracks up to date.

  It draws large when the list is empty — that opening is the whole panel —
  and as two lines above the list once there is a list to be above: the page
  and what it holds, then the buttons. Three buttons beside a page's name is
  one more than a 400px popup has room for.
-->
<script lang="ts">
  import Plus from '@lucide/svelte/icons/plus';
  import ToolTile from '$lib/components/ToolTile.svelte';
  import FileMusic from '@lucide/svelte/icons/file-music';
  import ListChecks from '@lucide/svelte/icons/list-checks';
  import Download from '@lucide/svelte/icons/download';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import { Button } from '$lib/components/ui/button/index.ts';
  import { count } from '$lib/soundcloud.ts';
  import type { Tool } from '$lib/types';

  interface Offer {
    /** Whether the tab is still being read. */
    reading: boolean;
    /** The page's name. */
    name: string;
    /** The line under it: who it is by, and what it holds. */
    meta: string;
    /** How many of its artists are not on the list. */
    fresh: number;
    /** How many are. */
    here: number;
    /** Why the tab has nothing to add, or ''. */
    reason: string;
  }

  interface Props {
    /** The artists tool, whose tile heads the empty list. */
    tool: Tool;
    /**
     * What the active tab is offering:
     * `{ reading, name, meta, fresh, here, reason }` — see the panel, which
     * builds it from one read of the page.
     */
    offer: Offer;
    /** How many artists are behind the Manager's list, and how many are not here. */
    listed?: number;
    listedFresh?: number;
    /** The same for the session queue. */
    queued?: number;
    queuedFresh?: number;
    /** Whether an add is running, which is what every button waits on. */
    adding?: boolean;
    /** Whether to draw the opening version, which is the empty list's body. */
    big?: boolean;
    /** Called with 'tab', 'list' or 'queue'. */
    onadd: (source: 'tab' | 'list' | 'queue') => void;
  }

  let {
    tool,
    offer,
    listed = 0,
    listedFresh = 0,
    queued = 0,
    queuedFresh = 0,
    adding = false,
    big = false,
    onadd,
  }: Props = $props();

  const addable = $derived(offer.fresh + offer.here);

  const tabLabel = $derived(offer.fresh > 0 ? `add ${offer.fresh}` : `refresh ${offer.here}`);

  const tabName = $derived(
    `${offer.fresh > 0 ? 'add' : 'refresh'} ${count(offer.fresh || offer.here, 'artist')}`
  );

  const tabTitle = $derived(
    offer.fresh > 0
      ? `lists ${count(offer.fresh, 'artist')} and looks up their socials.`
      : `all here already — brings ${count(offer.here, 'artist')} up to date.`
  );

  const canAddTab = $derived(!adding && !offer.reading && addable > 0);

  const canAddList = $derived(!adding && listedFresh > 0);

  const listTitle = $derived(
    listedFresh > 0
      ? `lists the ${count(listedFresh, 'artist')} behind the Manager’s tracks not here yet.`
      : `all ${count(listed, 'artist')} behind the Manager’s tracks are here already.`
  );

  const canAddQueue = $derived(!adding && queuedFresh > 0);

  const queueTitle = $derived(
    queuedFresh > 0
      ? `lists the ${count(queuedFresh, 'artist')} behind the queued tracks not here yet.`
      : `all ${count(queued, 'artist')} behind the queued tracks are here already.`
  );
</script>

<div
  class="flex flex-col gap-2 rounded-lg border border-line bg-surface {big
    ? 'px-3.5 py-4'
    : 'px-3 py-2.5'}"
>
  {#if big}
    <div class="flex flex-col items-center gap-1.5 text-center">
      <ToolTile icon={tool.icon} tone={tool.tone} size="md" />
      <h2 class="m-0 text-small font-semibold">nobody here yet</h2>
      <p class="max-w-[19rem] text-caption leading-relaxed text-fg-muted">
        open a SoundCloud playlist, likes or track page and add its artists. the Manager’s list
        and the session queue add from anywhere.
      </p>
    </div>
  {/if}

  <div class="flex flex-col gap-2 {big ? 'mt-1 items-center' : ''}">
    <div class="min-w-0 {big ? 'w-full text-center' : ''}">
      <p class="flex items-center gap-1.5 text-caption font-semibold {big ? 'justify-center' : ''}">
        {#if offer.reading}
          <LoaderCircle class="size-3 shrink-0 animate-spin text-fg-muted" />
        {:else}
          <FileMusic class="size-3 shrink-0 text-fg-muted" />
        {/if}
        <span class="truncate" title={offer.name}>
          {offer.reading ? 'looking at this tab…' : offer.name || 'this tab'}
        </span>
      </p>
      <p
        class="text-caption leading-relaxed text-fg-muted {big || offer.reason
          ? ''
          : 'truncate'}"
        title={offer.reason || offer.meta}
      >
        {offer.reading ? '' : offer.reason || offer.meta}
      </p>
    </div>

    <div class="flex flex-wrap items-center gap-1.5 {big ? 'w-full flex-col' : ''}">
      <Button
        size={big ? 'default' : 'sm'}
        class={big ? 'w-full' : ''}
        disabled={!canAddTab}
        aria-label={addable > 0 ? `${tabName} from this tab` : 'add from this tab'}
        title={addable > 0 ? tabTitle : 'nothing in this tab to add.'}
        onclick={() => onadd('tab')}
      >
        {#if adding}
          <LoaderCircle class="animate-spin" />
          adding…
        {:else}
          <Plus />
          {addable > 0 ? tabLabel : 'add'}
        {/if}
      </Button>

      {#if listed > 0}
        <Button
          size={big ? 'default' : 'sm'}
          variant="outline"
          class={big ? 'w-full' : ''}
          disabled={!canAddList}
          aria-label={`add the Manager’s list — ${count(listedFresh, 'artist')}`}
          title={listTitle}
          onclick={() => onadd('list')}
        >
          <Download />
          Manager ({listedFresh || listed})
        </Button>
      {/if}

      {#if queued > 0}
        <Button
          size={big ? 'default' : 'sm'}
          variant="outline"
          class={big ? 'w-full' : ''}
          disabled={!canAddQueue}
          aria-label={`add the session queue — ${count(queuedFresh, 'artist')}`}
          title={queueTitle}
          onclick={() => onadd('queue')}
        >
          <ListChecks />
          queue ({queuedFresh || queued})
        </Button>
      {/if}
    </div>
  </div>
</div>
