<!--
  The popup: the tool menu, and whichever tool is open on top of it.

  The tools used to be a page each, which the popup window navigated between:
  a reload — and a fresh round of lookups — every time one went back. They are
  one app now: picking a tool swaps the view, going back is instant, and the
  marks read from storage once are shared by all of them.

  Two things here are not tools. The session queue is about the others rather
  than beside them, and the settings are about the extension rather than any
  one of them; both are reached from buttons in the header of every view and
  drawn by this component as a sheet over whichever tool is open (see
  QueueButton.svelte and QueueSheet.svelte, SettingsButton.svelte and
  SettingsSheet.svelte). Only one sheet is ever up — $lib/view.svelte.ts sees
  to that — so this draws whichever it is.

  Which tool is open is not this component's own state but the session's (see
  $lib/view.svelte.ts): the popup closes on every click that opens a tab, and
  it comes back where it was rather than at the menu.

  What is *drawn* for it, though, is this component's business, and the order
  it does things in is the whole of how fast the popup feels:

    1. The route is known synchronously (the localStorage mirror in
       view.svelte.ts), so the colour is painted and the header drawn on the
       first turn — nothing is ever blank while a browser API is asked.
    2. The tool's chunk and the page it will read are both started right
       then, at the same time, rather than one after the panel has mounted.
    3. A ghost of the panel's body stands in until the chunk has arrived and
       the session has been read. Panels read their remembered values as they
       mount, so mounting one before the session is back would lose the place
       it is meant to restore.
-->
<script lang="ts">
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import ToolMenu from '$lib/components/ToolMenu.svelte';
  import PanelGhost from '$lib/components/PanelGhost.svelte';
  import Toaster from '$lib/components/Toaster.svelte';
  import { loadQueueSheet, type Sheet as QueueSheet } from '$lib/queue-sheet.ts';
  import { loadSettingsSheet, type Sheet as SettingsSheet } from '$lib/settings-sheet.ts';
  import { toolById, loadPanel, type PanelComponent } from '$lib/tools.ts';
  import { theme } from '$lib/theme.svelte.ts';
  import { downloaded } from '$lib/downloaded.svelte.ts';
  import { queue } from '$lib/queue.svelte.ts';
  import { settings } from '$lib/settings.svelte.ts';
  import { view } from '$lib/view.svelte.ts';

  const tool = $derived(view.open ? toolById(view.open) : null);

  theme.start();

  // Every panel shows the marks, and two of them make them; reading the map
  // once here means going back to the menu and into another tool does not
  // read it again. The session queue is read the same way, once, and shared:
  // one tool fills it, two of them work from it, and the count in every
  // header comes from it. None of the three is awaited — what they read is
  // drawn when it arrives, and the popup is on screen long before then.
  downloaded.start();

  queue.start();

  view.start();

  // The settings the content script obeys are read here too, so the sheet has
  // them the moment it is opened rather than a round trip after. Nothing in
  // the popup waits on them: the switches show the defaults until they land,
  // and the page has been reading them for itself since it loaded.
  settings.start();

  // --- the open tool's panel -------------------------------------------------

  /** The panel whose chunk is here. */
  let Panel = $state<PanelComponent | null>(null);

  /** Which tool `Panel` is, so a chunk that lands late is not shown late. */
  let loaded = $state<string | null>(null);

  /** Why the chunk could not be loaded, in a sentence. */
  let failed = $state('');

  /** Hands a chunk to `take` once it is here, or says `why` if it never comes. */
  async function arrive<T>(chunk: Promise<T>, take: (component: T) => void, why: string) {
    try {
      take(await chunk);
    } catch (_e) {
      failed = why;
    }
  }

  // Fetching the chunk, and starting the slow thing the panel will do first
  // (see `warm` in $lib/tools.js). Both begin the moment the route is known,
  // which with the synchronous mirror is before the first paint.
  $effect(() => {
    const opening = tool;
    // Cleared here rather than where it is set, so leaving a tool that would
    // not open — the one thing there is to do about one — takes its message
    // with it. Nothing reads `failed` in this effect, so writing it starts
    // nothing off again.
    failed = '';

    if (!opening || loaded === opening.id) return;
    opening.warm?.();
    const chunk = loadPanel(opening.id);

    if (!chunk) return;
    void arrive(
      chunk,
      (component) => {
        Panel = component;
        loaded = opening.id;
      },
      `couldn’t open ${opening.name}. close the popup and try again.`
    );
  });

  // The panel goes up once its chunk is here *and* the session has been read,
  // because that is what it restores its own place from.
  const showing = $derived(tool && Panel && loaded === tool.id && view.ready ? Panel : null);

  // --- the queue sheet -------------------------------------------------------

  // The queue is reached from the header of every view rather than from the
  // menu, so what draws it is here rather than in any one panel — and it is a
  // chunk of its own, for the reason the panels are: it brings the component
  // library with it, and the tool menu the popup boots into is deliberately
  // free of that.
  //
  // Fetched before it is asked for, but only once there is a reason to: a
  // queue with something in it is one that will be opened, and a sheet left
  // open in this session is about to be drawn. An empty queue on a first
  // opening fetches nothing until the button is pressed.

  /** The sheet, once its chunk is here. */
  let Sheet = $state<QueueSheet | null>(null);

  $effect(() => {
    if (Sheet || (queue.count === 0 && !view.queueOpen)) return;
    void arrive(
      loadQueueSheet(),
      (component) => {
        Sheet = component;
      },
      'couldn’t open the queue. close the popup and try again.'
    );
  });

  const sheet = $derived(view.queueOpen ? Sheet : null);

  // --- the settings sheet ----------------------------------------------------

  // A chunk of its own for the reason the queue sheet's is, and fetched on a
  // simpler rule: there is no "it will be opened" to read off a count, so it
  // is fetched when it is asked for (or when this session left it open).

  /** The sheet, once its chunk is here. */
  let SettingsPanel = $state<SettingsSheet | null>(null);

  $effect(() => {
    if (SettingsPanel || !view.settingsOpen) return;
    void arrive(
      loadSettingsSheet(),
      (component) => {
        SettingsPanel = component;
      },
      'couldn’t open settings. close the popup and try again.'
    );
  });

  const settingsSheet = $derived(view.settingsOpen ? SettingsPanel : null);

  /** Whether anything is up over the open view. */
  const covered = $derived(!!sheet || !!settingsSheet);

  /** The view the sheet comes up over. */
  let behind: HTMLElement | null = $state(null);

  // `inert` as an attribute rather than as `inert={…}`: Svelte sets it as a
  // DOM property, which a browser reflects onto the element and jsdom does
  // not — so the tests that check the view behind the sheet really is shut
  // away would be checking nothing. Toggling the attribute is what every
  // reader of it, here and in the browser, actually looks at.
  $effect(() => {
    behind?.toggleAttribute('inert', covered);
  });

  const IN = { duration: 220, easing: cubicOut };

  const OUT = { duration: 150, easing: cubicOut };

  // A view slides in when it is a navigation — the user picked a tool, or came
  // back — and is simply there when it is the popup opening on what was left
  // open. Both go through these two handlers, so setting the flag in them
  // covers every case; it is read as the transition is created, one flush
  // after the click, and never needs to be reactive.
  let navigating = false;

  const enter = () => (navigating ? IN : { duration: 0 });

  function open(id: string) {
    navigating = true;
    view.open = id;
  }

  function back() {
    navigating = true;
    view.open = null;
  }
</script>

<!-- Shut away from the keyboard and from screen readers while the sheet is
     over it: a sheet that can be tabbed out of behind is not a sheet.

     And given a floor while it is: a browser-action popup is only as tall as
     its content, and the sheet is `fixed`, which is to say out of the flow and
     no part of that content. Without this the queue opened over the tool menu
     would get whatever height four menu rows happen to make. What is behind
     the sheet is covered anyway, so the floor costs the view under it
     nothing. -->
<main bind:this={behind} class="relative overflow-hidden {covered ? 'min-h-[420px]' : ''}">
  {#if tool}
    {#key tool.id}
      <div in:fly={{ x: 18, ...enter() }} out:fly={{ x: 18, ...OUT }}>
        {#if showing}
          {@const Open = showing}
          <Open {tool} onback={back} />
        {:else}
          <PanelGhost {tool} onback={back} />
        {/if}
      </div>
    {/key}
  {:else}
    <div in:fly={{ x: -18, ...enter() }} out:fly={{ x: -18, ...OUT }}>
      <ToolMenu onopen={open} />
    </div>
  {/if}
</main>

{#if sheet}
  {@const Queue = sheet}
  <!-- `onopen` is how the queue hands a batch to another tool: it sets the
       tool's source to the queue, puts the sheet away and opens it. -->
  <Queue onclose={() => (view.queueOpen = false)} onopen={open} />
{/if}

{#if settingsSheet}
  {@const Options = settingsSheet}
  <Options onclose={() => (view.settingsOpen = false)} />
{/if}

{#if failed}
  <p role="alert" class="px-3 pb-3 text-caption text-critical">{failed}</p>
{/if}

<Toaster />

<style>
  /* Both views are in the DOM while one replaces the other; stacking them
     keeps the popup from jumping to their combined height mid-transition. */
  main :global(> div) {
    grid-area: 1 / 1;
    min-width: 0;
  }

  main {
    display: grid;
    /* minmax(0, …) rather than the default `auto`: a grid track sized to its
       content would let one long word widen the whole popup. */
    grid-template-columns: minmax(0, 1fr);
  }
</style>
