<!--
  A switch with its label beside it and a hint in a tooltip: "hide
  downloaded", "background tabs" and the like, over a list.

  The tooltip's trigger is the switch itself. It used to be a wrapper around
  the pair, which made the wrapper a tab stop of its own with no name, one
  Tab before every switch, and left the switch without the hint. The hint is
  also the switch's description, since the tooltip only exists while it is
  open.
-->
<script lang="ts">
  import { Label } from '$lib/components/ui/label/index.ts';
  import { Switch } from '$lib/components/ui/switch/index.ts';
  import * as Tooltip from '$lib/components/ui/tooltip/index.ts';

  interface Props {
    id: string;
    label: string;
    hint: string;
    checked: boolean;
  }

  let { id, label, hint, checked = $bindable() }: Props = $props();
</script>

<div class="flex shrink-0 items-center gap-1.5">
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Switch {...props} {id} aria-describedby={`${id}-hint`} bind:checked />
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content class="max-w-72">{hint}</Tooltip.Content>
  </Tooltip.Root>
  <Label for={id} class="text-caption font-normal whitespace-nowrap text-fg-muted">
    {label}
  </Label>
  <span id={`${id}-hint`} class="sr-only">{hint}</span>
</div>
