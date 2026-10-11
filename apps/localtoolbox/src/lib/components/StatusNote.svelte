<!--
  What a panel says instead of a list: why the page could not be used, what an
  import did, which tracks could not be checked. These stay on screen until
  something changes them — unlike a toast, which is for what just happened.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import Info from '@lucide/svelte/icons/info';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import { cn } from '$lib/utils.ts';

  interface Props {
    tone?: 'info' | 'ok' | 'bad' | 'busy';
    class?: string;
    children: Snippet;
  }

  let {
    tone = 'info',
    class: className = '',
    children,
  }: Props = $props();

  const TONES = {
    info: {
      icon: Info,
      wrap: 'border-border bg-muted/50 text-foreground',
      mark: 'text-muted-foreground',
    },
    ok: {
      icon: CircleCheck,
      wrap: 'border-success/25 bg-success/8 text-foreground',
      mark: 'text-success',
    },
    bad: {
      icon: CircleAlert,
      wrap: 'border-destructive/25 bg-destructive/8 text-foreground',
      mark: 'text-destructive',
    },
    busy: {
      icon: LoaderCircle,
      wrap: 'border-brand/25 bg-brand/8 text-foreground',
      mark: 'text-brand animate-spin',
    },
  };

  const style = $derived(TONES[tone]);

  const Icon = $derived(style.icon);
</script>

<div
  class={cn('flex items-start gap-2.5 rounded-xl border px-3 py-2.5 text-[12.5px]', style.wrap, className)}
  role={tone === 'bad' ? 'alert' : 'status'}
>
  <Icon class={cn('mt-px size-4 shrink-0', style.mark)} />
  <div class="min-w-0 flex-1 leading-relaxed">{@render children()}</div>
</div>
