<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ChannelView, NotificationTopic } from '#lib/types.ts';
	import EmailSettings from './EmailSettings.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/Email settings', component: EmailSettings, tags: ['autodocs'] });

	const topics: NotificationTopic[] = ['bookings'];

	const on: ChannelView = { status: 'active', pausedReason: null, address: 'neon@example.test', topics: ['bookings'] };

	const paused = (pausedReason: ChannelView['pausedReason']): ChannelView => ({ ...on, status: 'paused', pausedReason });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Off" args={{ available: true, channel: null, topics }} />

<Story name="Declined" args={{ available: true, channel: null, returned: 'declined', topics }} />

<Story name="Waiting on the link" args={{ available: true, channel: paused('email_unverified'), topics }} />

<Story name="Link held back" args={{ available: true, channel: paused('email_unverified'), returned: 'limited', topics }} />

<Story name="Host hasn't confirmed" args={{ available: true, channel: paused('email_unconfirmed'), topics }} />

<Story name="Address refused" args={{ available: true, channel: paused('address_rejected'), topics }} />

<Story name="On" args={{ available: true, channel: on, topics }} />

<Story name="Test sent" args={{ available: true, channel: on, topics, form: { success: 'Sent. Check your email.' } }} />

<Story name="Not set up here" args={{ available: false, channel: null, topics }} />

<Story name="Phone, on" args={{ available: true, channel: on, topics }} globals={phone} />

<Story name="Phone, waiting on the link" args={{ available: true, channel: paused('email_unverified'), topics }} globals={phone} />
