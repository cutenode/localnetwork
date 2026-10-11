<!--
  What the popup opens on: the list of tools, one row each.

  Every row carries its own `--brand`, so the icon tiles read as four
  differently-coloured things belonging to one set, and picking one paints the
  whole popup in that colour (see $lib/tools.ts).
-->
<script lang="ts">
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import QueueButton from './QueueButton.svelte';
  import SettingsButton from './SettingsButton.svelte';
  import ThemeToggle from './ThemeToggle.svelte';
  import { TOOLS } from '$lib/tools.ts';

  let { onopen }: { onopen: (id: string) => void } = $props();
</script>

<div class="flex flex-col">
  <header class="brand-wash flex items-center gap-1.5 border-b border-border/70 px-3 py-2.5">
    <div class="min-w-0 flex-1">
      <h1 class="text-[13px] leading-tight font-semibold tracking-tight">localtoolbox</h1>
      <p class="text-[11px] leading-tight text-muted-foreground">pick a tool</p>
    </div>
    <QueueButton />
    <SettingsButton />
    <!-- The theme is in the settings sheet too, and stays here: it is the one
         setting worth a single click, and the menu has room for it. -->
    <ThemeToggle />
  </header>

  <nav class="flex flex-col gap-1.5 p-2.5" aria-label="tools">
    {#each TOOLS as tool (tool.id)}
      {@const Icon = tool.icon}
      <button
        type="button"
        style="--brand: {tool.brand}"
        class="lift group flex items-center gap-3 rounded-xl border border-border/70 bg-card px-2.5 py-2.5 text-left
               outline-none hover:border-border focus-visible:ring-3 focus-visible:ring-ring/50"
        onclick={() => onopen(tool.id)}
      >
        <span
          class="brand-tile grid size-9 shrink-0 place-items-center rounded-[11px] text-white"
          aria-hidden="true"
        >
          <Icon class="size-[18px]" />
        </span>

        <span class="min-w-0 flex-1">
          <span class="block truncate text-[13px] font-semibold">{tool.name}</span>
          <span class="block text-[11.5px] leading-snug text-muted-foreground">
            {tool.description}
          </span>
        </span>

        <ChevronRight
          class="size-4 shrink-0 text-muted-foreground transition-transform duration-150 group-hover:translate-x-0.5 group-hover:text-foreground"
        />
      </button>
    {/each}
  </nav>
</div>
