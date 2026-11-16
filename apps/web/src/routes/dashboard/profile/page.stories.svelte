<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import type { ProfileView, Viewer } from '#lib/types.ts';
	import Page from './+page.svelte';

	/** The profile editor of a DJ Find DJs lists, with Take inquiries off and on. */
	const { Story } = defineMeta({
		title: 'Patterns/Profile editor',
		parameters: { sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/dashboard/profile') } } } }
	});

	const viewer: Viewer = { did: 'did:plc:neon', handle: 'neon.test', roles: [], isAdmin: false, isStaff: false, spacesCapable: true };

	const profile: ProfileView = {
		did: viewer.did,
		uri: `at://${viewer.did}/computer.localnetwork.profile/self`,
		cid: 'bafyprofile',
		record: { basics: { name: 'Neon' }, performer: { tagline: 'Hypnotic techno' }, createdAt: '2026-10-30T00:00:00Z', updatedAt: '2026-10-30T00:00:00Z' },
		avatarUrl: null
	};

	/** Neon, listed in Find DJs, not taking inquiries yet. */
	const off = {
		viewer,
		pendingRequests: 0,
		unread: 0,
		waiting: 0,
		needs: [],
		access: Promise.resolve([]),
		projects: Promise.resolve([]),
		sections: Promise.resolve(null),
		profile,
		values: { name: 'Neon', tagline: 'Hypnotic techno', takesInquiries: false },
		sectionsOn: { performer: true, promoter: false },
		needsSignIn: false,
		listing: { listedAt: '2026-10-30T00:00:00Z' },
		listRequest: null,
		geocoder: false,
		sets: Promise.resolve([])
	};

	const on = { ...off, profile: { ...profile, record: { ...profile.record, takesInquiries: true } }, values: { ...off.values, takesInquiries: true } };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story
	name="Inquiries off"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole('checkbox', { name: 'Take inquiries' })).not.toBeChecked();
	}}
>
	<Page data={off} params={{}} form={null} />
</Story>

<Story
	name="Taking inquiries"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole('checkbox', { name: 'Take inquiries' })).toBeChecked();
	}}
>
	<Page data={on} params={{}} form={null} />
</Story>

<Story name="Taking inquiries on a phone" asChild globals={phone}><Page data={on} params={{}} form={null} /></Story>
