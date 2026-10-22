<!-- Shows what the toast store is holding (see $lib/toast.svelte.ts). -->
<script lang="ts">
  import { fly, scale } from 'svelte/transition';
  import Check from '@lucide/svelte/icons/check';
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import Info from '@lucide/svelte/icons/info';
  import { toasts } from '$lib/toast.svelte.ts';

  const ICONS = { ok: Check, bad: CircleAlert, info: Info };

  const TONES = {
    ok: 'text-good',
    bad: 'text-critical',
    info: 'text-fg-muted',
  };
</script>

<div
  class="pointer-events-none fixed inset-x-0 bottom-0 z-50 flex flex-col items-center gap-1.5 px-3 pb-3"
  aria-live="polite"
>
  {#each toasts.items as toast (toast.id)}
    {@const Icon = ICONS[toast.tone]}
    <div
      in:fly={{ y: 10, duration: 180 }}
      out:scale={{ start: 0.96, duration: 130 }}
      class="pointer-events-auto flex max-w-full items-center gap-2 rounded-full border
             border-line-strong bg-raised py-1.5 pr-3.5 pl-2.5 text-caption font-medium"
    >
      <Icon class="size-3.5 shrink-0 {TONES[toast.tone]}" />
      <span class="truncate">{toast.message}</span>
    </div>
  {/each}
</div>
