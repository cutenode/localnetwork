<!--
  Adding a search site of your own, or editing one: the settings sheet's form
  under the list of badges, in the shape search-sites.schema.json gives a
  shipped one.

  What is typed is kept in `draft`, which the sheet remembers for the session
  as it remembers the import drawer: the address to type here is copied out
  of a shop's own tab, and going to that tab closes the popup.

  The address is judged as it is typed, by the same reading the shipped file
  gets (readTemplate), so a template this refuses is one a row could not use
  either, and the line under the field says why.
-->
<script lang="ts">
  import { untrack } from 'svelte';
  import Check from '@lucide/svelte/icons/check';
  import { Button } from '$lib/components/ui/button/index.ts';
  import { Input } from '$lib/components/ui/input/index.ts';
  import { Label } from '$lib/components/ui/label/index.ts';
  import { readTemplate, VARIABLE_LIST } from '$lib/search-sites.ts';
  import { searchSites } from '$lib/search-sites.svelte.ts';
  import type { Remembered } from '$lib/view.svelte.ts';

  interface Props {
    /**
     * The form, as the sheet remembers it: a site's id (empty for a new one),
     * name, badge, address and two colours, or nothing while it is shut.
     */
    draft: Remembered<string[]>;
  }

  let { draft }: Props = $props();

  // Read once: from here the form owns it, and writes every change back.
  const [id, name, badge, address, background, color] = untrack(() => draft.value);

  const form = $state({ id, name, badge, url: address, background, color });

  // Untracked, because writing a remembered value reads every other one: done
  // tracked, this effect would follow the whole sheet's places and rerun on
  // its own write.
  $effect(() => {
    const typed = [form.id, form.name, form.badge, form.url, form.background, form.color];

    untrack(() => (draft.value = typed));
  });

  const url = $derived(form.url.trim());

  const template = $derived(url === '' ? null : readTemplate(url));

  const taken = $derived(searchSites.all.some((site) => site.id !== form.id && site.url === url));

  const problem = $derived(
    template && 'problem' in template
      ? template.problem
      : taken
        ? 'that search is already a badge.'
        : '',
  );

  const ready = $derived(
    form.name.trim() !== '' && form.badge.trim() !== '' && template !== null && !problem,
  );

  function save() {
    const { id: editing, ...entry } = $state.snapshot(form);

    if (editing) searchSites.edit(editing, entry);
    else searchSites.add([{ ...entry, id: crypto.randomUUID() }]);

    draft.value = [];
  }
</script>

<form
  class="flex flex-col gap-1.5 rounded-md border border-line bg-canvas p-2"
  aria-label={form.id ? 'edit search site' : 'add a search site'}
  novalidate
  onsubmit={(event) => {
    event.preventDefault();
    if (ready) save();
  }}
>
  <div class="flex gap-2">
    <div class="flex min-w-0 flex-1 flex-col gap-1.5">
      <Label for="site-name" class="text-caption text-fg-muted">name</Label>
      <Input
        id="site-name"
        placeholder="Juno"
        autocomplete="off"
        class="h-8"
        bind:value={form.name}
      />
    </div>
    <div class="flex w-16 flex-col gap-1.5">
      <Label for="site-badge" class="text-caption text-fg-muted">badge</Label>
      <Input
        id="site-badge"
        placeholder="J"
        maxlength={3}
        autocomplete="off"
        spellcheck="false"
        class="h-8"
        bind:value={form.badge}
      />
    </div>
  </div>

  <Label for="site-url" class="text-caption text-fg-muted">search address</Label>
  <Input
    id="site-url"
    type="url"
    placeholder="https://shop.example/search?q={'{artist}'}"
    autocomplete="off"
    spellcheck="false"
    class="h-8 font-mono text-caption"
    aria-invalid={problem ? 'true' : undefined}
    aria-describedby="site-url-hint"
    bind:value={form.url}
  />
  <p
    id="site-url-hint"
    class="m-0 text-caption leading-relaxed {problem ? 'text-critical' : 'text-fg-muted'}"
  >
    {problem || `${VARIABLE_LIST} is filled in from the track.`}
  </p>

  <div class="flex items-center gap-2">
    <!-- The badge as a row will draw it, so the colours are picked
         against the letters they are for. -->
    <span
      class="grid size-5 shrink-0 place-items-center rounded text-[9px] font-bold"
      style="background:{form.background};color:{form.color}"
      aria-hidden="true">{form.badge.trim()}</span
    >
    <Label for="site-background" class="text-caption font-normal text-fg-muted">colour</Label>
    <input
      id="site-background"
      type="color"
      class="h-7 w-9 cursor-pointer rounded-sm border border-line-strong bg-raised p-0.5"
      bind:value={form.background}
    />
    <Label for="site-color" class="text-caption font-normal text-fg-muted">text</Label>
    <input
      id="site-color"
      type="color"
      class="h-7 w-9 cursor-pointer rounded-sm border border-line-strong bg-raised p-0.5"
      bind:value={form.color}
    />
  </div>

  <div class="flex items-center gap-1.5">
    <Button type="submit" size="sm" disabled={!ready}>
      <Check />
      save
    </Button>
    <Button size="sm" variant="ghost" onclick={() => (draft.value = [])}>cancel</Button>
  </div>
</form>
