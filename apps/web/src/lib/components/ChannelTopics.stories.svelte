<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ChannelView, NotificationTopic } from '#lib/types.ts';
	import ChannelTopics from './ChannelTopics.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/What goes where', component: ChannelTopics, tags: ['autodocs'] });

	const on: ChannelView = { status: 'active', pausedReason: null, address: 'neon@example.test', topics: ['bookings'] };

	const all: NotificationTopic[] = ['bookings', 'guests', 'waitlist', 'events'];

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Email on, DMs off" args={{ topics: ['bookings'], channels: [{ channel: 'email', view: on }, { channel: 'dm', view: null }] }} />

<Story name="DMs paused" args={{ topics: ['bookings'], channels: [{ channel: 'email', view: { ...on, topics: [] } }, { channel: 'dm', view: { ...on, status: 'paused', pausedReason: 'dm_not_allowed' } }] }} />

<Story name="Only the site" args={{ topics: ['bookings'], channels: [] }} />

<Story name="Every topic, on a phone" args={{ topics: all, channels: [{ channel: 'email', view: { ...on, topics: ['bookings', 'events'] } }, { channel: 'dm', view: on }] }} globals={phone} />
