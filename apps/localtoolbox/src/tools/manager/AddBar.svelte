<!--
  What the Manager can add to its list right now, and the buttons that do it.

  The tool keeps its own list and nothing goes onto it by itself (see
  ./listed.svelte.ts), so this is the one place tracks come in — and it has to
  be honest about two things at once: what the tab in front is offering, and
  how much of that the list already holds. "add 42" on a page whose forty-two
  tracks are all on the list already would be a button that does nothing
  visible, so it says **refresh** instead, which is what it really does.

  It draws large when the list is empty — that opening is the whole panel, and
  the only thing to do on it is add — and as one line above the list once there
  is a list to be above. Both are this component rather than two, so the words
  stay the same as the panel fills up.

  The session queue is the second source, and the reason the panel is worth
  opening on a tab that is not SoundCloud at all: a batch picked out on a page
  an hour ago can be put on the list from anywhere.
-->
<script lang="ts">
  import Plus from '@lucide/svelte/icons/plus';
  import FileMusic from '@lucide/svelte/icons/file-music';
  import ListChecks from '@lucide/svelte/icons/list-checks';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import { Button } from '$lib/components/ui/button/index.ts';
  import ToolTile from '$lib/components/ToolTile.svelte';
  import { count } from '$lib/soundcloud.ts';

  interface Props {
    /**
     * What the active tab is offering:
     * `{ reading, name, meta, fresh, here, reason }` — see the panel, which
     * builds it from one read of the page.
     */
    offer: {
      reading: boolean;
      name: string;
      meta: string;
      fresh: number;
      here: number;
      reason: string;
    };
    /** How many tracks the session queue holds, and how many are not listed. */
    queued?: number;
    queuedFresh?: number;
    /** Whether an add is running, which is what both buttons wait on. */
    adding?: boolean;
    /** Whether to draw the opening version, which is the empty list's body. */
    big?: boolean;
    /** Called with 'tab' or 'queue'. */
    onadd: (source: 'tab' | 'queue') => void;
    /** The tool's tone, for the tile on the opening version. */
    tone?: string;
  }

  let { offer, queued = 0, queuedFresh = 0, adding = false, big = false, onadd, tone = '' }: Props = $props();

  // What pressing the button would really touch: the tracks it can add, plus
  // the ones it would bring up to date. Not every row of a page is one of
  // them — a track the page rendered without a permalink is not something a
  // list can hold, since there would be nothing to tell it from another.
  const addable = $derived(offer.fresh + offer.here);

  // What the tab's button says. Adding is the common case and reads as itself;
  // a page already wholly on the list is the one worth a different word, since
  // pressing it then changes no row's presence and only its answers.
  const tabLabel = $derived(offer.fresh > 0 ? `add ${offer.fresh}` : `refresh ${offer.here}`);

  const tabName = $derived(
    `${offer.fresh > 0 ? 'add' : 'refresh'} ${count(offer.fresh || offer.here, 'track')}`
  );

  const tabTitle = $derived(
    offer.fresh > 0
      ? `lists ${count(offer.fresh, 'track')} and looks up their download links.`
      : `all here already — looks up the links for ${count(offer.here, 'track')} again.`
  );

  const canAddTab = $derived(!adding && !offer.reading && addable > 0);

  // The queue's button counts what it would add, as the tab's does — and is
  // off when that is none, because a queue wholly on the list is a button
  // that would do nothing visible. It says the whole queue's size then, so
  // the number on it still answers the count in the header.
  const canAddQueue = $derived(!adding && queuedFresh > 0);

  const queueLabel = $derived(`queue (${queuedFresh || queued})`);

  const queueTitle = $derived(
    queuedFresh > 0
      ? `lists the ${count(queuedFresh, 'queued track')} not on it yet.`
      : `all ${count(queued, 'queued track')} are on the list already.`
  );
</script>

<div
  class="flex flex-col gap-2 rounded-lg border border-line bg-surface {big
    ? 'px-3.5 py-5'
    : 'px-3 py-2.5'}"
>
  {#if big}
    <div class="flex flex-col items-center gap-1.5 text-center">
      <ToolTile icon={Plus} {tone} size="md" />
      <h2 class="m-0 mt-1 text-small font-semibold">nothing on the list yet</h2>
      <p class="m-0 max-w-[19rem] text-caption text-fg-muted">
        open a SoundCloud playlist, likes or track page and add it here. what you add stays.
      </p>
    </div>
  {/if}

  <div class="flex items-center gap-2.5 {big ? 'mt-1 flex-col' : ''}">
    <div class="min-w-0 flex-1 {big ? 'w-full text-center' : ''}">
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
      <!-- Either what is there to take or why there is nothing. A page that
           cannot be added has a *reason*, and the reason is the whole of what
           this bar has to say then, so it wraps; a count of what is on a page
           somebody can see for themselves is not, so above a list it keeps to
           one line and says the rest on hover. -->
      <p
        class="text-caption text-fg-muted {big || offer.reason
          ? ''
          : 'truncate'}"
        title={offer.reason || offer.meta}
      >
        {offer.reading ? '' : offer.reason || offer.meta}
      </p>
    </div>

    <div class="flex shrink-0 items-center gap-1.5 {big ? 'w-full flex-col' : ''}">
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

      {#if queued > 0}
        <Button
          size={big ? 'default' : 'sm'}
          variant="outline"
          class={big ? 'w-full' : ''}
          disabled={!canAddQueue}
          aria-label={`add the session queue — ${count(queued, 'track')}`}
          title={queueTitle}
          onclick={() => onadd('queue')}
        >
          <ListChecks />
          {queueLabel}
        </Button>
      {/if}
    </div>
  </div>
</div>
