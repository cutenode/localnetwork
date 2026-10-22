<!--
  The frame of a panel that is on its way: the header it will have, and a
  shape where its body will be.

  A tool's panel is its own chunk now ($lib/tools.ts), and the page it reads
  is a couple of round trips behind that, so there is a moment between picking
  a tool and seeing it. Everything the header shows is known without loading
  anything (the name, the icon and the tone are in the registry), so the
  only part that has to be guessed at is the body, and this is the guess: rows
  the size of the rows most of these panels list.

  The way back works while it is up. Waiting for a chunk is exactly when
  someone might change their mind, and a back arrow that does nothing for a
  moment is worse than no back arrow at all.
-->
<script lang="ts">
  import ArrowLeft from '@lucide/svelte/icons/arrow-left';
  import QueueButton from './QueueButton.svelte';
  import ToolTile from './ToolTile.svelte';
  import SettingsButton from './SettingsButton.svelte';
  import type { PanelProps } from '$lib/types';

  let { tool, onback }: PanelProps = $props();

</script>

<div class="flex max-h-[580px] min-h-[200px] flex-col" aria-busy="true">
  <header class="flex h-13 shrink-0 items-center gap-2.5 border-b border-line px-3">
    <button type="button" class="icon-btn -ml-1" aria-label="back to all tools" onclick={onback}>
      <ArrowLeft class="size-4" />
    </button>

    <ToolTile icon={tool.icon} tone={tool.tone} />

    <div class="min-w-0 flex-1">
      <h1 class="m-0 truncate text-label leading-tight font-semibold">{tool.name}</h1>
      <p class="m-0 truncate text-caption leading-tight text-fg-muted">opening…</p>
    </div>

    <!-- Neither the queue nor the settings is the panel's to draw, so both are
         here too: the header the ghost puts up is the header the panel will
         have. -->
    <div class="flex shrink-0 items-center gap-1.5">
      <QueueButton />
      <SettingsButton />
    </div>
  </header>

  <div class="flex min-h-0 flex-1 flex-col gap-2 px-3 py-3" aria-hidden="true">
    {#each [0, 1, 2, 3, 4] as row (row)}
      <div class="ghost flex items-center gap-2.5 rounded-lg border border-line bg-surface p-2">
        <span class="ghost-fill size-9 shrink-0 rounded-md"></span>
        <span class="flex min-w-0 flex-1 flex-col gap-1.5">
          <span class="ghost-fill h-2.5 rounded-full" style="width: {[72, 58, 66, 50, 62][row]}%"
          ></span>
          <span class="ghost-fill h-2 rounded-full" style="width: {[40, 46, 34, 42, 38][row]}%"
          ></span>
        </span>
      </div>
    {/each}
  </div>
</div>

<style>
  /* Each row a little behind the one above it, so the list reads as one thing
     settling rather than five things flashing in step. */
  .ghost {
    animation: ghost-breathe 1.4s ease-in-out infinite;
  }
  .ghost:nth-child(2) {
    animation-delay: 0.08s;
  }
  .ghost:nth-child(3) {
    animation-delay: 0.16s;
  }
  .ghost:nth-child(4) {
    animation-delay: 0.24s;
  }
  .ghost:nth-child(5) {
    animation-delay: 0.32s;
  }

  .ghost-fill {
    display: block;
    background: var(--color-well);
  }

  @keyframes ghost-breathe {
    0%,
    100% {
      opacity: 0.55;
    }
    50% {
      opacity: 0.9;
    }
  }

  /* A pulse is decoration; someone who has asked for less motion gets the
     shape without it. */
  @media (prefers-reduced-motion: reduce) {
    .ghost {
      animation: none;
      opacity: 0.7;
    }
  }
</style>
