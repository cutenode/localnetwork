<!--
  SoundCloud Playlist to Text: the tracks as a block of text, in whichever
  shape the next thing wants them — a list to paste, a CSV, JSON.

  The tracks are the page's, or the session queue's: a batch picked out in the
  Manager tool becomes a set list, a message to a friend or a row of rows in
  a spreadsheet without going back to the page it was picked from.
-->
<script lang="ts">
  import Copy from '@lucide/svelte/icons/copy';
  import Check from '@lucide/svelte/icons/check';
  import FileDown from '@lucide/svelte/icons/file-down';
  import PanelShell from '$lib/components/PanelShell.svelte';
  import StatusNote from '$lib/components/StatusNote.svelte';
  import SourcePicker from '$lib/components/SourcePicker.svelte';
  import { Button } from '$lib/components/ui/button/index.ts';
  import { Checkbox } from '$lib/components/ui/checkbox/index.ts';
  import { Label } from '$lib/components/ui/label/index.ts';
  import * as Select from '$lib/components/ui/select/index.ts';
  import * as Tooltip from '$lib/components/ui/tooltip/index.ts';
  import { copyText, downloadText } from '$lib/clipboard.ts';
  import { flagPref, choicePref } from '$lib/prefs.svelte.ts';
  import { queue } from '$lib/queue.svelte.ts';
  import { count, pageSummary, readPage } from '$lib/soundcloud.ts';
  import { PAGE, QUEUE, SOURCE_PREF, SOURCES } from '$lib/source.ts';
  import { toasts } from '$lib/toast.svelte.ts';
  import { FORMATS, FORMAT_VALUES, formatFor, processTracks, slugify } from './formats.ts';
  import type { PanelProps, TracksAnswer } from '$lib/types';

  let { tool, onback }: PanelProps = $props();

  const format = choicePref('pref:format', 'artist-title', FORMAT_VALUES);

  const source = choicePref(SOURCE_PREF, PAGE, SOURCES);

  const splitTitle = flagPref('pref:split-title', false);

  const dedupe = flagPref('pref:dedupe', false);

  /** The page's answer, once it is read. */
  let data = $state<TracksAnswer | null>(null);

  let status = $state('');

  let loading = $state(true);

  let justCopied = $state(false);

  const SPLIT_HINT =
    'plenty of uploads bury the real artist in the title. this uses that instead of the ' +
    'uploader’s name.';

  const chosen = $derived(formatFor(format.value));

  const fromQueue = $derived(source.value === QUEUE);

  // What there is to turn into text: the queue, or the page once it has been
  // read. Both are plain track objects, and everything below works on either.
  const listed = $derived(fromQueue ? queue.items : (data?.tracks ?? []));

  const tracks = $derived(
    processTracks(listed, { splitTitle: splitTitle.value, dedupe: dedupe.value })
  );

  const output = $derived(chosen.render(tracks));

  const meta = $derived.by(() => {
    if (fromQueue) return `session queue · ${count(queue.count, 'track')}`;

    return data ? pageSummary(data) : '';
  });

  const dropped = $derived(listed.length - tracks.length);

  async function readTab() {
    const result = await readPage('EXTRACT_TRACKS');

    if (result.status) status = result.status;
    else data = result.data!;
    loading = false;
  }

  void readTab();

  async function copy() {
    const copied = await copyText(output);
    justCopied = copied;
    toasts.said(copied, 'copied', 'couldn’t copy');

    if (copied) setTimeout(() => (justCopied = false), 1400);
  }

  function save() {
    // `title` is the playlist title, or "Your likes" / "Likes by <user>" on
    // Likes pages; the queue is not a page and has no title of its own.
    const stem = fromQueue ? 'session-queue' : slugify(data!.title);
    const name = downloadText(output, `${stem}.${chosen.ext}`, chosen.mime);
    toasts.ok(`saved ${name}`);
  }
</script>

<PanelShell {tool} {onback} {meta}>
  <div class="flex flex-col gap-2.5">
    <SourcePicker {source} queued={queue.count} />

    {#if fromQueue && queue.count === 0}
      <StatusNote tone="info">
        nothing queued yet. queue tracks from Manager.
      </StatusNote>
    {:else if !fromQueue && loading}
      <StatusNote tone="busy">reading the page…</StatusNote>
    {:else if !fromQueue && status}
      <StatusNote tone="bad">{status}</StatusNote>
    {:else}
      <div class="flex items-center gap-2.5">
        <span class="shrink-0 text-[11.5px] text-muted-foreground">format</span>
        <Select.Root type="single" bind:value={format.value}>
          <Select.Trigger class="h-8 flex-1 text-[12.5px]" aria-label="output format">
            {chosen.label}
          </Select.Trigger>
          <Select.Content>
            {#each FORMATS as option (option.value)}
              <Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>
            {/each}
          </Select.Content>
        </Select.Root>
      </div>

      <div class="flex flex-col gap-2">
        <Tooltip.Root>
          <Tooltip.Trigger>
            {#snippet child({ props })}
              <div {...props} class="flex items-center gap-2">
                <Checkbox id="split-title" bind:checked={splitTitle.value} />
                <Label for="split-title" class="text-[12px] font-normal">
                  pull the artist out of “Artist - Title”
                </Label>
              </div>
            {/snippet}
          </Tooltip.Trigger>
          <Tooltip.Content class="max-w-72">{SPLIT_HINT}</Tooltip.Content>
        </Tooltip.Root>

        <div class="flex items-center gap-2">
          <Checkbox id="dedupe" bind:checked={dedupe.value} />
          <Label for="dedupe" class="text-[12px] font-normal">drop duplicates</Label>
        </div>
      </div>

      <div class="overflow-hidden rounded-xl border border-border/70 bg-card">
        <div
          class="flex items-center justify-between border-b border-border/70 bg-muted/40 px-2.5 py-1.5"
        >
          <span class="tabular text-[11px] text-muted-foreground">
            {tracks.length} line{tracks.length === 1 ? '' : 's'}
            {#if dropped > 0}· {dropped} duplicate{dropped === 1 ? '' : 's'} removed{/if}
          </span>
          <span class="font-mono text-[10px] tracking-wide text-muted-foreground uppercase">
            .{chosen.ext}
          </span>
        </div>
        <textarea
          readonly
          spellcheck="false"
          aria-label="the tracks as text"
          class="scroll-slim h-56 w-full resize-y bg-transparent p-2.5 font-mono text-[11.5px] leading-relaxed whitespace-pre outline-none"
          value={output}
        ></textarea>
      </div>

      <div class="flex gap-2">
        <Button class="flex-1" onclick={copy}>
          {#if justCopied}<Check />copied{:else}<Copy />copy{/if}
        </Button>
        <Button variant="outline" class="flex-1" onclick={save}>
          <FileDown />
          download
        </Button>
      </div>
    {/if}
  </div>
</PanelShell>
