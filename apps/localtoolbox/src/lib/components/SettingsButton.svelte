<!--
  The way into the settings sheet, in the header of every view.

  Beside the queue button, and written out in plain HTML for the same reason:
  this sits in the tool menu too, and the menu is what the popup boots into.
  Pulling the component library into that chunk for one icon button costs more
  than the button is worth. The sheet, which does use the library, is a chunk
  of its own (App.svelte).

  In every header rather than in the menu alone, because what it holds is
  about the SoundCloud page you are looking at while a tool is open — not
  about the popup.
-->
<script lang="ts">
  import Settings from '@lucide/svelte/icons/settings';
  import { view } from '$lib/view.svelte.ts';

  const LABEL = 'settings';
</script>

<button
  type="button"
  class="grid size-7 shrink-0 place-items-center rounded-[min(var(--radius-md),12px)]
         border border-transparent bg-clip-padding text-muted-foreground transition-all
         outline-none select-none hover:bg-muted hover:text-foreground
         focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50
         active:translate-y-px dark:hover:bg-muted/50"
  aria-label={LABEL}
  title={LABEL}
  aria-expanded={view.settingsOpen}
  onclick={() => (view.settingsOpen = !view.settingsOpen)}
>
  <Settings class="size-4" />
</button>
