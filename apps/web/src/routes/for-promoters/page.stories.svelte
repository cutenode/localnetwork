<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { MatchResult } from '#lib/matching.ts';
	import { PAGE_META } from '#lib/testing/page-meta.ts';
	import type { ProjectView, Viewer } from '#lib/types.ts';
	import { DEMO_PROJECTS } from '../../../scripts/seed-data.ts';
	import Layout from '../+layout.svelte';
	import type { PageData } from './$types';
	import Page from './+page.svelte';

	/** The promoters' landing page in the real layout, fed the seed data's projects as the directory would send them. */
	const { Story } = defineMeta({
		title: 'Patterns/For promoters',
		parameters: { layout: 'fullscreen', sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/for-promoters') } } } }
	});

	const listings: ProjectView[] = DEMO_PROJECTS.slice(0, 3).map((profile, index) => ({
		uri: `at://did:plc:localnetwork/computer.localnetwork.project/${index + 1}`,
		rkey: String(index + 1),
		cid: `cid${index + 1}`,
		slug: profile.name.toLowerCase().replaceAll(/[^a-z0-9]+/g, '-'),
		profile,
		managers: ['did:plc:neon'],
		status: 'active',
		revision: 1,
		createdBy: 'did:plc:neon',
		createdAt: '2026-10-01T00:00:00Z',
		updatedAt: '2026-10-01T00:00:00Z',
		avatarUrl: null
	}));

	const results: MatchResult[] = listings.map((candidate, index) => ({
		candidate,
		score: 92 - index * 11,
		breakdown: { genre: 100, distance: 90, budget: 80, capacity: 70, setType: 60, format: 50 },
		reasons: [`Plays ${candidate.profile.genres[0]}`, 'Local', 'Fits budget'].slice(0, 3 - index),
		warnings: []
	}));

	const found = { example: listings[0], djs: DEMO_PROJECTS.length, cities: 5, unclaimed: 2 };

	const shortlist = { genre: listings[0].profile.genres[0], place: 'Berlin, DE', href: '/match?go=1', results };

	const loading = new Promise<never>(() => {});

	const member: Viewer = { did: 'did:plc:pat', handle: 'pat.test', roles: [], isAdmin: false, isStaff: false, spacesCapable: true };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	type State = Pick<PageData, 'directory' | 'shortlist' | 'promoting'>;

	const live: State = { directory: Promise.resolve(found), shortlist: Promise.resolve(shortlist), promoting: Promise.resolve(false) };
</script>

{#snippet landing(viewer: Viewer | null, state: State)}
	{@const layout = { viewer, pendingRequests: 0, meta: PAGE_META }}
	<Layout data={layout} params={{}}>
		<Page data={{ ...layout, geocoder: false, ...state }} params={{}} form={null} />
	</Layout>
{/snippet}

<Story name="Full page" asChild>{@render landing(null, live)}</Story>

<Story name="Full page on a phone" asChild globals={phone}>{@render landing(null, live)}</Story>

<!-- The listing, the shortlist and the closing call each hold their room until the directory answers. -->
<Story name="Loading" asChild>{@render landing(null, { directory: loading, shortlist: loading, promoting: loading })}</Story>

<Story name="Loading on a phone" asChild globals={phone}>{@render landing(null, { directory: loading, shortlist: loading, promoting: loading })}</Story>

<Story name="Signed in" asChild>{@render landing(member, live)}</Story>

<!-- Someone with the promoter section on gets their planner instead of the call to turn it on. -->
<Story name="Already promoting" asChild>{@render landing(member, { ...live, promoting: Promise.resolve(true) })}</Story>

<!-- A new directory, before anyone is listed. -->
<Story name="Empty directory" asChild>{@render landing(null, { ...live, directory: Promise.resolve({ example: null, djs: 0, cities: 0, unclaimed: 0 }), shortlist: Promise.resolve({ genre: null, place: null, href: '/match', results: [] }) })}</Story>

<Story name="Directory offline" asChild>{@render landing(null, { directory: Promise.resolve(null), shortlist: Promise.resolve(null), promoting: Promise.resolve(false) })}</Story>

<Story name="Directory offline on a phone" asChild globals={phone}>{@render landing(null, { directory: Promise.resolve(null), shortlist: Promise.resolve(null), promoting: Promise.resolve(false) })}</Story>
