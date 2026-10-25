<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { RequestView, Viewer } from '#lib/types.ts';
	import ManageLink from './ManageLink.svelte';
	import PreviewBanner from './PreviewBanner.svelte';
	import RequestList from './RequestList.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/Manage link', component: ManageLink, tags: ['autodocs'] });

	const viewer = (did: string, isStaff = false): Viewer => ({ did, handle: null, roles: isStaff ? ['moderator'] : [], isAdmin: false, isStaff, spacesCapable: true });

	const project = { slug: 'neon-tide', managers: ['did:plc:neon'] };

	const request = (rkey: string, status: RequestView['status'], name: string): RequestView => ({
		uri: `at://did:plc:pat/computer.localnetwork.moderation.request/${rkey}`,
		cid: 'c',
		rkey,
		kind: 'claim',
		status,
		requester: { did: 'did:plc:pat', handle: 'pat.test' },
		repoDid: 'did:plc:pat',
		projectUri: null,
		project: { slug: rkey, name },
		record: { kind: 'claim', createdAt: '2026-10-01T00:00:00Z' },
		createdAt: '2026-10-01T00:00:00Z',
		updatedAt: '2026-10-01T00:00:00Z',
		decision: null,
		resultProjectSlug: null
	});
</script>

<Story name="Manager" args={{ viewer: viewer('did:plc:neon'), project }} />

<Story name="Manager, small" args={{ viewer: viewer('did:plc:neon'), project, section: 'press-kits', size: 'sm' }} />

<Story name="Staff" args={{ viewer: viewer('did:plc:mod', true), project }} />

<Story name="Visitor (nothing)" args={{ viewer: null, project }} />

<Story name="Not public yet" asChild>
	<PreviewBanner status="hidden" />
</Story>

<Story name="Archived" asChild>
	<PreviewBanner status="archived" />
</Story>

<Story name="Requests" asChild>
	<div class="card max-w-xl p-0">
		<RequestList requests={[request('halflight', 'pending', 'Halflight'), request('saffron', 'approved', 'Saffron Social'), request('neon', 'rejected', 'Neon Tide')]} />
	</div>
</Story>

<Story name="Requests, none" asChild>
	<div class="card max-w-xl p-0"><RequestList requests={[]} /></div>
</Story>
