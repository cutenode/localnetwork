<!--
  The search box and the "Hide downloaded" toggle, with a line underneath that
  only appears when something is being hidden — and says so in words when
  nothing is left.
-->
<script lang="ts">
  import Search from '@lucide/svelte/icons/search';
  import X from '@lucide/svelte/icons/x';
  import { Input } from '$lib/components/ui/input/index.ts';
  import { Switch } from '$lib/components/ui/switch/index.ts';
  import { Label } from '$lib/components/ui/label/index.ts';
  import * as Tooltip from '$lib/components/ui/tooltip/index.ts';
  import type { createFilter } from '$lib/list-filter.svelte.ts';

  interface Props {
    /** From createFilter() in $lib/list-filter.svelte.js. */
    filter: ReturnType<typeof createFilter>;
    shown?: number;
    total?: number;
    id?: string;
  }

  let {
    filter,
    shown = 0,
    total = 0,
    id = 'hide-downloaded',
  }: Props = $props();

  const HIDE_HINT = 'hides tracks already marked downloaded.';

  const countLine = $derived.by(() => {
    if (shown === total) return '';

    if (shown > 0) return `${shown} of ${total} shown`;

    if (filter.query.trim()) return `nothing matches “${filter.query.trim()}”.`;

    return total === 1
      ? 'the only track is marked downloaded.'
      : `all ${total} are marked downloaded.`;
  });
</script>

<div class="flex flex-col gap-1.5">
  <div class="flex items-center gap-2.5">
    <div class="relative min-w-0 flex-1">
      <Search
        class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground"
      />
      <Input
        type="search"
        class="h-8 pr-7 pl-8 text-[12.5px] [&::-webkit-search-cancel-button]:hidden"
        placeholder="search title or artist"
        autocomplete="off"
        spellcheck="false"
        aria-label="search the list"
        bind:value={filter.query}
      />
      {#if filter.query}
        <button
          type="button"
          class="absolute top-1/2 right-1.5 grid size-5 -translate-y-1/2 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          aria-label="clear the search"
          onclick={() => (filter.query = '')}
        >
          <X class="size-3" />
        </button>
      {/if}
    </div>

    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <div {...props} class="flex shrink-0 items-center gap-1.5">
            <Switch {id} bind:checked={filter.hideDownloaded} />
            <Label for={id} class="text-[11.5px] whitespace-nowrap text-muted-foreground">
              hide downloaded
            </Label>
          </div>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content class="max-w-64">{HIDE_HINT}</Tooltip.Content>
    </Tooltip.Root>
  </div>

  {#if countLine}
    <p class="tabular text-[11.5px] text-muted-foreground">{countLine}</p>
  {/if}
</div>
