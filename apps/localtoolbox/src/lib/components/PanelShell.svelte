<!--
  The frame every tool panel sits in: the header with the way back, the tool's
  name and the line under it, and one scrolling body between the header and an
  optional footer.

  A browser-action popup is sized by its content, so the frame — not the
  panel — owns the height: the header and footer stay put and the body between
  them scrolls, rather than the whole popup growing past what the screen will
  show.

  Owning the one thing that scrolls, it is also what remembers how far: the
  popup closes on every click that opens a tab, and a list forty tracks long
  would otherwise start again at the top each time.

  It also carries the tooltip provider bits-ui wants above every tooltip.
  That used to sit at the root of the app, which put the whole component
  library in the chunk the popup boots from — and the tool menu, which is
  what the popup boots *into*, has not got a tooltip on it. Every tooltip
  inside a panel is inside this; the only ones that are not are the queue
  sheet's, and it carries a provider of its own for the same reason.
-->
<script lang="ts">
  import { untrack, type Snippet } from 'svelte';
  import ArrowLeft from '@lucide/svelte/icons/arrow-left';
  import ToolTile from './ToolTile.svelte';
  import QueueButton from './QueueButton.svelte';
  import SettingsButton from './SettingsButton.svelte';
  import { Button } from '$lib/components/ui/button/index.ts';
  import * as Tooltip from '$lib/components/ui/tooltip/index.ts';
  import { remembered } from '$lib/view.svelte.ts';
  import type { Tool } from '$lib/types';

  interface Props {
    tool: Tool;
    onback: () => void;
    /** The line under the name. */
    meta?: string;
    /** Buttons for the right of the header. */
    actions?: Snippet;
    /** Shown between the header and the body — a progress bar, say. */
    banner?: Snippet;
    footer?: Snippet;
    children: Snippet;
  }

  let {
    tool,
    onback,
    meta = '',
    actions,
    banner,
    footer,
    children,
  }: Props = $props();

  // --- where the list was ----------------------------------------------------

  // `untrack`, because capturing the id is the point: a mounted panel belongs
  // to one tool for its whole life (App.svelte builds a new one when the tool
  // changes), so the place is filed under the tool it was made for.
  const scrolled = remembered(untrack(() => tool.id), 'scroll', 0);

  // Read once, here: the effect below must not re-run every time the number
  // moves, and after this the stored value is only ever written.
  const startAt = scrolled.value;

  let body: HTMLElement | null = $state(null);

  let restoring = startAt > 0;

  /** Where the restore last put the list, so its own scrolling is known. */
  let placed = -1;

  /** Ends the restore, however it ended. Set by the effect below, which is the only thing that
      starts one. */
  let done: (() => void) | undefined;

  // The list is not there when the panel mounts — it arrives when the page has
  // been read, or a lookup has come back — so the position is put back as the
  // content appears, growing towards it. Three things end that: it holds, the
  // user scrolls somewhere else first, or the content turns out never to get
  // that long (a shorter page than the one it was left on), which is what the
  // timeout is for. Without one, every later change to the list — a row
  // finishing, a section opening — would drag it to the bottom again.
  $effect(() => {
    if (!body || !restoring) return;
    const element = body;
    const observer = new MutationObserver(put);
    const timer = setTimeout(() => done!(), 2000);

    done = () => {
      restoring = false;
      observer.disconnect();
      clearTimeout(timer);
    };

    function put() {
      element.scrollTop = startAt;
      placed = element.scrollTop;

      if (placed >= startAt) done!();
    }

    put();
    observer.observe(element, { childList: true, subtree: true });

    return done;
  });

  // Scroll events are frame-bound, and everything one turn changes is written
  // once (view.svelte.js), so following the position as it moves costs a small
  // write per frame of actual scrolling. While a restore is still going, the
  // only position it does not record is the one the restore itself put there.
  function onscroll() {
    if (!body) return;

    if (restoring) {
      if (body.scrollTop === placed) return;
      restoring = false;
      done?.();
    }

    scrolled.value = body.scrollTop;
  }
</script>

<div class="flex max-h-[580px] min-h-[200px] flex-col">
  <!-- Renders no element of its own: the header, the body and the footer are
       still the flex column's own children. -->
  <Tooltip.Provider delayDuration={350}>
    <header class="flex h-13 shrink-0 items-center gap-2.5 border-b border-line px-3">
      <Tooltip.Root>
        <Tooltip.Trigger>
          {#snippet child({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              class="-ml-1 shrink-0"
              aria-label="back to all tools"
              onclick={onback}
            >
              <ArrowLeft />
            </Button>
          {/snippet}
        </Tooltip.Trigger>
        <Tooltip.Content>all tools</Tooltip.Content>
      </Tooltip.Root>

      <ToolTile icon={tool.icon} tone={tool.tone} />

      <div class="min-w-0 flex-1">
        <!-- The name is what gives way when the header runs out of room, and
             it truncates rather than wrapping — so it says the whole of
             itself on hover, as the line under it does. -->
        <h1 class="m-0 truncate text-label leading-tight font-semibold" title={tool.name}>
          {tool.name}
        </h1>
        {#if meta}
          <p class="tabular m-0 truncate text-caption leading-tight text-fg-muted" title={meta}>
            {meta}
          </p>
        {/if}
      </div>

      <div class="flex shrink-0 items-center gap-1.5">
        <QueueButton />
        <SettingsButton />
        {@render actions?.()}
      </div>
    </header>

    {@render banner?.()}

    <div bind:this={body} {onscroll} class="scroll-slim min-h-0 flex-1 overflow-y-auto px-3 py-3">
      {@render children()}
    </div>

    {#if footer}
      <div class="shrink-0 border-t border-line bg-surface px-3 py-2.5">
        {@render footer()}
      </div>
    {/if}
  </Tooltip.Provider>
</div>
