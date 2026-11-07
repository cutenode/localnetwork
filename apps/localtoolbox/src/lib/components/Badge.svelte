<!--
  A lettered badge in a site's colour: a social link on an artist, or a
  search on a site switched on in settings. The letters are what somebody
  recognises, so they start the button's name too ("IG: open Instagram"),
  which is what a voice command or a screen reader has to match. Where it
  goes is the tooltip and the description, never the name.
-->
<script lang="ts">
  import { Button } from '$lib/components/ui/button/index.ts';

  interface Props {
    /** The two or three letters on it. */
    badge: string;
    /** What it does, after the letters in its name: "open Instagram". */
    action: string;
    /** Its colours, as a style attribute. */
    style: string;
    /** The tooltip: where it goes. */
    title: string;
    disabled?: boolean;
    onclick: () => void;
  }

  let { badge, action, style, title, disabled = false, onclick }: Props = $props();
</script>

<Button
  size="icon-sm"
  variant="ghost"
  {style}
  class="w-7 border-transparent text-[10px] font-bold hover:brightness-110"
  {disabled}
  aria-label={`${badge}: ${action}`}
  {title}
  {onclick}
>
  {badge}
</Button>
