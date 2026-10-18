<!--
  Settings: the handful of choices that belong to the extension rather than to
  any one tool.

  A panel's own options stay on the panel — the sort order and "background
  tabs" say how the Manager list behaves and are read a foot from where they
  are set. What is here is the other kind: the theme, which is every view at
  once; what the content script does to a SoundCloud page, which is somewhere
  the popup is not (see $lib/settings.svelte.ts for why those cannot live in
  the popup's own storage); and what the Downloader hands to a gate that asks
  for it, which is typed once and never thought about again
  ($lib/downloader.svelte.ts); and which shops the Manager's search badges
  point at, which is a fact about where somebody buys records rather than
  about the list in front of them ($lib/search-sites.svelte.ts, over the sites
  named in $lib/search-sites.json); and whether an opened row's words are put
  into Title Case, which is a house style for a tracklist rather than a fact
  about any one of them ($lib/title-case.svelte.ts, over $lib/title-case.ts).

  So the two kinds of storage both turn up here, and that is the point rather
  than a muddle: what a setting *is* decides where it is set, and where it is
  kept is a question about who has to read it. The switches at the top are in
  chrome.storage.local because a SoundCloud page obeys them; the address, the
  name, the badge switches and the casing below are in the popup's own
  localStorage because nothing outside the popup has any use for them.

  One section is not a setting at all. Everything the extension has saved
  — the marks, the looked-up details, the Manager's list, the session queue and
  every panel option — goes out to one file there and reads back in
  (StateTransfer.svelte). It was in the Manager panel's footer, which was the
  wrong place for it twice over: the file carries four other tools' worth of
  state, and a backup is something you take before opening a playlist rather
  than while looking at one. Nor is the last: which version this is, and a
  link to report a bug with that version already filled in ($lib/bug-report.ts),
  since this sheet is about the extension rather than any one tool.

  A sheet rather than a tool, and for the reason the session queue is one: it
  is about the tools rather than beside them. It comes up over whichever one is
  open, from a button in the header of every view (SettingsButton.svelte), and
  goes away leaving that tool exactly as it was.

  It carries its own Tooltip.Provider because it is rendered at the root of the
  app rather than inside PanelShell, which is where every other tooltip in the
  extension lives.
-->
<script lang="ts">
  import { fly, fade } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Bug from '@lucide/svelte/icons/bug';
  import Monitor from '@lucide/svelte/icons/monitor';
  import Moon from '@lucide/svelte/icons/moon';
  import Settings from '@lucide/svelte/icons/settings';
  import Sun from '@lucide/svelte/icons/sun';
  import X from '@lucide/svelte/icons/x';
  import StateTransfer from '$lib/components/StateTransfer.svelte';
  import { Button } from '$lib/components/ui/button/index.ts';
  import { Input } from '$lib/components/ui/input/index.ts';
  import { Label } from '$lib/components/ui/label/index.ts';
  import { Switch } from '$lib/components/ui/switch/index.ts';
  import * as Tooltip from '$lib/components/ui/tooltip/index.ts';
  import { bugReportUrl, extensionVersion } from '$lib/bug-report.ts';
  import { downloader } from '$lib/downloader.svelte.ts';
  import { searchSites } from '$lib/search-sites.svelte.ts';
  import { settings } from '$lib/settings.svelte.ts';
  import { theme } from '$lib/theme.svelte.ts';
  import { titleCasing } from '$lib/title-case.svelte.ts';
  import { view } from '$lib/view.svelte.ts';

  interface Props {
    /** Puts the sheet away, leaving the view under it where it was. */
    onclose: () => void;
  }

  let {
    onclose,
  }: Props = $props();

  const REPLACE_HINT =
    'replaces Free Download / Buy and More → Download file — ours queues the track first.';

  const LIKES_VIEW_HINT =
    'the Badges grid shows no genre, no like button and no download link; this presses List.';

  const EMAIL_HINT =
    'only Hypeddit and Laylo ask for these, and both hand them to the artist — a Laylo drop ' +
    'signs you up for their mailing list.';

  const MARK_HINT = 'off, saved files go unmarked — mark them by hand from the row.';

  const SEARCH_HINT = 'tracks with no download link get one badge per site.';

  const NO_SEARCH_HINT = 'all off — those tracks get no badges.';

  const TITLE_CASE_HINT =
    'in an opened row, and in what a click off one copies. capitals somebody typed are kept ' +
    'as typed; a field shouted in capitals is re-cased.';

  const badges = $derived(searchSites.all);

  const version = extensionVersion();

  // The tool under the sheet, if any, is the part a report is most likely about.
  const reportUrl = $derived(bugReportUrl(view.open));

  const THEMES = [
    { mode: 'system', icon: Monitor, label: 'system' },
    { mode: 'light', icon: Sun, label: 'light' },
    { mode: 'dark', icon: Moon, label: 'dark' },
  ];

  // Escape closes it, wherever the focus is: a sheet over the whole popup has
  // to answer the key that puts one away.
  function onkeydown(event: KeyboardEvent) {
    if (event.key !== 'Escape') return;
    event.stopPropagation();
    onclose();
  }
</script>

<svelte:window {onkeydown} />

<Tooltip.Provider delayDuration={350}>
  <div class="fixed inset-0 z-40 flex flex-col justify-end">
    <!-- The ground behind the sheet. A click on it puts the sheet away, which
         is a shortcut rather than the way out: the close button in the header
         is what a keyboard uses, so this is hidden from the tree entirely. -->
    <button
      type="button"
      tabindex="-1"
      aria-hidden="true"
      class="absolute inset-0 bg-foreground/30 backdrop-blur-[1px]"
      transition:fade={{ duration: 150 }}
      onclick={onclose}
    ></button>

    <div
      role="dialog"
      aria-modal="true"
      aria-label="settings"
      class="relative flex max-h-[85vh] flex-col rounded-t-2xl border-t border-border/70
             bg-background shadow-[0_-8px_30px_oklch(0_0_0/12%)]"
      transition:fly={{ y: 240, duration: 240, easing: cubicOut, opacity: 1 }}
    >
      <header
        class="brand-wash flex shrink-0 items-center gap-2.5 rounded-t-2xl border-b
               border-border/70 px-3 py-2.5"
      >
        <div
          class="brand-tile grid size-8 shrink-0 place-items-center rounded-[10px] text-white"
          aria-hidden="true"
        >
          <Settings class="size-4" />
        </div>

        <div class="min-w-0 flex-1">
          <h2 class="truncate text-[13px] leading-tight font-semibold">settings</h2>
          <p class="truncate text-[11px] leading-tight text-muted-foreground">
            for the whole extension
          </p>
        </div>

        <!-- The browser's own tooltip rather than the shell's, for the reason
             the queue sheet's close button has one: this is the only way out,
             and the one control a screen reader has to find here is better off
             being the button itself. -->
        <Button
          variant="ghost"
          size="icon-sm"
          class="-mr-1 shrink-0 text-muted-foreground"
          aria-label="close settings"
          title="close"
          onclick={onclose}
        >
          <X />
        </Button>
      </header>

      <div class="scroll-slim min-h-0 flex-1 overflow-y-auto px-3 py-3">
        <div class="flex flex-col gap-2.5">
          <section class="flex flex-col gap-2 rounded-xl border border-border/70 bg-card p-2.5">
            <h3 class="text-[11.5px] font-semibold">SoundCloud pages</h3>

            <div class="flex items-start gap-2.5">
              <Switch
                id="replace-download-buttons"
                class="mt-0.5 shrink-0"
                bind:checked={
                  () => settings.replaceDownloadButtons,
                  (on) => (settings.replaceDownloadButtons = on)
                }
              />
              <div class="min-w-0 flex-1">
                <Label for="replace-download-buttons" class="text-[12px] font-normal">
                  replace SoundCloud’s download buttons
                </Label>
                <p class="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">
                  {REPLACE_HINT}
                </p>
              </div>
            </div>

            <div class="flex items-start gap-2.5">
              <Switch
                id="likes-list-view"
                class="mt-0.5 shrink-0"
                bind:checked={
                  () => settings.likesListView, (on) => (settings.likesListView = on)
                }
              />
              <div class="min-w-0 flex-1">
                <Label for="likes-list-view" class="text-[12px] font-normal">
                  open your likes as a list
                </Label>
                <p class="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">
                  {LIKES_VIEW_HINT}
                </p>
              </div>
            </div>
          </section>

          <!--
            What the Downloader gives a gate that asks, and what it does with
            what comes back. Kept here rather than on that panel because none
            of it is about the list on screen: the address and the name are
            typed on the day one gate asks for them, and a failed row that
            wants either sends you straight to this sheet.
          -->
          <section class="flex flex-col gap-2 rounded-xl border border-border/70 bg-card p-2.5">
            <h3 class="text-[11.5px] font-semibold">downloads</h3>

            <div class="flex flex-col gap-1.5">
              <Label for="email" class="text-[11.5px] text-muted-foreground">email</Label>
              <Input
                id="email"
                type="email"
                placeholder="you@example.com"
                autocomplete="off"
                spellcheck="false"
                class="h-8 text-[12px]"
                bind:value={downloader.email.value}
              />
              <Label for="name" class="text-[11.5px] text-muted-foreground">name</Label>
              <Input
                id="name"
                type="text"
                placeholder="your name"
                autocomplete="off"
                class="h-8 text-[12px]"
                bind:value={downloader.name.value}
              />
              <p class="text-[11px] leading-relaxed text-muted-foreground">{EMAIL_HINT}</p>
            </div>

            <div class="flex items-start gap-2.5">
              <Switch
                id="mark"
                class="mt-0.5 shrink-0"
                bind:checked={downloader.mark.value}
              />
              <div class="min-w-0 flex-1">
                <Label for="mark" class="text-[12px] font-normal">
                  mark the SoundCloud track downloaded
                </Label>
                <p class="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">
                  {MARK_HINT}
                </p>
              </div>
            </div>
          </section>

          <!--
            The badges a link-less track carries in Manager, one switch each.
            Which sites there are is data rather than code
            (src/lib/search-sites.json), so this list is however long that file
            is and nothing here names a site — somebody who adds one to the
            file gets its switch for free.

            Here rather than on the Manager panel for the reason the address
            above is: it is not about the list in front of you. Which shops you
            buy from is a fact about you, settled once, and the panel's own
            options are the two that say how its list is drawn.
          -->
          <section class="flex flex-col gap-2 rounded-xl border border-border/70 bg-card p-2.5">
            <h3 class="text-[11.5px] font-semibold">search badges</h3>
            <p class="text-[11px] leading-relaxed text-muted-foreground">{SEARCH_HINT}</p>

            <div class="flex flex-col gap-2">
              {#each badges as site (site.id)}
                <div class="flex items-center gap-2.5">
                  <Switch
                    id={`search-${site.id}`}
                    class="shrink-0"
                    bind:checked={() => site.on, (on) => searchSites.set(site.id, on)}
                  />
                  <!-- The badge itself, drawn as the row draws it: the two
                       letters on the button are what somebody has to
                       recognise, and the name beside them is what this list
                       is otherwise a list of. -->
                  <span
                    class="grid size-5 shrink-0 place-items-center rounded text-[9px] font-bold"
                    style={site.style}
                    aria-hidden="true">{site.badge}</span
                  >
                  <Label for={`search-${site.id}`} class="text-[12px] font-normal">
                    {site.name}
                  </Label>
                </div>
              {/each}
            </div>

            {#if badges.every((site) => !site.on)}
              <p class="text-[11px] leading-relaxed text-muted-foreground">{NO_SEARCH_HINT}</p>
            {/if}
          </section>

          <!-- A section of its own (StateTransfer.svelte): it is the only
               thing here that writes anything but a setting, and the one
               control in the sheet that can take something away. -->
          <StateTransfer />

          <section class="flex flex-col gap-2 rounded-xl border border-border/70 bg-card p-2.5">
            <h3 class="text-[11.5px] font-semibold">appearance</h3>
            <div class="flex gap-1.5">
              {#each THEMES as option (option.mode)}
                {@const Icon = option.icon}
                {@const on = theme.mode === option.mode}
                <Button
                  size="xs"
                  variant={on ? 'default' : 'outline'}
                  class={on ? '' : 'text-muted-foreground'}
                  aria-pressed={on}
                  onclick={() => theme.set(option.mode)}
                >
                  <Icon />
                  {option.label}
                </Button>
              {/each}
            </div>

            <!--
              How an opened row's words are drawn, rather than which of them
              there are: the transform is $lib/title-case.ts and the switch
              itself $lib/title-case.svelte.ts, thrown here and read by the
              Manager panel under this sheet, which redraws its opened rows as
              it moves.
            -->
            <div class="flex items-start gap-2.5">
              <Switch
                id="title-case"
                class="mt-0.5 shrink-0"
                bind:checked={titleCasing.value}
              />
              <div class="min-w-0 flex-1">
                <Label for="title-case" class="text-[12px] font-normal">
                  title case the title, artist and genre
                </Label>
                <p class="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">
                  {TITLE_CASE_HINT}
                </p>
              </div>
            </div>
          </section>

          <section class="flex flex-col gap-2 rounded-xl border border-border/70 bg-card p-2.5">
            <h3 class="text-[11.5px] font-semibold">about</h3>
            <div class="flex items-center gap-2.5">
              <p class="min-w-0 flex-1 truncate text-[11px] text-muted-foreground">
                localtoolbox {version}
              </p>
              <Button
                size="xs"
                variant="outline"
                class="text-muted-foreground"
                href={reportUrl}
                target="_blank"
                rel="noreferrer"
              >
                <Bug />
                report a bug
              </Button>
            </div>
          </section>
        </div>
      </div>
    </div>
  </div>
</Tooltip.Provider>
