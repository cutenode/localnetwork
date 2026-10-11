<!--
  Stands in for App.svelte when a test mounts one panel on its own: the panels
  use tooltips, which bits-ui wants a provider above, and they report what just
  happened through the toaster the shell renders.
-->
<script lang="ts">
  import type { PanelProps } from '$lib/types';
  import type { PanelComponent } from '$lib/tools.ts';
  import * as Tooltip from '$lib/components/ui/tooltip/index.js';
  import Toaster from '$lib/components/Toaster.svelte';

  interface Props extends PanelProps {
    /** The panel itself, as its tool's chunk exports it. */
    Panel: PanelComponent;
  }

  let { Panel, tool, onback }: Props = $props();
</script>

<Tooltip.Provider delayDuration={350}>
  <Panel {tool} {onback} />
  <Toaster />
</Tooltip.Provider>
