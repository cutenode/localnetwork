<!--
  The frame of a panel that is on its way: the header it will have, and a
  shape where its body will be.

  A tool's panel is its own chunk now ($lib/tools.ts), and the page it reads
  is a couple of round trips behind that, so there is a moment between picking
  a tool and seeing it. Everything the header shows is known without loading
  anything — the name, the icon and the colour are in the registry — so the
  only part that has to be guessed at is the body, and this is the guess: rows
  the size of the rows most of these panels list.

  The way back works while it is up. Waiting for a chunk is exactly when
  someone might change their mind, and a back arrow that does nothing for a
  moment is worse than no back arrow at all.
-->
<script lang="ts">
  import ArrowLeft from '@lucide/svelte/icons/arrow-left';
  import QueueButton from './QueueButton.svelte';
  import SettingsButton from './SettingsButton.svelte';
  import type { PanelProps } from '$lib/types';

  let { tool, onback }: PanelProps = $props();

  const Icon = $derived(tool.icon);
</script>

<div class="flex max-h-[580px] min-h-[200px] flex-col" aria-busy="true">
  <header
    class="brand-wash flex shrink-0 items-center gap-2.5 border-b border-border/70 px-3 py-2.5"
  >
    <button
      type="button"
      class="-ml-1 grid size-7 shrink-0 place-items-center rounded-lg text-muted-foreground
             outline-none hover:bg-muted hover:text-foreground focus-visible:ring-3
             focus-visible:ring-ring/50"
      aria-label="back to all tools"
      onclick={onback}
    >
      <ArrowLeft class="size-4" />
    </button>

    <div
      class="brand-tile grid size-8 shrink-0 place-items-center rounded-[10px] text-white"
      aria-hidden="true"
    >
      <Icon class="size-4" />
    </div>

    <div class="min-w-0 flex-1">
      <h1 class="truncate text-[13px] leading-tight font-semibold">{tool.name}</h1>
      <p class="truncate text-[11px] leading-tight text-muted-foreground">opening…</p>
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
      <div class="ghost flex items-center gap-2.5 rounded-xl border border-border/70 bg-card p-2">
        <span class="ghost-fill size-9 shrink-0 rounded-[10px]"></span>
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
    background: color-mix(in oklch, var(--muted-foreground) 22%, transparent);
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
