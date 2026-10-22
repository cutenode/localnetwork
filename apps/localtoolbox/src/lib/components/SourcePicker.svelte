<!--
  The two places a panel's tracks can come from — the page in the active tab,
  or the session queue — as one control at the top of the three tools that can
  read either.
-->
<script lang="ts">
  import FileMusic from '@lucide/svelte/icons/file-music';
  import ListChecks from '@lucide/svelte/icons/list-checks';
  import { PAGE, QUEUE } from '$lib/source.ts';
  import type { Pref } from '$lib/prefs.svelte.ts';

  interface Props {
    /** A choicePref over SOURCES; see $lib/source.js. */
    source: Pref<string>;
    /** How many tracks the queue holds, shown on its button. */
    queued?: number;
    /** Called after the source changes, for a panel that must re-read. */
    onchange?: (value: string) => void;
  }

  let {
    source,
    queued = 0,
    onchange,
  }: Props = $props();

  const OPTIONS = [
    { value: PAGE, label: 'this page', icon: FileMusic },
    { value: QUEUE, label: 'session queue', icon: ListChecks },
  ];

  function choose(value: string) {
    if (source.value === value) return;
    source.value = value;
    onchange?.(value);
  }
</script>

<div
  class="flex items-center gap-1 rounded-lg border border-line bg-surface p-1"
  role="group"
  aria-label="where the tracks come from"
>
  {#each OPTIONS as option (option.value)}
    {@const Icon = option.icon}
    {@const on = source.value === option.value}
    <button
      type="button"
      aria-pressed={on}
      class="flex flex-1 items-center justify-center gap-1.5 rounded-md px-2 py-1 text-caption font-medium transition-colors
             {on ? 'bg-fg text-fg-inverse' : 'text-fg-muted hover:text-fg'}"
      onclick={() => choose(option.value)}
    >
      <Icon class="size-3.5 shrink-0" />
      <span class="truncate">{option.label}</span>
      {#if option.value === QUEUE}
        <span class="tabular {on ? '' : 'text-fg-muted'}">({queued})</span>
      {/if}
    </button>
  {/each}
</div>
