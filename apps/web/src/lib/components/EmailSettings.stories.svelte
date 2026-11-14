<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ChannelView } from '#lib/types.ts';
	import EmailSettings from './EmailSettings.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/Email settings', component: EmailSettings, tags: ['autodocs'] });

	const on: ChannelView = { status: 'active', pausedReason: null, address: 'neon@example.test', topics: ['bookings'] };

	const paused = (pausedReason: ChannelView['pausedReason']): ChannelView => ({ ...on, status: 'paused', pausedReason });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Off" args={{ available: true, channel: null }} />

<Story name="Declined" args={{ available: true, channel: null, returned: 'declined' }} />

<Story name="Waiting on the link" args={{ available: true, channel: paused('email_unverified') }} />

<Story name="Link held back" args={{ available: true, channel: paused('email_unverified'), returned: 'limited' }} />

<Story name="Host hasn't confirmed" args={{ available: true, channel: paused('email_unconfirmed') }} />

<!-- No address came back from the host yet, so the status names none. -->
<Story name="Host hasn't confirmed, no address" args={{ available: true, channel: { ...paused('email_unconfirmed'), address: null } }} />

<Story name="Address refused" args={{ available: true, channel: paused('address_rejected') }} />

<!-- A pause stored without its reason reads as the host not having confirmed the address. -->
<Story name="Paused, no reason" args={{ available: true, channel: paused(null) }} />

<Story name="Paused for another reason" args={{ available: true, channel: paused('dm_not_allowed') }} />

<Story name="On" args={{ available: true, channel: on }} />

<Story name="Test sent" args={{ available: true, channel: on, form: { success: 'Sent. Check your email.' } }} />

<Story name="Not set up here" args={{ available: false, channel: null }} />

<Story name="Phone, on" args={{ available: true, channel: on }} globals={phone} />

<Story name="Phone, waiting on the link" args={{ available: true, channel: paused('email_unverified') }} globals={phone} />
