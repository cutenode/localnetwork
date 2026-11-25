<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { light } from '#lib/testing/light.ts';
	import { desktop, phone } from '#lib/testing/viewports.ts';
	import WaitingEvents from './WaitingEvents.svelte';

	/**
	 * What a venue's approvers see: upcoming events naming it that are waiting for the venue, each to approve, or nothing
	 * waiting. Each at 1280px in the dark palette and at 390px in the light one.
	 */
	const { Story } = defineMeta({
		title: 'Patterns/Waiting for the venue',
		component: WaitingEvents,
		tags: ['autodocs']
	});

	const event = (rkey: string, name: string, startsAt: string, party: string | null, lineup: number) => ({
		uri: `at:space/did:plc:root/computer.localnetwork.space.party/3party/did:plc:root/computer.localnetwork.event/${rkey}`,
		name,
		startsAt,
		party,
		lineup
	});

	const waiting = [
		event('3a', 'Night Shift: Winter Opening', '2026-12-05T23:00:00+01:00', 'Night Shift', 4),
		event('3b', 'Closing Party With a Very Long Name That Runs On Past the Edge of the Card', '2026-12-31T22:00:00+01:00', null, 0)
	];
</script>

<Story name="Waiting" args={{ waiting }} globals={desktop} />

<Story name="Waiting, phone, light" args={{ waiting }} globals={phone} beforeEach={light} />

<Story name="Nothing waiting" args={{ waiting: [] }} globals={desktop} />

<Story name="Nothing waiting, phone, light" args={{ waiting: [] }} globals={phone} beforeEach={light} />
