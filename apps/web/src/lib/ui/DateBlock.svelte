<script lang="ts">
	import { tones, type Tone } from './Eyebrow.svelte';

	/** A calendar-page date for event lists, a size smaller on a phone. `date` is the record's YYYY-MM-DD. */
	let { date, tone = 'chorus' }: { date: string; tone?: Tone } = $props();

	const day = $derived(new Date(`${date}T00:00:00Z`));

	const part = (options: Intl.DateTimeFormatOptions) => new Intl.DateTimeFormat('en-GB', { timeZone: 'UTC', ...options }).format(day);
</script>

<time datetime={date} class="flex w-12 shrink-0 flex-col items-center rounded-lg bg-well py-1.5 text-center sm:w-16 sm:py-2">
	<span class="font-mono text-caption uppercase {tones[tone]}">{part({ weekday: 'short' })}</span>
	<span class="text-emphasis font-semibold sm:text-heading">{part({ day: 'numeric' })}</span>
	<span class="text-caption text-fg-subtle">{part({ month: 'short' })}</span>
</time>
