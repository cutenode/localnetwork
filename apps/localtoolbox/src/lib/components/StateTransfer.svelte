<!--
  Your data: everything the extension has saved, out to a JSON file and back.

  It was in the footer of the Manager panel, because the list was the biggest
  part of what the file carries. But what it carries is not one tool's — the
  marks are painted on the SoundCloud page, the looked-up details are the
  content script's cache, the session queue is behind the count in every
  header, and the options belong to three panels. A backup of all that is
  about the extension, which is what this sheet is for; and taking it here
  means it can be taken from any view rather than only from the one tool whose
  footer happened to hold it.

  The counts at the top are the point of putting it in front of somebody. An
  "export" button on its own says nothing about what would come out of it; a
  row of numbers says how much there is to lose, which is the whole argument
  for pressing it.

  Importing **adds** by default, and every part of it does: marks merge, the
  newer lookups win, listed and queued tracks go on the end without landing
  twice, and an option you have already chosen is left alone. Replacing is a
  switch you have to find and turn on, and then it is the exact opposite in
  every one of those parts — see `apply` in state-transfer.ts, which does the
  writing for the two that have no store of their own.

  The file's shape is specified in docs/manager-state.schema.json.
-->
<script lang="ts">
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import FileDown from '@lucide/svelte/icons/file-down';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import Upload from '@lucide/svelte/icons/upload';
  import StatusNote from '$lib/components/StatusNote.svelte';
  import { Button } from '$lib/components/ui/button/index.ts';
  import { Label } from '$lib/components/ui/label/index.ts';
  import { Switch } from '$lib/components/ui/switch/index.ts';
  import { Textarea } from '$lib/components/ui/textarea/index.ts';
  import { downloadText } from '$lib/clipboard.ts';
  import { details } from '$lib/details.svelte.ts';
  import { downloaded } from '$lib/downloaded.svelte.ts';
  import { transfer } from '$lib/globals.ts';
  import { reloadPrefs } from '$lib/prefs.svelte.ts';
  import { queue } from '$lib/queue.svelte.ts';
  import { count } from '$lib/soundcloud.ts';
  import { remembered } from '$lib/view.svelte.ts';
  // The two things here that belong to a tool rather than to the extension.
  // Both lists are in the file, so each has to be read for an export and
  // written for an import, and the store that owns it is the only thing
  // allowed to do either — the same rule the queue and the marks follow.
  import { listed } from '../../tools/manager/listed.svelte.ts';
  import { artists } from '../../tools/artists/artists.svelte.ts';

  // Both are read once here and followed after, so the numbers below are live:
  // a lookup made in a SoundCloud tab, or an import run in another window,
  // moves them without this sheet being reopened. Neither is awaited — the
  // strip shows zeroes for the turn it takes them to land.
  details.start();

  listed.start();

  artists.start();

  // The drawer is a place, kept for the rest of the browsing session as the
  // sheet's own openness is — and more than any other, because choosing a file
  // is itself a way of closing the popup: on some platforms the file dialog
  // takes the focus and the popup goes with it, and coming back to a shut
  // drawer means starting the import over.
  //
  // Under the sheet's own name rather than a tool's: this is not in a panel
  // any more, and there is one of it however many tools there are.
  const importing = remembered('settings', 'import', false);

  // What has been pasted in is *not* kept, unlike every other text box in the
  // extension: what goes in here is a whole state file, which can be megabytes
  // of marks and looked-up details, and it is emptied the moment it is read.
  let pasted = $state('');

  // Nor is the replace switch. It comes back off every time the popup does,
  // and that is deliberate: this is the one control here that can lose
  // something, and a popup reopened an hour later must not still be armed.
  let replacing = $state(false);

  /** What the last press did. */
  let note: { tone: 'ok' | 'bad'; text: string } | null = $state(null);

  /** The file picker, which the button opens. */
  let picker: HTMLInputElement | null = $state(null);

  const REPLACE_HINT = 'off, the file is added to what you have. on, it replaces the lot.';

  // What to show for something thrown: its message, or the thing itself when
  // whatever was thrown is not an Error, or is one with nothing to say.
  const reasonFor = (cause: unknown) => (cause instanceof Error && cause.message) || String(cause);

  /** A time in the file as the day it names, or '' for one it does not. */
  const dayOf = (when: string) => {
    const at = new Date(when);

    return Number.isNaN(at.getTime()) ? '' : at.toISOString().slice(0, 10);
  };

  // --- what is here ----------------------------------------------------------

  const HELD = [
    { label: 'marks', of: () => downloaded.count },
    { label: 'looked up', of: () => details.count },
    { label: 'listed', of: () => listed.count },
    { label: 'artists', of: () => artists.count },
    { label: 'queued', of: () => queue.count },
  ];

  // --- what is in the box ----------------------------------------------------

  // The pasted text, read as a state file, on every change to it.
  //
  // Nothing is written by this — `parse` only refuses — so it costs a refusal
  // to be wrong, and what it buys is that a file is judged when it lands
  // rather than when Import is pressed. A file chosen by mistake says so
  // straight away, and the button that would write it is off until the text
  // in the box is a file this can read.
  //
  // Derived rather than debounced on purpose. What arrives here arrives whole
  // — a chosen file, or a paste — so this runs once per file rather than once
  // per keystroke, and nobody types a backup out by hand.
  const reading = $derived.by(() => {
    if (pasted.trim() === '') return null;

    try {
      const doc = transfer().parse(pasted);

      return { doc, holds: transfer().summary(doc) };
    } catch (e) {
      return { why: reasonFor(e) };
    }
  });

  const ready = $derived(!!reading?.doc);

  /** What a file holds, as a sentence — or that it holds nothing at all. */
  function holdings(holds: ReturnType<typeof TbxState.summary>) {
    const parts = [];

    if (holds.marks > 0) parts.push(count(holds.marks, 'mark'));

    if (holds.details > 0) parts.push(count(holds.details, 'looked-up track'));

    if (holds.listed > 0) parts.push(`${count(holds.listed, 'track')} listed`);

    if (holds.artists > 0) parts.push(count(holds.artists, 'artist'));

    if (holds.queue > 0) parts.push(count(holds.queue, 'queued track'));

    if (holds.prefs > 0) parts.push(count(holds.prefs, 'option'));

    return parts.length > 0 ? parts.join(', ') : 'nothing at all';
  }

  // --- exporting -------------------------------------------------------------

  async function onExport() {
    try {
      // The queue and the list come from their stores rather than from
      // storage, so that a queue the store could only keep in memory — the
      // header is counting it either way — is in the file too. Plain entries:
      // what goes into the file cannot be a `$state` proxy.
      const snapshot = await transfer().snapshot(queue.entries, listed.entries, artists.entries);

      const name = downloadText(
        transfer().serialize(snapshot),
        transfer().filename(),
        'application/json'
      );

      note = { tone: 'ok', text: `saved ${holdings(transfer().summary(snapshot))} to ${name}.` };
    } catch (e) {
      note = { tone: 'bad', text: `couldn’t export: ${reasonFor(e)}` };
    }
  }

  // --- importing -------------------------------------------------------------

  // A chosen file fills the box, so what is about to be imported is always the
  // text on screen — and is read by the same eye before it is written.
  async function onFile(event: Event & { currentTarget: HTMLInputElement }) {
    // Null only on an input that is not a file picker.
    const [file] = event.currentTarget.files!;

    if (!file) return;

    try {
      pasted = await file.text();
    } catch (e) {
      note = { tone: 'bad', text: `couldn’t read the file: ${reasonFor(e)}` };
    }
  }

  function openDrawer() {
    importing.value = true;
    note = null;
  }

  function closeDrawer() {
    importing.value = false;
    pasted = '';
    replacing = false;
  }

  async function onImport() {
    const doc = reading?.doc;

    // Nothing but a race gets here without one — the button is off until the
    // box reads — so this says what it is rather than reporting anything.
    if (!doc) return;
    const replace = replacing;

    let result;

    try {
      result = await transfer().apply(doc, { replace });
    } catch (e) {
      note = { tone: 'bad', text: `import failed: ${reasonFor(e)}` };

      return;
    }

    // The queue and the list go through the stores that own them, as the marks
    // go through theirs: `add` appends the file's tracks behind whatever is
    // here and refuses one already held, `replace` makes each of them exactly
    // what the file says.
    const queued = replace ? await queue.replace(doc.queue) : await queue.add(doc.queue);
    const put = replace ? await listed.replace(doc.listed) : await listed.add(doc.listed);
    const who = replace ? await artists.replace(doc.artists) : await artists.add(doc.artists);

    const parts = replace
      ? [
          count(result.marks, 'mark'),
          `${count(result.details, 'track')} of download details`,
          `${count(put.added, 'track')} listed`,
          count(who.added, 'artist'),
          `${count(queued, 'track')} queued`,
        ]
      : [count(result.marks, 'new mark'), `${count(result.details, 'track')} of download details`];

    if (!replace && put.added > 0) parts.push(`${count(put.added, 'track')} onto the list`);

    if (!replace && who.added > 0) parts.push(`${count(who.added, 'artist')} onto the artists`);

    if (!replace && queued > 0) parts.push(`${count(queued, 'track')} into the queue`);

    if (result.prefs > 0) parts.push(count(result.prefs, 'option'));

    const tail =
      (put.refused > 0 ? ` ${count(put.refused, 'track')} didn’t fit on the list.` : '') +
      (who.refused > 0 ? ` ${count(who.refused, 'artist')} didn’t fit on the artists.` : '');

    note = {
      tone: 'ok',
      text:
        `${replace ? 'replaced everything with' : 'imported'} ${parts.join(', ')}.${tail} ` +
        'reload the tab to see the markers.',
    };
    closeDrawer();
    // Three panels' options may have moved under them; the one this sheet is
    // drawn over is still mounted and still showing the old ones.
    reloadPrefs();
  }
</script>

<section class="flex flex-col gap-2.5 rounded-xl border border-border/70 bg-card p-2.5">
  <div>
    <h3 class="text-[11.5px] font-semibold">your data</h3>
    <p class="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">
      everything the extension has saved, out to one file and back.
    </p>
  </div>

  <!-- What there is to lose, which is the argument for taking a backup. A
       definition list rather than four spans: each number is the answer to the
       word under it, and that is what a screen reader should hear. -->
  <dl class="grid grid-cols-5 gap-1.5">
    {#each HELD as held (held.label)}
      <div class="flex flex-col items-center gap-0.5 rounded-lg bg-muted/60 px-1 py-1.5">
        <dd class="tabular text-[14px] leading-none font-semibold">{held.of()}</dd>
        <dt class="text-center text-[10px] leading-tight text-muted-foreground">{held.label}</dt>
      </div>
    {/each}
  </dl>

  <div class="flex gap-1.5">
    <Button
      size="xs"
      variant="outline"
      class="flex-1"
      title="writes the lot to a JSON file — the one way to keep the session queue."
      onclick={onExport}
    >
      <FileDown />
      export
    </Button>
    <Button
      size="xs"
      variant="outline"
      class="flex-1"
      aria-expanded={importing.value}
      onclick={openDrawer}
    >
      <Upload />
      import…
    </Button>
  </div>

  {#if importing.value}
    <div class="flex flex-col gap-2 rounded-lg border border-border/70 bg-muted/40 p-2">
      <div class="flex items-center gap-2">
        <!-- The picker itself is never seen: a file input draws whatever the
             platform draws, which is the one control in this popup that would
             not look like the rest of it. The button is what carries the name
             and the focus ring; the input is out of the tree and out of the
             tab order behind it. -->
        <input
          bind:this={picker}
          type="file"
          class="hidden"
          tabindex="-1"
          aria-hidden="true"
          accept="application/json,.json"
          onchange={onFile}
        />
        <Button size="xs" variant="secondary" onclick={() => picker?.click()}>
          <FolderOpen />
          choose a file
        </Button>
        <span class="text-[11px] text-muted-foreground">or paste it below</span>
      </div>

      <!-- Capped, and scrolling past the cap. The box grows to its content,
           which for a state file is tens of thousands of lines: left to do
           that it would push every control here off the bottom of a popup
           that is only as tall as what is in it. -->
      <Textarea
        rows={3}
        spellcheck="false"
        autocomplete="off"
        placeholder="paste an exported state file"
        aria-label="paste an exported state file"
        aria-invalid={reading && !ready ? 'true' : undefined}
        class="max-h-28 min-h-14 resize-none overflow-y-auto font-mono text-[11px] leading-snug"
        bind:value={pasted}
      />

      {#if reading}
        <StatusNote tone={ready ? 'ok' : 'bad'} class="px-2.5 py-2 text-[11px]">
          {#if ready}
            {@const day = dayOf(reading.holds!.exportedAt)}
            {day ? `a backup from ${day}` : 'a state file'} — {holdings(reading.holds!)}.
          {:else}
            {reading.why}
          {/if}
        </StatusNote>
      {/if}

      <!-- The opt-in. Off is the whole of the promise this makes, so it draws
           as an ordinary row until it is on, and then as the thing it is. -->
      <div
        class="flex items-start gap-2.5 rounded-lg border px-2 py-1.5 transition-colors
               {replacing ? 'border-destructive/30 bg-destructive/8' : 'border-transparent'}"
      >
        <Switch id="replace-state" class="mt-0.5 shrink-0" bind:checked={replacing} />
        <div class="min-w-0 flex-1">
          <Label
            for="replace-state"
            class="text-[12px] font-normal {replacing ? 'text-destructive' : ''}"
          >
            replace what’s here instead of adding to it
          </Label>
          <p class="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">{REPLACE_HINT}</p>
        </div>
      </div>

      <div class="flex items-center gap-1.5">
        <Button
          size="xs"
          variant={replacing ? 'destructive' : 'default'}
          disabled={!ready}
          onclick={onImport}
        >
          {#if replacing}
            <Upload />
            replace everything
          {:else}
            <CircleCheck />
            add to what’s here
          {/if}
        </Button>
        <Button size="xs" variant="ghost" onclick={closeDrawer}>cancel</Button>
      </div>
    </div>
  {/if}

  {#if note}
    <p
      class="text-[11px] leading-relaxed {note.tone === 'ok'
        ? 'text-success'
        : 'text-destructive'}"
    >
      {note.text}
    </p>
  {/if}
</section>
