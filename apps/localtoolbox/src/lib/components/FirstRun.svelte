<!--
  What the tool menu shows in place of the tools until it has been read once:
  where the extension sends anything. The README's "Privacy" section says the
  same at more length. Kept with the panel options in localStorage, so it is
  read before the first paint and never flashes up on a later opening.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { flagPref } from '$lib/prefs.svelte.ts';

  const seen = flagPref('pref:notice-seen', false);

  let { children }: { children: Snippet } = $props();

  const LINES = [
    'Hypeddit gates that ask get the email and name from settings, and Hypeddit shares them with the artist.',
    'A Spotify step follows the artist and saves the track on your own Spotify account.',
    'Nothing goes to a localtoolbox server. There isn’t one.',
  ];
</script>

{#if seen.value}
  {@render children()}
{:else}
  <section class="flex flex-col gap-2 p-3" aria-labelledby="first-run">
    <p id="first-run" class="section-title m-0 mb-0.5">before you start</p>
    <ul class="m-0 flex list-none flex-col gap-1.5 rounded-lg border border-line bg-surface p-3">
      {#each LINES as line (line)}
        <li class="text-caption leading-relaxed text-fg-soft">{line}</li>
      {/each}
    </ul>
    <button
      type="button"
      class="inline-flex h-8 items-center justify-center self-end rounded-md bg-fg px-3 text-label
             font-semibold text-fg-inverse transition-colors hover:bg-fg-soft"
      onclick={() => (seen.value = true)}
    >
      continue
    </button>
  </section>
{/if}
