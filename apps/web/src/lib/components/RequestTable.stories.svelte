<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { RequestView } from '#lib/types.ts';
	import RequestTable from './RequestTable.svelte';

	const { Story } = defineMeta({ title: 'Staff/Requests', component: RequestTable, tags: ['autodocs'] });

	const request = (rkey: string, kind: RequestView['kind'], status: RequestView['status'], name: string, onBehalfOf?: string): RequestView => ({
		uri: `at://did:plc:pat/computer.localnetwork.moderation.request/${rkey}`,
		cid: 'c',
		rkey,
		kind,
		status,
		requester: { did: 'did:plc:pat', handle: 'pat.test' },
		repoDid: 'did:plc:pat',
		projectUri: null,
		project: { slug: rkey, name },
		record: { kind, onBehalfOf, createdAt: '2026-10-01T00:00:00Z' },
		createdAt: '2026-10-01T00:00:00Z',
		updatedAt: '2026-10-01T00:00:00Z',
		decision: null,
		resultProjectSlug: null
	});

	const requests = [
		request('3m2k7xj4a2c2a', 'claim', 'pending', 'Neon Tide'),
		request('3m2k7xj4a2c2b', 'create', 'changes_requested', 'Pirate Signal Sound System', 'did:plc:pat'),
		request('3m2k7xj4a2c2c', 'update', 'approved', 'Harbour Lights'),
		// A DJ asking to be listed in Find DJs names no project, so it reads as the DJ.
		{ ...request('3m2k7xj4a2c2d', 'list', 'pending', ''), project: null }
	];
</script>

<Story name="Requests" args={{ requests }} />

<Story name="Requests on a phone" args={{ requests }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />

<Story name="Nothing here" args={{ requests: [] }} />
