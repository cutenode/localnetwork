<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ChannelView, DmAvailability, NotificationTopic } from '#lib/types.ts';
	import DmSettings from './DmSettings.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/DM settings', component: DmSettings, tags: ['autodocs'] });

	const topics: NotificationTopic[] = ['bookings'];

	const on: ChannelView = { status: 'active', pausedReason: null, address: null, topics: ['bookings'] };

	const held: ChannelView = { ...on, status: 'paused', pausedReason: 'dm_not_allowed' };

	const root = 'localnetwork.computer';

	const answer = (state: DmAvailability) => Promise.resolve(state);

	/** Bluesky chat taking its time. */
	const checking = new Promise<DmAvailability>(() => {});

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Off" args={{ available: true, channel: null, availability: answer('ok'), root, topics }} />

<Story name="Checking Bluesky chat" args={{ available: true, channel: null, availability: checking, root, topics }} />

<Story name="Needs a follow" args={{ available: true, channel: held, availability: answer('needs_follow'), root, topics }} />

<Story name="DMs closed" args={{ available: true, channel: held, availability: answer('closed'), root, topics }} />

<Story name="Chat unreachable" args={{ available: true, channel: null, availability: answer('unavailable'), root, topics }} />

<Story name="On" args={{ available: true, channel: on, availability: answer('ok'), root, topics }} />

<Story name="Not set up here" args={{ available: false, channel: null, availability: null, root, topics }} />

<Story name="Phone, needs a follow" args={{ available: true, channel: held, availability: answer('needs_follow'), root, topics }} globals={phone} />
