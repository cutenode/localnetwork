<!--
  Session Queue: the tracks picked out while working through a page, held
  together until something is done with the whole batch.

  The Manager tool fills it — opening a download link queues its track, and
  the list button on each row queues one by hand — and everything that is done
  to a batch is here: mark the lot as downloaded once the files have really
  been saved, or send it to one of the other tools, which then read the queue
  instead of the page (the same tracks, whatever tab is open in front).

  What it holds is a working list, not a library: it lives in the extension's
  session storage and goes away with the browser. The marks it makes are what
  lasts.

  Why it is a sheet and not a tool: the queue is about the other tools rather
  than beside them. As the fourth entry in the menu it could only be reached
  by leaving whichever tool had filled it, and coming back meant reading the
  page again. It comes up over the open tool now, from a button in the header
  of every view (QueueButton.svelte), and goes away leaving that tool exactly
  as it was.

  It carries its own Tooltip.Provider because it is rendered at the root of
  the app rather than inside PanelShell, which is where every other tooltip in
  the extension lives.
-->
<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { fly, fade } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Check from '@lucide/svelte/icons/check';
  import Eraser from '@lucide/svelte/icons/eraser';
  import ListChecks from '@lucide/svelte/icons/list-checks';
  import ListX from '@lucide/svelte/icons/list-x';
  import X from '@lucide/svelte/icons/x';
  import StatusNote from '$lib/components/StatusNote.svelte';
  import TrackRow from '$lib/components/TrackRow.svelte';
  import { Button } from '$lib/components/ui/button/index.ts';
  import * as Tooltip from '$lib/components/ui/tooltip/index.ts';
  import { artworkOf } from '$lib/artwork.ts';
  import { downloaded } from '$lib/downloaded.svelte.ts';
  import { queue } from '$lib/queue.svelte.ts';
  import { choicePref } from '$lib/prefs.svelte.ts';
  import { count } from '$lib/soundcloud.ts';
  import { QUEUE, PAGE, SOURCE_PREF, SOURCES } from '$lib/source.ts';
  import { toasts } from '$lib/toast.svelte.ts';
  import { TOOLS } from '$lib/tools.ts';
  import { remembered } from '$lib/view.svelte.ts';

  interface Props {
    /** Puts the sheet away, leaving the view under it where it was. */
    onclose: () => void;
    /** Opens a tool, for the buttons that hand it the batch. */
    onopen: (id: string) => void;
  }

  let {
    onclose,
    onopen,
  }: Props = $props();

  // Set for the tool a batch is sent to, so it opens on the queue rather than
  // on whatever page is in the active tab.
  const source = choicePref(SOURCE_PREF, PAGE, SOURCES);

  // What the other tools do with a batch, in the order the menu lists them.
  // Nothing else here knows about them: the name, the icon and the colour of
  // each button come from the registry.
  const HINTS = new Map([
    ['soundcloud-playlist-to-text', 'turn the queue into text you can paste.'],
    ['third-party-downloads', 'walk the queue’s gate links and save the files.'],
  ]);

  const sendTo = TOOLS.filter((entry) => HINTS.has(entry.id));

  const marked = $derived(queue.permalinks.filter((permalink) => downloaded.has(permalink)).length);

  const unmarked = $derived(queue.count - marked);

  const meta = $derived(
    queue.count === 0
      ? 'nothing in it yet'
      : [count(queue.count, 'track'), ...(marked > 0 ? [`${marked} marked`] : [])].join(' · ')
  );

  async function markAll() {
    const permalinks = queue.permalinks.filter((permalink) => !downloaded.has(permalink));

    try {
      await downloaded.setMany(permalinks, true);
    } catch (_e) {
      // A full or blocked storage. The marks are the one thing here that is
      // meant to last, so a write that did not happen is worth saying out
      // loud rather than reporting as done.
      toasts.bad('couldn’t save the marks. storage refused the write.');

      return;
    }

    toasts.ok(`marked ${count(permalinks.length, 'track')} downloaded`);
  }

  // The other half of "mark the batch": once the files really have been saved
  // and the marks made, the marked tracks are what is *done* with, and the
  // queue is a working list rather than a record of one. Taking them out in
  // one go leaves exactly what is left to do — which is what "clear" cannot
  // give you, since it empties the lot, and what taking them out row by row
  // costs a click each.
  //
  // The marks are untouched, as they are everywhere else here: they live in
  // local storage and outlast the session (downloaded-store.js). Nothing is
  // lost that the marks do not already hold, so unlike "clear" this does not
  // ask twice — it is the Manager's `unqueue` on the tracks the queue itself
  // can pick out.
  function removeMarked() {
    const permalinks = queue.permalinks.filter((permalink) => downloaded.has(permalink));

    if (permalinks.length === 0) return;
    queue.removeMany(permalinks);
    toasts.ok(`took ${count(permalinks.length, 'track')} out of the queue`);
  }

  function open(id: string) {
    source.value = QUEUE;
    onclose();
    onopen(id);
  }

  // "Clear" asks once more, as the Manager tool's does: the first click arms
  // the button for a few seconds, the second empties the queue.
  //
  // The timer is cleared on the way out because the sheet goes in and out far
  // more often than a panel ever did — it is put away on every Escape, every
  // close, every hand-off — and a countdown left running belongs to a sheet
  // that is no longer there. A sheet opened again comes back disarmed, which
  // is what a confirm button should do.
  let armed = $state(false);

  let armedTimer: ReturnType<typeof setTimeout> | 0 = 0;

  onDestroy(() => clearTimeout(armedTimer));

  function onClear() {
    if (!armed) {
      armed = true;
      armedTimer = setTimeout(() => (armed = false), 4000);

      return;
    }

    clearTimeout(armedTimer);
    armed = false;
    queue.clear();
    toasts.ok('queue emptied');
  }

  // Escape closes it, wherever the focus is: a sheet over the whole popup has
  // to answer the key that puts one away.
  function onkeydown(event: KeyboardEvent) {
    if (event.key !== 'Escape') return;
    event.stopPropagation();
    onclose();
  }

  // --- where the list was ----------------------------------------------------

  // The same bargain PanelShell strikes for a panel's body, and a simpler one:
  // the queue is read before anything is drawn, so the rows are all there on
  // the first turn and the place can go straight back. `untrack`, because the
  // stored number is only ever read here — after this it is written.
  const scrolled = remembered('queue', 'scroll', 0);

  const startAt = untrack(() => scrolled.value);

  let body: HTMLElement | null = $state(null);

  $effect(() => {
    if (body && startAt > 0) body.scrollTop = startAt;
  });

  function onscroll() {
    if (body) scrolled.value = body.scrollTop;
  }
</script>

<svelte:window {onkeydown} />

<Tooltip.Provider delayDuration={350}>
  <div class="fixed inset-0 z-40 flex flex-col justify-end">
    <!-- The ground behind the sheet. A click on it puts the sheet away, which
         is a shortcut rather than the way out: the close button in the header
         is what a keyboard uses, so this is hidden from the tree entirely. -->
    <button
      type="button"
      tabindex="-1"
      aria-hidden="true"
      class="absolute inset-0 bg-foreground/30 backdrop-blur-[1px]"
      transition:fade={{ duration: 150 }}
      onclick={onclose}
    ></button>

    <div
      role="dialog"
      aria-modal="true"
      aria-label="session queue"
      class="relative flex max-h-[85vh] flex-col rounded-t-2xl border-t border-border/70
             bg-background shadow-[0_-8px_30px_oklch(0_0_0/12%)]"
      transition:fly={{ y: 240, duration: 240, easing: cubicOut, opacity: 1 }}
    >
      <header
        class="brand-wash flex shrink-0 items-center gap-2.5 rounded-t-2xl border-b
               border-border/70 px-3 py-2.5"
      >
        <div
          class="brand-tile grid size-8 shrink-0 place-items-center rounded-[10px] text-white"
          aria-hidden="true"
        >
          <ListChecks class="size-4" />
        </div>

        <div class="min-w-0 flex-1">
          <h2 class="truncate text-[13px] leading-tight font-semibold">session queue</h2>
          <p class="tabular truncate text-[11px] leading-tight text-muted-foreground" title={meta}>
            {meta}
          </p>
        </div>

        <div class="flex shrink-0 items-center gap-1.5">
          {#if queue.count > 0}
            <Button
              size="sm"
              variant={armed ? 'default' : 'outline'}
              class={armed ? 'bg-destructive text-white hover:bg-destructive/90' : ''}
              title="empties the queue. marks stay."
              onclick={onClear}
            >
              <Eraser />
              {armed ? 'empty it?' : 'clear'}
            </Button>
          {/if}

          <!-- The browser's own tooltip rather than the shell's, which is
               what the back arrow in a panel header has. The difference is
               that this is the only way out of an empty queue, and a tooltip
               trigger is a wrapper around the button: the one control a
               screen reader has to find here is better off being the button
               itself. -->
          <Button
            variant="ghost"
            size="icon-sm"
            class="-mr-1 text-muted-foreground"
            aria-label="close the queue"
            title="close"
            onclick={onclose}
          >
            <X />
          </Button>
        </div>
      </header>

      <div bind:this={body} {onscroll} class="scroll-slim min-h-0 flex-1 overflow-y-auto px-3 py-3">
        {#if queue.count === 0}
          <StatusNote tone="info">nothing here yet. queue tracks from Manager.</StatusNote>
        {:else}
          <div class="flex flex-col gap-2.5">
            <Button disabled={unmarked === 0} onclick={markAll}>
              <Check />
              {unmarked === 0
                ? 'all marked downloaded'
                : `mark ${count(unmarked, 'track')} downloaded`}
            </Button>

            <!-- Only when there is something for it to take: the same rule the
                 header's "clear" follows, and the reason neither appears over
                 an empty queue. -->
            {#if marked > 0}
              <Button
                variant="outline"
                title="takes the tracks already marked downloaded out of the queue. the marks stay."
                onclick={removeMarked}
              >
                <ListX />
                take {marked === queue.count ? 'all' : count(marked, 'track')} marked out
              </Button>
            {/if}

            <div class="flex flex-col gap-1.5 rounded-xl border border-border/70 bg-card p-2.5">
              <span class="text-[11.5px] text-muted-foreground"> send the queue somewhere </span>
              <div class="flex flex-wrap gap-1.5">
                {#each sendTo as entry (entry.id)}
                  {@const Icon = entry.icon}
                  <Tooltip.Root>
                    <Tooltip.Trigger>
                      {#snippet child({ props })}
                        <Button
                          {...props}
                          size="xs"
                          variant="outline"
                          style="--brand: {entry.brand}"
                          onclick={() => open(entry.id)}
                        >
                          <Icon />
                          {entry.short}
                        </Button>
                      {/snippet}
                    </Tooltip.Trigger>
                    <Tooltip.Content class="max-w-64">{HINTS.get(entry.id)}</Tooltip.Content>
                  </Tooltip.Root>
                {/each}
              </div>
            </div>

            <ul class="flex flex-col gap-1.5">
              {#each queue.items as item (item.permalink)}
                {@const isDownloaded = downloaded.has(item.permalink)}
                <TrackRow
                  permalink={item.permalink}
                  title={item.title}
                  artist={item.artist}
                  artworkUrl={artworkOf(item)}
                  {isDownloaded}
                >
                  {#snippet actions()}
                    <Button
                      size="icon-sm"
                      variant={isDownloaded ? 'default' : 'outline'}
                      class={isDownloaded
                        ? 'bg-success text-success-foreground hover:bg-success/85'
                        : 'text-muted-foreground'}
                      aria-pressed={isDownloaded}
                      aria-label="mark downloaded"
                      title="toggle the downloaded mark"
                      onclick={() => downloaded.toggle(item.permalink)}
                    >
                      <Check />
                    </Button>
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      class="text-muted-foreground"
                      aria-label="remove from the queue"
                      title="take out of the queue. its mark stays."
                      onclick={() => queue.remove(item.permalink)}
                    >
                      <X />
                    </Button>
                  {/snippet}
                </TrackRow>
              {/each}
            </ul>
          </div>
        {/if}
      </div>
    </div>
  </div>
</Tooltip.Provider>
