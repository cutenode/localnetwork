<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { NotificationTopic } from '#lib/types.ts';
	import ChannelTopics from './ChannelTopics.svelte';

	const topics: NotificationTopic[] = ['bookings', 'guests', 'waitlist', 'events'];

	const { Story } = defineMeta({ title: 'Dashboard/Channel topics', component: ChannelTopics, tags: ['autodocs'], args: { action: '?/emailTopics', legend: 'What to email', topics, selected: ['bookings', 'events'] } });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Some picked" />

<Story name="None picked" args={{ selected: [] }} />

<!-- What email and DMs offer today. -->
<Story name="One topic" args={{ topics: ['bookings'], selected: ['bookings'] }} />

<Story name="Phone" globals={phone} />
