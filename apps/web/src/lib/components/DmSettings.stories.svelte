<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
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

	/** Asks Bluesky chat again: the page reloads its data, which Storybook's SvelteKit stand-in announces on the window. */
	const tryAgain = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		let reloaded = false;
		const onReload = () => (reloaded = true);

		window.addEventListener('storybook:invalidateAll', onReload);
		await userEvent.click(await within(canvasElement).findByRole('button', { name: 'Try again' }));
		window.removeEventListener('storybook:invalidateAll', onReload);
		await expect(reloaded).toBe(true);
	};
</script>

<Story name="Off" args={{ available: true, channel: null, availability: answer('ok'), root, topics }} />

<Story name="Checking Bluesky chat" args={{ available: true, channel: null, availability: checking, root, topics }} />

<Story name="Not allowed yet" args={{ available: true, channel: held, availability: answer('not_allowed'), root, topics }} />

<Story name="Chat unreachable" args={{ available: true, channel: null, availability: answer('unavailable'), root, topics }} />

<Story name="Chat unreachable, trying again" args={{ available: true, channel: null, availability: answer('unavailable'), root, topics }} play={tryAgain} />

<Story name="On" args={{ available: true, channel: on, availability: answer('ok'), root, topics }} />

<Story name="Not set up here" args={{ available: false, channel: null, availability: null, root, topics }} />

<Story name="Phone, not allowed yet" args={{ available: true, channel: held, availability: answer('not_allowed'), root, topics }} globals={phone} />
