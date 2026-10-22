<!--
  A track's cover art at the size the row wants it.

  The URL is normally the exact one the SoundCloud page already rendered, so it
  is served straight from the browser cache; a row whose thumbnail never
  lazy-loaded falls back to the cover the lookup named (see artworkOf in
  $lib/artwork.ts). A track with neither gets a placeholder that says so rather
  than an empty square.
-->
<script lang="ts">
  import Music from '@lucide/svelte/icons/music';
  import { cn } from '$lib/utils.ts';

  let { src = '', size = 'size-10', class: className = '' } = $props();
</script>

{#if src}
  <img
    {src}
    alt=""
    loading="lazy"
    class={cn(
      'shrink-0 rounded-md bg-well object-cover ring-1 ring-line',
      size,
      className
    )}
  />
{:else}
  <div
    class={cn(
      'grid shrink-0 place-items-center rounded-md bg-well text-fg-subtle ring-1 ring-line',
      size,
      className
    )}
    title="no cover art"
  >
    <Music class="size-4 opacity-60" />
  </div>
{/if}
