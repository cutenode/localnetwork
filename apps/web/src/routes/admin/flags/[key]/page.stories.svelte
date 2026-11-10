<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { ada } from '#lib/testing/requests.ts';
	import type { FlagGroupView, FlagView } from '#lib/types.ts';
	import FlagPage from './+page.svelte';

	/** One flag's page: who it's on for, and the switch that turns it off for everyone. */
	const { Story } = defineMeta({ title: 'Staff/Flag' });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const flag: FlagView = {
		key: 'parityCanary',
		description: 'Gates nothing. Turn it on for an account to check that the website and locallibrary give it the same answer.',
		enabled: true,
		everyone: false,
		percentage: 25,
		groups: [{ rkey: '3lzy2ji4nms2z', name: 'Beta DJs' }],
		accounts: [{ did: 'did:plc:milo', handle: 'milo.test' }],
		updatedAt: '2026-11-02T10:00:00Z'
	};

	const groups: FlagGroupView[] = [{ rkey: '3lzy2ji4nms2z', name: 'Beta DJs', description: null, members: [{ did: 'did:plc:milo', handle: 'milo.test' }], flags: ['parityCanary'] }];

	const data = (fields: Partial<FlagView> = {}) => ({
		viewer: ada,
		pendingRequests: 0,
		grants: ['flags' as const, 'flagGroups' as const],
		flag: { ...flag, ...fields },
		groups
	});
</script>

<Story name="On for a share and a group" asChild><FlagPage data={data()} params={{ key: 'parityCanary' }} form={null} /></Story>

<Story
	name="Off for everyone"
	asChild
	play={async ({ canvasElement }) => {
		await expect(within(canvasElement).getByRole('switch', { name: /^Off for everyone/ })).toBeChecked();
		await expect(within(canvasElement).getByLabelText('Percentage of accounts')).toHaveValue(25);
	}}
>
	<FlagPage data={data({ enabled: false })} params={{ key: 'parityCanary' }} form={null} />
</Story>

<Story name="Off for everyone, on a phone" asChild globals={phone}><FlagPage data={data({ enabled: false })} params={{ key: 'parityCanary' }} form={null} /></Story>

<Story name="Couldn't save" asChild><FlagPage data={data()} params={{ key: 'parityCanary' }} form={{ error: 'Could not resolve @nobody.example', values: undefined, reauth: false }} /></Story>

<Story name="No groups yet" asChild><FlagPage data={{ ...data(), groups: [] }} params={{ key: 'parityCanary' }} form={null} /></Story>
