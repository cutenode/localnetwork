<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import VenueSettings from './VenueSettings.svelte';

	/** A venue's settings: how events naming it show, and who it blocks. */
	const { Story } = defineMeta({ title: 'Patterns/Venue settings', component: VenueSettings, tags: ['autodocs'] });

	const owner = { edit: true, block: true };

	const blocks = [
		{ rkey: '3a', kind: 'party', name: 'Night Shift' },
		{ rkey: '3b', kind: 'profile', name: '@dj.test' },
		{ rkey: '3c', kind: 'act', name: 'Crew' }
	];
</script>

<Story name="Owner, nobody blocked" args={{ approval: 'open', can: owner, blocks: [] }} />

<Story name="Owner, with blocks" args={{ approval: 'approval', can: owner, blocks }} />

<Story name="Blocks only" args={{ approval: 'open', can: { edit: false, block: true }, blocks }} />

<Story name="Block refused" args={{ approval: 'open', can: owner, blocks: [], values: { kind: 'profile', name: 'nobody.test' } }} />

<Story name="Phone, with blocks" args={{ approval: 'approval', can: owner, blocks }} globals={phone} />
