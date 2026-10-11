<!--
  One track in a list: its cover art, its title and artist, an optional line
  saying something about it, and whatever buttons the panel puts on the right.

  A track marked as downloaded is tinted and keeps a check in front of its
  title, so the ones still to get stand out at a glance while scrolling.

  The artwork is a button when the panel gives it something to do (`onartwork`),
  and a plain picture otherwise. `artworkState` is what that button is busy
  with, shown over the thumbnail so the row itself says what happened rather
  than only the toast at the foot of the popup.

  A row gains a tick box in front of everything else when the panel is in
  selecting mode (`selectable`): the list is then something to pick a batch out
  of, and what is done to that batch is above the list rather than on the row.

  A panel that gives a `details` snippet gets a second line under the row, the
  full width of it, for whatever will not fit on one: the row proper stays the
  fixed-height thing a list of five hundred can be scrolled through, and what
  opens out of it sits below rather than inside. The snippet is rendered only
  when it is passed, so a row that has nothing to open is the single line it
  always was.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import Check from '@lucide/svelte/icons/check';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import Artwork from './Artwork.svelte';
  import { Checkbox } from '$lib/components/ui/checkbox/index.ts';
  import { cn } from '$lib/utils.ts';

  type ClickHandler = ((event: MouseEvent) => void) | null;

  interface Props {
    /** The track's permalink path, which is also how a row is identified. */
    permalink?: string;
    title?: string;
    artist?: string;
    /** A line under the artist, where there is something to say. */
    detail?: string;
    artworkUrl?: string;
    isDownloaded?: boolean;
    /** Whether the row carries a tick box: the panel is selecting a batch. */
    selectable?: boolean;
    /** Whether this row is ticked. */
    selected?: boolean;
    /** Called with the click event when the tick box is clicked. */
    onselect?: ClickHandler;
    /** Called with the click event when the artwork is clicked. */
    onartwork?: ClickHandler;
    /** What the artwork button is doing. */
    artworkState?: '' | 'copying' | 'copied';
    /** The artwork button's accessible name. */
    artworkLabel?: string;
    /** What its tooltip says the two gestures do. */
    artworkHint?: string;
    class?: string;
    /** Buttons for the right-hand side. */
    actions?: Snippet;
    /** A second line under the row: what the row opens into, when it does. */
    details?: Snippet | null;
  }

  let {
    permalink = '',
    title = '',
    artist = '',
    detail = '',
    artworkUrl = '',
    isDownloaded = false,
    selectable = false,
    selected = false,
    onselect = null,
    onartwork = null,
    artworkState = '',
    artworkLabel = 'copy the cover art',
    artworkHint = '',
    class: className = '',
    actions,
    details = null,
  }: Props = $props();
</script>

<li
  data-permalink={permalink}
  class={cn(
    'lift rounded-xl border px-2 py-1.5',
    isDownloaded
      ? 'border-success/25 bg-success/8'
      : 'border-border/70 bg-card hover:border-border',
    selected && 'border-primary/60 ring-1 ring-primary/30',
    className
  )}
>
  <div class="flex items-center gap-2.5">
    {#if selectable}
      <!-- The tick box shows what the panel's selection says, and every click on
           it is the panel's to interpret (a Shift-click takes in a run of rows),
           so the binding's setter is deliberately empty: the box cannot tick
           itself. -->
      <Checkbox
        class="ml-0.5 shrink-0"
        bind:checked={() => selected, () => {}}
        aria-label={`select ${title}`}
        onclick={onselect}
      />
    {/if}

    {#if onartwork && artworkUrl}
      <button
        type="button"
        class="relative shrink-0 cursor-pointer rounded-lg outline-none
               focus-visible:ring-3 focus-visible:ring-ring/50"
        aria-label={artworkLabel}
        title={artworkHint || artworkLabel}
        onclick={onartwork}
      >
        <Artwork src={artworkUrl} class="transition hover:brightness-110" />
        {#if artworkState}
          <span class="absolute inset-0 grid place-items-center rounded-lg bg-background/75">
            {#if artworkState === 'copying'}
              <LoaderCircle class="size-4 animate-spin" />
            {:else}
              <Check class="size-4 text-success" />
            {/if}
          </span>
        {/if}
      </button>
    {:else}
      <Artwork src={artworkUrl} />
    {/if}

    <div class="min-w-0 flex-1">
      <p class="flex items-center gap-1 truncate text-[12.5px] font-semibold" {title}>
        {#if isDownloaded}
          <Check class="size-3 shrink-0 text-success" aria-label="marked downloaded" />
        {/if}
        <span class="truncate">{title}</span>
      </p>
      <p class="truncate text-[11.5px] text-muted-foreground" title={artist}>{artist}</p>
      {#if detail}
        <p class="text-[11px] break-words text-muted-foreground/90">{detail}</p>
      {/if}
    </div>

    {#if actions}
      <div class="flex shrink-0 items-center gap-1">{@render actions()}</div>
    {/if}
  </div>

  {#if details}
    {@render details()}
  {/if}
</li>
