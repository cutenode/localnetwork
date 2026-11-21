<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import { enhanced } from '#lib/testing/submits.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import type { ChannelView, NotificationTopic } from '#lib/types.ts';
	import ChannelTopics, { type ChannelColumn } from './ChannelTopics.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/What goes where', component: ChannelTopics, tags: ['autodocs'] });

	const on: ChannelView = { status: 'active', pausedReason: null, address: 'neon@example.test', topics: ['bookings'] };

	const all: NotificationTopic[] = ['bookings', 'guests', 'waitlist', 'events'];

	const some: ChannelColumn[] = [
		{ channel: 'email', view: { ...on, topics: ['bookings', 'events'] } },
		{ channel: 'dm', view: on }
	];
</script>

<Story name="Email on, DMs off" args={{ topics: ['bookings'], channels: [{ channel: 'email', view: on }, { channel: 'dm', view: null }] }} />

<Story name="DMs paused" args={{ topics: ['bookings'], channels: [{ channel: 'email', view: { ...on, topics: [] } }, { channel: 'dm', view: { ...on, status: 'paused', pausedReason: 'dm_not_allowed' } }] }} />

<Story name="None picked" args={{ topics: all, channels: [{ channel: 'email', view: { ...on, topics: [] } }, { channel: 'dm', view: { ...on, topics: [] } }] }} />

<Story name="Every inbox section, on a phone" args={{ topics: all, channels: some }} globals={phone} />

<!-- A ticked box saves its column on the spot, with every box ticked in it. -->
<Story
	name="Saving"
	args={{ topics: all, channels: some }}
	play={async ({ canvasElement }) => {
		const box = within(canvasElement).getByRole('checkbox', { name: 'Guests by Email' });

		await userEvent.click(box);
		await expect(enhanced).toHaveBeenCalledOnce();

		const form = box instanceof HTMLInputElement ? box.form : null;

		await expect(new FormData(form ?? undefined).getAll('topic')).toEqual(['bookings', 'guests', 'events']);
	}}
/>
