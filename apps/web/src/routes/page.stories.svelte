<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { toolCards } from '#lib/tools/index.ts';
	import type { ProjectView, Viewer } from '#lib/types.ts';
	import { DEMO_PROJECTS } from '../../scripts/seed-data.ts';
	import Layout from './+layout.svelte';
	import Page from './+page.svelte';

	/**
	 * The live homepage: the real `+layout.svelte` around the real `+page.svelte`, fed the seed data's
	 * projects and the real tool cards, so it can't drift from the site. Section ideas that aren't on
	 * the site are under Proposals/Homepage.
	 */
	const { Story } = defineMeta({
		title: 'Patterns/Homepage',
		parameters: { layout: 'fullscreen', sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/') } } } }
	});

	const recent: ProjectView[] = DEMO_PROJECTS.slice(0, 6).map((profile, index) => ({
		uri: `at://did:plc:localnetwork/computer.localnetwork.project/${index + 1}`,
		rkey: String(index + 1),
		cid: `cid${index + 1}`,
		slug: profile.name.toLowerCase().replaceAll(/[^a-z0-9]+/g, '-'),
		profile,
		managers: index % 3 ? ['did:plc:neon'] : [],
		status: 'active',
		revision: 1,
		createdBy: 'did:plc:neon',
		createdAt: '2026-10-01T00:00:00Z',
		updatedAt: '2026-10-01T00:00:00Z',
		avatarUrl: null
	}));

	/** Counts of each value across the seed projects, most first, the way the directory's stats list them. */
	function tally<T>(keys: T[], id: (key: T) => string) {
		const counts = new Map<string, { key: T; count: number }>();

		for (const key of keys) counts.set(id(key), { key, count: (counts.get(id(key))?.count ?? 0) + 1 });

		return [...counts.values()].sort((a, b) => b.count - a.count);
	}

	const cities = tally(
		DEMO_PROJECTS.map(({ location }) => ({ city: location.city, country: location.country })),
		(place) => `${place.city},${place.country}`
	).map(({ key, count }) => ({ ...key, count }));

	const directory = {
		recent,
		stats: { active: DEMO_PROJECTS.length, cities: cities.length, unclaimed: recent.filter((project) => !project.managers.length).length },
		genres: tally(
			DEMO_PROJECTS.flatMap((project) => project.genres),
			(genre) => genre
		)
			.slice(0, 14)
			.map(({ key, count }) => ({ value: key, count })),
		cities: cities.slice(0, 8)
	};

	const empty = { recent: [], stats: { active: 0, cities: 0, unclaimed: 0 }, genres: [], cities: [] };

	const offline = { recent: [], stats: null, genres: [], cities: [] };

	const member: Viewer = { did: 'did:plc:neon', handle: 'neon.test', roles: [], isAdmin: false, isStaff: false, spacesCapable: true };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

{#snippet home(viewer: Viewer | null, data: typeof directory | typeof empty | typeof offline)}
	{@const layout = { viewer, pendingRequests: 0 }}
	<Layout data={layout} params={{}}>
		<Page data={{ ...layout, geocoder: false, tools: toolCards(), ...data }} params={{}} form={null} />
	</Layout>
{/snippet}

<Story name="Full page" asChild>{@render home(null, directory)}</Story>

<Story name="Full page on a phone" asChild globals={phone}>{@render home(null, directory)}</Story>

<Story name="Signed in" asChild>{@render home(member, directory)}</Story>

<!-- A new directory, before anyone lists a project: the browse section is left out. -->
<Story name="Empty directory" asChild>{@render home(null, empty)}</Story>

<!-- The directory isn't answering. Search and listing still work, so the rest of the page stays. -->
<Story name="Directory offline" asChild>{@render home(null, offline)}</Story>

<Story name="Directory offline on a phone" asChild globals={phone}>{@render home(null, offline)}</Story>
