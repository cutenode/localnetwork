<script lang="ts">
	import { tones, type Tone } from './Eyebrow.svelte';

	/**
	 * A calendar-page date for event lists, a size smaller on a phone. `date` is the record's YYYY-MM-DD. A `tentative`
	 * date isn't settled yet, so it's dashed and loses its colour. `compact` keeps the phone size on every screen, for a
	 * list that shouldn't outweigh the one above it.
	 */
	let { date, tone = 'chorus', tentative = false, compact = false }: { date: string; tone?: Tone; tentative?: boolean; compact?: boolean } = $props();

	const day = $derived(new Date(`${date}T00:00:00Z`));

	const part = (options: Intl.DateTimeFormatOptions) => new Intl.DateTimeFormat('en-GB', { timeZone: 'UTC', ...options }).format(day);
</script>

<time datetime={date} class={['flex w-12 shrink-0 flex-col items-center rounded-lg py-1.5 text-center', !compact && 'sm:w-16 sm:py-2', tentative ? 'border border-dashed border-line-strong' : 'bg-well']}>
	<span class="font-mono text-caption uppercase {tones[tentative ? 'muted' : tone]}">{part({ weekday: 'short' })}</span>
	<span class={['text-emphasis font-semibold', !compact && 'sm:text-heading']}>{part({ day: 'numeric' })}</span>
	<span class="text-caption text-fg-subtle">{part({ month: 'short' })}</span>
</time>
