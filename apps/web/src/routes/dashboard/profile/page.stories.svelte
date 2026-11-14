<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ProfileView, Viewer } from '#lib/types.ts';
	import Page from './+page.svelte';

	/** The profile editor right after a save, which points to the Promoter section when the save turned promoting on. */
	const { Story } = defineMeta({ title: 'Patterns/Dashboard profile' });

	const viewer: Viewer = { did: 'did:plc:pat', handle: 'pat.test', roles: [], isAdmin: false, isStaff: false, spacesCapable: true };

	const profile: ProfileView = {
		did: viewer.did,
		uri: `at://${viewer.did}/computer.localnetwork.profile/self`,
		cid: 'bafyprofile',
		record: { basics: { name: 'Pat' }, promoter: {}, createdAt: '2026-10-30T00:00:00Z', updatedAt: '2026-11-12T00:00:00Z' },
		avatarUrl: null
	};

	const data = {
		viewer,
		pendingRequests: 0,
		unread: 0,
		waiting: 0,
		needs: [],
		access: Promise.resolve([]),
		projects: Promise.resolve([]),
		sections: Promise.resolve({ performer: false, promoter: true }),
		profile,
		values: { name: 'Pat' },
		sectionsOn: { performer: false, promoter: true },
		needsSignIn: false,
		listing: null,
		listRequest: null,
		geocoder: false,
		sets: Promise.resolve([])
	};

	const at = (search: string) => ({ sveltekit_experimental: { state: { page: { url: new URL(`https://localnetwork.computer/dashboard/profile${search}`) } } } });
</script>

<Story name="Saved" asChild parameters={at('?saved=1')}><Page params={{}} form={null} {data} /></Story>

<Story name="Saved, promoting just turned on" asChild parameters={at('?saved=1&promoting=1')}><Page params={{}} form={null} {data} /></Story>
