<!--
  What the popup opens on: the list of tools, one row each.

  Drawn the way the website draws its pages: a bar with the waveform mark and
  the name, a mono label over the group, and bordered rows that answer the
  pointer with a stronger border. Each tool's tone is its icon tile and nothing
  else (see $lib/tools.ts).
-->
<script lang="ts">
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import QueueButton from './QueueButton.svelte';
  import SettingsButton from './SettingsButton.svelte';
  import ThemeToggle from './ThemeToggle.svelte';
  import ToolTile from './ToolTile.svelte';
  import { TOOLS } from '$lib/tools.ts';

  let { onopen }: { onopen: (id: string) => void } = $props();

  /** The website's mark: five waveform bars in the band colours. */
  const BARS = [
    ['h-1.5', 'bg-band-low'],
    ['h-3.5', 'bg-band-low'],
    ['h-2.5', 'bg-band-mid'],
    ['h-4', 'bg-band-high'],
    ['h-2', 'bg-band-low'],
  ];
</script>

<div class="flex flex-col">
  <header class="flex h-13 items-center gap-1 border-b border-line px-3">
    <h1 class="m-0 flex min-w-0 flex-1 items-center gap-2 text-emphasis font-semibold">
      <span aria-hidden="true" class="flex h-4 items-center gap-0.5">
        {#each BARS as [height, colour], i (i)}
          <span class="w-0.5 rounded-[1px] {height} {colour}"></span>
        {/each}
      </span>
      localtoolbox
    </h1>
    <QueueButton />
    <SettingsButton />
    <!-- The theme is in the settings sheet too, and stays here: it is the one
         setting worth a single click, and the menu has room for it. -->
    <ThemeToggle />
  </header>

  <nav class="flex flex-col gap-2 p-3" aria-label="tools">
    <p aria-hidden="true" class="section-title m-0 mb-0.5">pick a tool</p>
    {#each TOOLS as tool (tool.id)}
      <button
        type="button"
        class="group flex items-center gap-3 rounded-lg border border-line bg-surface p-2.5 text-left
               transition-colors hover:border-line-strong"
        onclick={() => onopen(tool.id)}
      >
        <ToolTile icon={tool.icon} tone={tool.tone} size="md" />

        <span class="min-w-0 flex-1">
          <span class="block truncate text-small font-semibold">{tool.name}</span>
          <span class="block text-caption text-fg-muted">{tool.description}</span>
        </span>

        <ChevronRight class="size-4 shrink-0 text-fg-subtle transition-colors group-hover:text-fg" />
      </button>
    {/each}
  </nav>
</div>
