<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { FlagView } from '#lib/types.ts';
	import FlagTable from './FlagTable.svelte';

	const { Story } = defineMeta({ title: 'Staff/Flags', component: FlagTable, tags: ['autodocs'] });

	// SAFETY: stories show flags the catalog may not have yet.
	const flag = (key: string, description: string, gates: Partial<FlagView>): FlagView => ({
		key: key as FlagView['key'],
		description,
		enabled: true,
		everyone: false,
		percentage: 0,
		groups: [],
		accounts: [],
		updatedAt: null,
		...gates
	});

	const flags = [
		flag('newPlanner', 'The planner with booking notes.', { percentage: 25, groups: [{ rkey: '3lzy2ji4nms2z', name: 'Beta DJs' }], updatedAt: '2026-11-02T10:00:00Z' }),
		flag('parityCanary', 'Gates nothing.', { accounts: [{ did: 'did:plc:milo', handle: 'milo.test' }], updatedAt: '2026-11-01T09:00:00Z' }),
		flag('quietInbox', 'Fewer inbox emails.', { everyone: true, updatedAt: '2026-10-30T09:00:00Z' }),
		flag('slowCharts', 'Charts from the new index.', { enabled: false, percentage: 40, accounts: [{ did: 'did:plc:milo', handle: 'milo.test' }], updatedAt: '2026-11-08T21:00:00Z' }),
		flag('tapeDecks', 'Not turned on yet.', {})
	];
</script>

<Story name="Flags" args={{ flags }} />

<Story name="Flags on a phone" args={{ flags }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />

<Story name="No flags" args={{ flags: [] }} />
