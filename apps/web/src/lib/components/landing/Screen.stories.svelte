<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import Screen from './Screen.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Landing screen', component: Screen, tags: ['autodocs'] });

	const threads = [
		{ title: 'Friday Social', with: 'Neon Tide' },
		{ title: 'Rooftop Rave, a long show name that runs past the edge of a phone', with: 'Saffron Social' }
	];
</script>

{#snippet inbox()}
	<ul class="flex flex-col divide-y divide-line">
		{#each threads as thread (thread.title)}
			<li class="flex min-w-0 flex-col gap-0.5 py-3">
				<p class="truncate font-semibold">{thread.title}</p>
				<p class="text-small text-fg-muted">{thread.with}</p>
			</li>
		{/each}
	</ul>
{/snippet}

{#snippet caption()}Every inquiry in one thread per show.{/snippet}

<Story name="Live" asChild>
	<Screen path="/dashboard/bookings" {caption}>{@render inbox()}</Screen>
</Story>

<Story name="Example" asChild>
	<Screen path="/dashboard/bookings" example {caption}>{@render inbox()}</Screen>
</Story>

<Story name="Phone" asChild globals={phone}>
	<Screen path="/dashboard/a-path-long-enough-to-run-past-the-edge-of-a-phone" example {caption}>{@render inbox()}</Screen>
</Story>
