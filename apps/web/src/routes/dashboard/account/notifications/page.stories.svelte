<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { accountLayout } from '#lib/testing/account.ts';
	import type { ChannelView, DmAvailability } from '#lib/types.ts';
	import type { PageProps } from './$types';
	import Page from './+page.svelte';

	/** Account > Notifications: what goes where, then each channel's own setting. */
	const { Story } = defineMeta({ title: 'Patterns/Notifications' });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const on: ChannelView = { status: 'active', pausedReason: null, address: 'neon@example.test', topics: ['bookings'] };

	const data = (email: ChannelView | null, dm: ChannelView | null, availability: DmAvailability = 'ok'): PageProps['data'] => ({
		...accountLayout,
		topics: ['bookings'],
		email: { available: true, channel: email, returned: null },
		dm: { available: true, channel: dm, availability: Promise.resolve(availability), root: 'localnetwork.computer' }
	});
</script>

<Story name="Email on" asChild>
	<Page params={{}} form={null} data={data(on, null)} />
</Story>

<Story name="Email on, on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={data(on, null)} />
</Story>

<Story name="Both off" asChild>
	<Page params={{}} form={null} data={data(null, null)} />
</Story>

<Story name="Waiting on the link, DMs held, on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={data({ ...on, status: 'paused', pausedReason: 'email_unverified' }, { ...on, address: null, status: 'paused', pausedReason: 'dm_not_allowed' }, 'not_allowed')} />
</Story>
