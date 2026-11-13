<!--
  Settings: the handful of choices that belong to the extension rather than to
  any one tool.

  A panel's own options stay on the panel: the sort order and "background
  tabs" say how the Manager list behaves and are read a foot from where they
  are set. What is here is the other kind: the theme, which is every view at
  once; what the content script does to a SoundCloud page, which is somewhere
  the popup is not (see $lib/settings.svelte.ts for why those cannot live in
  the popup's own storage); what the Downloader hands to a gate that asks
  for it, which is typed once and never thought about again
  ($lib/downloader.svelte.ts); which shops the Manager's search badges
  point at, and any added here, which is a fact about where somebody buys
  records rather than about the list in front of them
  ($lib/search-sites.svelte.ts, over the sites named in
  $lib/search-sites.json; the form is SearchSiteForm.svelte); and whether an
  opened row's words are put into Title Case, which is a house style for a
  tracklist rather than a fact about any one of them ($lib/title-case.svelte.ts,
  over $lib/title-case.ts).

  So the two kinds of storage both turn up here: what a setting *is* decides
  where it is set, and who has to read it decides where it is kept. The
  switches at the top are in chrome.storage.local because a SoundCloud page
  obeys them; the address, the name, the badge switches and the casing below
  are in the popup's own localStorage because nothing outside the popup has
  any use for them.

  One section is not a setting at all. Everything the extension has saved (the
  marks, the Manager's list, the artists list, the session queue, the search
  sites and three panel options) goes out to one file there and reads
  back in (StateTransfer.svelte). It is here rather than in the Manager panel's
  footer because the file carries other tools' state too, and a backup is
  something you take before opening a playlist rather than while looking at
  one. The last section is not one either: which version this is, and a link
  to report a bug with that version already filled in ($lib/bug-report.ts),
  since this sheet is about the extension rather than any one tool.

  A sheet rather than a tool, and for the reason the session queue is one: it
  is about the tools rather than beside them. It comes up over whichever one is
  open, from a button in the header of every view (SettingsButton.svelte), and
  goes away leaving that tool exactly as it was. The sheet itself, and where
  focus goes, is Sheet.svelte.
-->
<script lang="ts">
  import Bug from '@lucide/svelte/icons/bug';
  import Monitor from '@lucide/svelte/icons/monitor';
  import Moon from '@lucide/svelte/icons/moon';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Plus from '@lucide/svelte/icons/plus';
  import Settings from '@lucide/svelte/icons/settings';
  import Sun from '@lucide/svelte/icons/sun';
  import X from '@lucide/svelte/icons/x';
  import SearchSiteForm from '$lib/components/SearchSiteForm.svelte';
  import Sheet from '$lib/components/Sheet.svelte';
  import StateTransfer from '$lib/components/StateTransfer.svelte';
  import { Button } from '$lib/components/ui/button/index.ts';
  import { Input } from '$lib/components/ui/input/index.ts';
  import { Label } from '$lib/components/ui/label/index.ts';
  import { Switch } from '$lib/components/ui/switch/index.ts';
  import { bugReportUrl, extensionVersion } from '$lib/bug-report.ts';
  import { downloader } from '$lib/downloader.svelte.ts';
  import { GREY, sixDigits, type SearchSite } from '$lib/search-sites.ts';
  import { searchSites } from '$lib/search-sites.svelte.ts';
  import { settings } from '$lib/settings.svelte.ts';
  import { theme } from '$lib/theme.svelte.ts';
  import { titleCasing } from '$lib/title-case.svelte.ts';
  import { remembered, view } from '$lib/view.svelte.ts';

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

  // What Hypeddit's privacy policy says it does with a gate's address
  // (hypeddit.com/privacy): the artist gets it, and so do the service it checks
  // addresses with and its own marketing providers. The address also goes into
  // the email field of a Droploud or Laylo page opened from the Downloader,
  // where the person sends it or not.
  const EMAIL_HINT =
    'Hypeddit gates that ask get these, and Hypeddit shares them with the artist, its ' +
    'email-checking service and its marketing providers. Droploud and Laylo pages you open ' +
    'from here get the email filled in, for you to send.';

  const MARK_HINT = 'off, saved files go unmarked — mark them by hand from the row.';

  const SEARCH_HINT = 'tracks with no download link get one badge per site.';

  const NO_SEARCH_HINT = 'all off — those tracks get no badges.';

  const TITLE_CASE_HINT =
    'in an opened row, and in what a click off one copies. capitals somebody typed are kept ' +
    'as typed; a field shouted in capitals is re-cased.';

  const badges = $derived(searchSites.all);

  // The site form, open or shut, kept for the session as the import drawer
  // is: the address it wants is copied out of a shop's tab, and going there
  // closes the popup. Empty while shut; SearchSiteForm.svelte says the rest.
  const siteDraft = remembered<string[]>('settings', 'search-site', []);

  /** Opens the form on a site of your own, or on a new one. */
  const openSite = (site?: SearchSite) =>
    (siteDraft.value = site
      ? [site.id, site.name, site.badge, site.url, sixDigits(site.background), sixDigits(site.color)]
      : ['', '', '', '', GREY, '#ffffff']);

  function removeSite(id: string) {
    searchSites.remove(id);

    if (siteDraft.value[0] === id) siteDraft.value = [];
  }

  const version = extensionVersion();

  // The tool under the sheet, if any, is the part a report is most likely about.
  const reportUrl = $derived(bugReportUrl(view.open));

  const THEMES = [
    { mode: 'system', icon: Monitor, label: 'system' },
    { mode: 'light', icon: Sun, label: 'light' },
    { mode: 'dark', icon: Moon, label: 'dark' },
  ];
</script>

<Sheet title="settings" icon={Settings} meta="for the whole extension" closeLabel="close settings" {onclose}>
  <div class="flex flex-col gap-2.5">
    <section class="flex flex-col gap-2 rounded-lg border border-line bg-surface p-3">
      <h3 class="section-title m-0">SoundCloud pages</h3>

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
          <Label for="replace-download-buttons" class="text-label leading-snug font-normal">
            replace SoundCloud’s download buttons
          </Label>
          <p class="m-0 mt-1 text-caption leading-relaxed text-fg-muted">
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
          <Label for="likes-list-view" class="text-label leading-snug font-normal">
            open your likes as a list
          </Label>
          <p class="m-0 mt-1 text-caption leading-relaxed text-fg-muted">
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
    <section class="flex flex-col gap-2 rounded-lg border border-line bg-surface p-3">
      <h3 class="section-title m-0">downloads</h3>

      <div class="flex flex-col gap-1.5">
        <Label for="email" class="text-caption text-fg-muted">email</Label>
        <Input
          id="email"
          type="email"
          placeholder="you@example.com"
          autocomplete="off"
          spellcheck="false"
          class="h-8"
          bind:value={downloader.email.value}
        />
        <Label for="name" class="text-caption text-fg-muted">name</Label>
        <Input
          id="name"
          type="text"
          placeholder="your name"
          autocomplete="off"
          class="h-8"
          bind:value={downloader.name.value}
        />
        <p class="m-0 text-caption leading-relaxed text-fg-muted">{EMAIL_HINT}</p>
      </div>

      <div class="flex items-start gap-2.5">
        <Switch
          id="mark"
          class="mt-0.5 shrink-0"
          bind:checked={downloader.mark.value}
        />
        <div class="min-w-0 flex-1">
          <Label for="mark" class="text-label leading-snug font-normal">
            mark the SoundCloud track downloaded
          </Label>
          <p class="m-0 mt-1 text-caption leading-relaxed text-fg-muted">
            {MARK_HINT}
          </p>
        </div>
      </div>
    </section>

    <!--
      The badges a link-less track carries in Manager, one switch each.
      Which sites there are is data rather than code
      (src/lib/search-sites.json), so this list is however long that file
      is and nothing here names a site. Somebody who adds one to the
      file gets its switch for free, and so does one added with the form
      below, which can also be edited and removed again.

      Here rather than on the Manager panel for the reason the address
      above is: it is not about the list in front of you. Which shops you
      buy from is a fact about you, settled once, and the panel's own
      options are the two that say how its list is drawn.
    -->
    <section class="flex flex-col gap-2 rounded-lg border border-line bg-surface p-3">
      <h3 class="section-title m-0">search badges</h3>
      <p class="m-0 text-caption leading-relaxed text-fg-muted">{SEARCH_HINT}</p>

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
            <Label for={`search-${site.id}`} class="min-w-0 flex-1 truncate text-label leading-snug font-normal">
              {site.name}
            </Label>
            {#if site.own}
              <!-- Pulled into the row's own height, so a row of yours is
                   spaced like the file's. -->
              <Button
                size="icon-xs"
                variant="ghost"
                class="-my-0.5"
                aria-label={`edit ${site.name}`}
                aria-expanded={siteDraft.value[0] === site.id}
                onclick={() => openSite(site)}
              >
                <Pencil />
              </Button>
              <Button
                size="icon-xs"
                variant="ghost"
                class="-my-0.5"
                aria-label={`remove ${site.name}`}
                onclick={() => removeSite(site.id)}
              >
                <X />
              </Button>
            {/if}
          </div>
        {/each}
      </div>

      {#if badges.every((site) => !site.on)}
        <p class="m-0 text-caption leading-relaxed text-fg-muted">{NO_SEARCH_HINT}</p>
      {/if}

      {#if siteDraft.value.length > 0}
        <!-- Keyed, so editing another site starts the form over on it. -->
        {#key siteDraft.value[0]}
          <SearchSiteForm draft={siteDraft} />
        {/key}
      {:else}
        <Button size="sm" variant="outline" class="self-start" onclick={() => openSite()}>
          <Plus />
          add a site
        </Button>
      {/if}
    </section>

    <!-- A section of its own (StateTransfer.svelte): it is the only
         thing here that writes anything but a setting, and the one
         control in the sheet that can take something away. -->
    <StateTransfer />

    <section class="flex flex-col gap-2 rounded-lg border border-line bg-surface p-3">
      <h3 class="section-title m-0">appearance</h3>
      <div class="flex gap-1.5">
        {#each THEMES as option (option.mode)}
          {@const Icon = option.icon}
          {@const on = theme.mode === option.mode}
          <Button
            size="sm"
            variant={on ? 'default' : 'outline'}
            class={on ? '' : 'text-fg-muted'}
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
          <Label for="title-case" class="text-label leading-snug font-normal">
            title case the title, artist and genre
          </Label>
          <p class="m-0 mt-1 text-caption leading-relaxed text-fg-muted">
            {TITLE_CASE_HINT}
          </p>
        </div>
      </div>
    </section>

    <section class="flex flex-col gap-2 rounded-lg border border-line bg-surface p-3">
      <h3 class="section-title m-0">about</h3>
      <div class="flex items-center gap-2.5">
        <p class="m-0 min-w-0 flex-1 truncate font-mono text-caption text-fg-muted">
          localtoolbox {version}
        </p>
        <Button
          size="sm"
          variant="outline"
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
</Sheet>
