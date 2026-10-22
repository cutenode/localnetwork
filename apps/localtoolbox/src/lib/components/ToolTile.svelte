<!--
  A tool's icon on its tone, the way the website draws a DJ's monogram on a
  track colour: in the menu, in its panel's header and ghost, and on the add
  bar at the top of a list. Without a tone it is a plain tile, for the sheets,
  which belong to no one tool.

  Plain HTML, since the menu the popup boots into draws it.
-->
<script lang="ts">
  import type { LucideIcon } from '@lucide/svelte';

  let {
    icon: Icon,
    tone = '',
    size = 'sm',
  }: { icon: LucideIcon; tone?: string; size?: 'sm' | 'md' } = $props();
</script>

<span
  aria-hidden="true"
  class="grid shrink-0 place-items-center rounded-md
         {tone ? `${tone} text-black` : 'bg-well text-fg'}
         {size === 'md' ? 'size-9' : 'size-8'}"
>
  <Icon class={size === 'md' ? 'size-4.5' : 'size-4'} />
</span>
