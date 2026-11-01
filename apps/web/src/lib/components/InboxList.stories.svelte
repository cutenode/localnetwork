<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { NotificationView } from '#lib/types.ts';
	import InboxList from './InboxList.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/Inbox', component: InboxList, tags: ['autodocs'] });

	const item = (id: string, overrides: Partial<NotificationView> = {}): NotificationView => ({
		id,
		topic: 'bookings',
		title: 'Neon Tide sent a message',
		body: 'Warehouse night',
		href: '/dashboard/bookings/3mabc',
		occurredAt: '2026-10-28T21:15:00Z',
		read: false,
		...overrides
	});

	const items = [
		item('a'),
		item('b', { title: 'New booking inquiry for Neon Tide', body: 'Halloween at the Depot, a long title that runs past the edge of a phone', occurredAt: '2026-10-27T18:00:00Z' }),
		item('c', { title: 'The promoter marked the booking confirmed', occurredAt: '2026-10-20T12:00:00Z', read: true }),
		item('d', { topic: null, title: 'Something changed', body: undefined, occurredAt: '2026-10-02T09:30:00Z', read: true })
	];

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Unread and read" args={{ items }} />

<Story name="All read" args={{ items: items.map((listed) => ({ ...listed, read: true })) }} />

<Story name="Empty" args={{ items: [] }} />

<Story name="Phone" args={{ items }} globals={phone} />
