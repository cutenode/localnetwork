<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { ada } from '#lib/testing/requests.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import type { FlagGroupView, FlagView } from '#lib/types.ts';
	import FlagPage from './+page.svelte';

	/** One flag's page: who it's on for, the switch that turns it off for everyone, and a preview for this browser. */
	const { Story } = defineMeta({ title: 'Staff/Flag' });

	const flag: FlagView = {
		key: 'spacePages',
		description: "The public act, party, venue and event pages. Off, they answer 404, and no listing is written until it's on for everyone, since a listing points at a public page.",
		showsOn: ['act', 'party', 'venue', 'event'],
		removeBy: '2027-02-12',
		enabled: true,
		everyone: false,
		percentage: 25,
		groups: [{ rkey: '3lzy2ji4nms2z', name: 'Beta DJs' }],
		accounts: [{ did: 'did:plc:milo', handle: 'milo.test' }],
		updatedAt: '2026-11-02T10:00:00Z',
		readyToRemove: false
	};

	const groups: FlagGroupView[] = [{ rkey: '3lzy2ji4nms2z', name: 'Beta DJs', description: null, members: [{ did: 'did:plc:milo', handle: 'milo.test' }], flags: ['spacePages'] }];

	/** A release flag on for everyone past its time: what the page asks staff to delete. */
	const calendar: Partial<FlagView> = {
		key: 'dashboardCalendar',
		description: 'Shows Calendar in the dashboard sidebar and serves /dashboard/calendar, every night for the year ahead.',
		showsOn: [],
		removeBy: '2027-02-15',
		everyone: true,
		readyToRemove: true
	};

	const data = (fields: Partial<FlagView> = {}, flagPreview?: Record<string, boolean>) => ({
		viewer: ada,
		pendingRequests: 0,
		grants: ['flags' as const, 'flagGroups' as const],
		flag: { ...flag, ...fields },
		groups,
		flagPreview
	});
</script>

<Story name="On for a share and a group" asChild><FlagPage data={data()} params={{ key: 'spacePages' }} form={null} /></Story>

<Story
	name="Off for everyone"
	asChild
	play={async ({ canvasElement }) => {
		await expect(within(canvasElement).getByRole('switch', { name: /^Off for everyone/ })).toBeChecked();
		await expect(within(canvasElement).getByLabelText('Percentage of people')).toHaveValue(25);
	}}
>
	<FlagPage data={data({ enabled: false })} params={{ key: 'spacePages' }} form={null} />
</Story>

<Story name="Off for everyone, on a phone" asChild globals={phone}><FlagPage data={data({ enabled: false })} params={{ key: 'spacePages' }} form={null} /></Story>

<Story
	name="Previewing it on in this browser"
	asChild
	play={async ({ canvasElement }) => {
		await expect(within(canvasElement).getByRole('button', { name: 'On for me' })).toHaveAttribute('aria-pressed', 'true');
		await expect(within(canvasElement).getByRole('button', { name: 'Stop previewing' })).toBeVisible();
	}}
>
	<FlagPage data={data({}, { spacePages: true })} params={{ key: 'spacePages' }} form={null} />
</Story>

<Story name="Previewing, on a phone" asChild globals={phone}><FlagPage data={data({}, { spacePages: false })} params={{ key: 'spacePages' }} form={null} /></Story>

<Story name="Couldn't save" asChild><FlagPage data={data()} params={{ key: 'spacePages' }} form={{ error: 'Could not resolve @nobody.example', values: undefined, reauth: false }} /></Story>

<Story name="Shows on every page" asChild><FlagPage data={data({ showsOn: ['global'] })} params={{ key: 'spacePages' }} form={null} /></Story>

<Story name="Shows on no page kind" asChild><FlagPage data={data({ showsOn: [] })} params={{ key: 'spacePages' }} form={null} /></Story>

<Story name="No groups yet" asChild><FlagPage data={{ ...data(), groups: [] }} params={{ key: 'spacePages' }} form={null} /></Story>

<!-- A release flag that's been on for everyone long enough: the page says what to delete. -->
<Story
	name="Ready to remove"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);

		await expect(canvas.getByText('Ready to remove')).toBeVisible();
		await expect(canvas.getByText('Feb 15, 2027')).toBeVisible();
	}}
>
	<FlagPage data={data(calendar)} params={{ key: 'dashboardCalendar' }} form={null} />
</Story>

<Story name="Ready to remove, on a phone" asChild globals={phone}><FlagPage data={data(calendar)} params={{ key: 'dashboardCalendar' }} form={null} /></Story>
