<!--
  The shell both sheets come up in (QueueSheet.svelte, SettingsSheet.svelte):
  the ground behind, the panel sliding up from the bottom, its header with a
  way out, and one scrolling body.

  It is also what moves focus, since a sheet is a modal and the view under it
  is shut away (App.svelte makes it inert): focus goes into the sheet as it
  comes up, so a keyboard or a screen reader is where the sheet is, and goes
  back to whatever opened it when it is put away.

  It carries its own Tooltip.Provider because a sheet is rendered at the root
  of the app rather than inside PanelShell, which is where every other tooltip
  in the extension lives.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { LucideIcon } from '@lucide/svelte';
  import { fly, fade } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import X from '@lucide/svelte/icons/x';
  import ToolTile from '$lib/components/ToolTile.svelte';
  import { Button } from '$lib/components/ui/button/index.ts';
  import * as Tooltip from '$lib/components/ui/tooltip/index.ts';
  import { motion } from '$lib/motion.ts';

  interface Props {
    /** The sheet's name, as its heading. */
    title: string;
    icon: LucideIcon;
    /** The line under the name. */
    meta: string;
    /** The close button's name: "close the queue". */
    closeLabel: string;
    /** Puts the sheet away, leaving the view under it where it was. */
    onclose: () => void;
    /** Buttons for the header, left of the close button. */
    actions?: Snippet;
    /** The scrolling body, for a sheet that remembers how far down it was. */
    body?: HTMLElement | null;
    onscroll?: () => void;
    children: Snippet;
  }

  let {
    title,
    icon,
    meta,
    closeLabel,
    onclose,
    actions,
    body = $bindable(null),
    onscroll,
    children,
  }: Props = $props();

  // Whatever had focus as the sheet came up, which is the button that opened
  // it unless the popup opened on a sheet left up last time.
  let opener = document.activeElement instanceof HTMLElement && document.activeElement !== document.body
    ? document.activeElement
    : null;

  /** Puts focus back on the opener, once, if it is still there to take it. */
  function restore() {
    const target = opener;
    opener = null;

    if (target?.isConnected && !target.closest('[inert]')) target.focus();
  }

  // After the flush that puts the sheet away: until then the view the opener
  // is in is still inert, and an inert button cannot take focus.
  function close() {
    onclose();
    queueMicrotask(restore);
  }

  // A sheet put away some other way (the queue handing its batch to a tool)
  // still gives focus back, if there is anything left to give it to.
  $effect(() => restore);

  // Escape closes it, wherever the focus is: a sheet over the whole popup has
  // to answer the key that puts one away.
  function onkeydown(event: KeyboardEvent) {
    if (event.key !== 'Escape') return;
    event.stopPropagation();
    close();
  }

  const focus = (node: HTMLElement) => node.focus();
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
      class="absolute inset-0 bg-black/50"
      transition:fade={motion({ duration: 150 })}
      onclick={close}
    ></button>

    <!-- Focused itself rather than its first control, which in the queue is
         "clear": coming up, a screen reader reads the sheet's name. -->
    <div
      role="dialog"
      aria-modal="true"
      aria-label={title}
      tabindex="-1"
      class="relative flex max-h-[85vh] flex-col rounded-t-xl border-t border-line-strong bg-canvas outline-none"
      transition:fly={motion({ y: 240, duration: 240, easing: cubicOut, opacity: 1 })}
      {@attach focus}
    >
      <header class="flex h-13 shrink-0 items-center gap-2.5 border-b border-line px-3">
        <ToolTile {icon} />

        <div class="min-w-0 flex-1">
          <h2 class="m-0 truncate text-label leading-tight font-semibold">{title}</h2>
          <p class="tabular m-0 truncate text-caption leading-tight text-fg-muted" title={meta}>
            {meta}
          </p>
        </div>

        <div class="flex shrink-0 items-center gap-1.5">
          {@render actions?.()}

          <!-- The browser's own tooltip rather than the shell's, which is what
               the back arrow in a panel header has. This is the only way out
               of a sheet, and a tooltip trigger wraps the button: the one
               control a screen reader has to find here is better off being
               the button itself. -->
          <Button
            variant="ghost"
            size="icon-sm"
            class="-mr-1"
            aria-label={closeLabel}
            title="close"
            onclick={close}
          >
            <X />
          </Button>
        </div>
      </header>

      <div bind:this={body} {onscroll} class="scroll-slim min-h-0 flex-1 overflow-y-auto px-3 py-3">
        {@render children()}
      </div>
    </div>
  </div>
</Tooltip.Provider>
