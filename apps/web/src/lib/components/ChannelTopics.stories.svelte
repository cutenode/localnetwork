<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import { submit } from '#lib/testing/submits.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import type { NotificationTopic } from '#lib/types.ts';
	import ChannelTopics from './ChannelTopics.svelte';

	const topics: NotificationTopic[] = ['bookings', 'guests', 'waitlist', 'events'];

	const { Story } = defineMeta({ title: 'Dashboard/Channel topics', component: ChannelTopics, tags: ['autodocs'], args: { action: '?/emailTopics', legend: 'What to email', topics, selected: ['bookings', 'events'] } });
</script>

<Story name="Some picked" />

<Story name="None picked" args={{ selected: [] }} />

<!-- What email and DMs offer today. -->
<Story name="One topic" args={{ topics: ['bookings'], selected: ['bookings'] }} />

<Story name="Phone" globals={phone} />

<Story
	name="Saving"
	play={async ({ canvasElement }) => {
		await userEvent.click(within(canvasElement).getByRole('checkbox', { name: 'Guests' }));
		await expect(await submit(canvasElement, 'Save', 'topic')).toEqual({ topic: ['bookings', 'guests', 'events'] });
	}}
/>
