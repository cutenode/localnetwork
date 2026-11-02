<!-- Shows what the toast store is holding (see $lib/toast.svelte.ts). -->
<script lang="ts">
  import { fly, scale } from 'svelte/transition';
  import Check from '@lucide/svelte/icons/check';
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import Info from '@lucide/svelte/icons/info';
  import X from '@lucide/svelte/icons/x';
  import { motion } from '$lib/motion.ts';
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
    <!-- Held while it is being read: under the pointer, or with focus on its
         close button. Not a control itself, so the handlers are only there
         to pause the countdown. -->
    <div
      in:fly={motion({ y: 10, duration: 180 })}
      out:scale={motion({ start: 0.96, duration: 130 })}
      role="presentation"
      class="pointer-events-auto flex max-w-full items-center gap-2 rounded-full border
             border-line-strong bg-raised py-1.5 pl-2.5 text-caption font-medium
             {toast.tone === 'bad' ? 'pr-1.5' : 'pr-3.5'}"
      onpointerenter={() => toasts.hold(toast.id)}
      onpointerleave={() => toasts.release(toast.id)}
      onfocusin={() => toasts.hold(toast.id)}
      onfocusout={() => toasts.release(toast.id)}
    >
      <Icon class="size-3.5 shrink-0 {TONES[toast.tone]}" />
      <span class="truncate">{toast.message}</span>
      {#if toast.tone === 'bad'}
        <!-- Something that went wrong can be put away by hand, since it stays
             up long enough to be in the way. -->
        <button
          type="button"
          class="grid size-5 shrink-0 place-items-center rounded-full text-fg-muted transition-colors
                 hover:bg-well hover:text-fg"
          aria-label="dismiss"
          onclick={() => toasts.dismiss(toast.id)}
        >
          <X class="size-3" />
        </button>
      {/if}
    </div>
  {/each}
</div>
