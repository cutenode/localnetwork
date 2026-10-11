<!--
  The way into the session queue, in the header of every view.

  The queue used to be the fourth tool in the menu, which put the one thing
  that is *about* the other tools on the same footing as them: a batch picked
  out in Manager could only be looked at by leaving Manager. It is a
  header control now — the count is in front of you wherever you are, and the
  sheet it opens goes over whatever you were doing rather than replacing it.

  Written out in plain HTML rather than with the shell's Button and Tooltip,
  for the reason ThemeToggle beside it is: this sits in the tool menu too, and
  the menu is what the popup boots into. Pulling the component library into
  that chunk for one icon button costs more than the button is worth. The
  sheet, which does use the library, is a chunk of its own (App.svelte).
-->
<script lang="ts">
  import ListChecks from '@lucide/svelte/icons/list-checks';
  import { queue } from '$lib/queue.svelte.ts';
  import { view } from '$lib/view.svelte.ts';
  import { count } from '$lib/soundcloud.ts';

  // `aria-expanded` says whether the sheet is up, so the label does not have
  // to flip between "open" and "close" — what it names is the queue, and how
  // full it is.
  const label = $derived(
    queue.count === 0 ? 'session queue: empty' : `session queue: ${count(queue.count, 'track')}`
  );
</script>

<!--
  Narrow on purpose. It shares a panel's header with the tool's own buttons and
  with the tool's name, which is what gives way when the row runs out of room:
  every pixel here is one off the name. Hence the tight padding, and hence an
  empty queue drawing no count at all rather than a nought.
-->
<button
  type="button"
  class="flex h-7 shrink-0 items-center gap-0.5 rounded-[min(var(--radius-md),12px)] border
         border-transparent bg-clip-padding px-1 text-muted-foreground transition-all
         outline-none select-none hover:bg-muted hover:text-foreground
         focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50
         active:translate-y-px dark:hover:bg-muted/50"
  aria-label={label}
  title={label}
  aria-expanded={view.queueOpen}
  onclick={() => (view.queueOpen = !view.queueOpen)}
>
  <ListChecks class="size-4 shrink-0" />
  {#if queue.count > 0}
    <!-- The count in the open tool's own colour: a queue with something in it
         is worth noticing, and this is the one place it can be noticed from
         without leaving the tool. `aria-hidden`, because the label above says
         the same thing in words. -->
    <span
      class="tabular grid h-4 min-w-4 shrink-0 place-items-center rounded-full bg-primary px-0.5
             text-[10px] leading-none font-semibold text-primary-foreground"
      aria-hidden="true"
    >
      {queue.count}
    </span>
  {/if}
</button>
