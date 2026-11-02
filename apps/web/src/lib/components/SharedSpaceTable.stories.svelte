<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { SharedSpaceSummary } from '#lib/types.ts';
	import SharedSpaceTable from './SharedSpaceTable.svelte';

	const { Story } = defineMeta({ title: 'Staff/Spaces', component: SharedSpaceTable, tags: ['autodocs'] });

	const space = (kind: SharedSpaceSummary['kind'], skey: string, people: number, listed: boolean, handle: string | null): SharedSpaceSummary => ({
		uri: `at://did:plc:root/space/computer.localnetwork.space.${kind}/${skey}`,
		kind,
		skey,
		listed,
		people,
		createdBy: { did: `did:plc:${skey}`, handle },
		createdAt: '2026-10-20T10:00:00Z'
	});

	const spaces = [
		space('performer', '3m2k7xj4a2c2a', 2, true, 'neon.test'),
		space('party', '3m2k7xj4a2c2b', 5, false, 'harbour.test'),
		space('venue', '3m2k7xj4a2c2c', 1, false, null)
	];
</script>

<Story name="Acts, parties and venues" args={{ spaces }} />

<Story name="Acts, parties and venues on a phone" args={{ spaces }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />

<Story name="Loading" args={{ spaces: new Promise<SharedSpaceSummary[]>(() => {}) }} />

<Story name="None yet" args={{ spaces: [] }} />

<Story name="Couldn't load" args={{ spaces: null }} />
