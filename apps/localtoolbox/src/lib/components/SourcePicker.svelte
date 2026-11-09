<!--
  The two places a panel's tracks can come from (the page in the active tab,
  or the session queue), as one control at the top of the two tools that can
  read either.
-->
<script lang="ts">
  import FileMusic from '@lucide/svelte/icons/file-music';
  import ListChecks from '@lucide/svelte/icons/list-checks';
  import { PAGE, QUEUE, source } from '$lib/source.ts';

  interface Props {
    /** How many tracks the queue holds, shown on its button. */
    queued?: number;
  }

  let { queued = 0 }: Props = $props();

  const OPTIONS = [
    { value: PAGE, label: 'this page', icon: FileMusic },
    { value: QUEUE, label: 'session queue', icon: ListChecks },
  ];

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
      onclick={() => (source.value = option.value)}
    >
      <Icon class="size-3.5 shrink-0" />
      <span class="truncate">{option.label}</span>
      {#if option.value === QUEUE}
        <span class="tabular {on ? '' : 'text-fg-muted'}">({queued})</span>
      {/if}
    </button>
  {/each}
</div>
