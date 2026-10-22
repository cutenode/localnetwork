<!--
  Light, dark, or whatever the browser is set to.

  The one control on the tool menu, and the reason it is written out in plain
  HTML rather than with the shell's Button and Tooltip: the menu is what the
  popup boots into, and pulling the component library into that chunk for one
  icon button cost more than the button is worth. `title` is the browser's own
  tooltip, which is the right one for a control the panels' tooltips never
  have to line up with.
-->
<script lang="ts">
  import Sun from '@lucide/svelte/icons/sun';
  import Moon from '@lucide/svelte/icons/moon';
  import Monitor from '@lucide/svelte/icons/monitor';
  import { theme } from '$lib/theme.svelte.ts';

  const ICONS = { system: Monitor, light: Sun, dark: Moon };

  const LABELS = {
    system: 'theme: follows the browser',
    light: 'theme: light',
    dark: 'theme: dark',
  };

  const Icon = $derived(ICONS[theme.mode]);

  const label = $derived(LABELS[theme.mode]);
</script>

<button
  type="button"
  class="icon-btn"
  aria-label={label}
  title={label}
  onclick={() => theme.next()}
>
  <Icon class="size-4" />
</button>
