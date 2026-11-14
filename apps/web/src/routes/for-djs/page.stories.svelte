<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { slimCapture } from '#lib/components/tools/sources.ts';
	import { PAGE_META } from '#lib/testing/page-meta.ts';
	import { findTool } from '#lib/tools/index.ts';
	import type { ProjectView, ShowView, SupportedTrack } from '#lib/types.ts';
	import { DEMO_PROJECTS } from '../../../scripts/seed-data.ts';
	import Layout from '../+layout.svelte';
	import type { PageData } from './$types';
	import Page from './+page.svelte';

	/** The DJs' landing page in the real layout, fed the seed data and locallibrary's real captures. */
	const { Story } = defineMeta({
		title: 'Patterns/For DJs',
		parameters: { layout: 'fullscreen', sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/for-djs') } } } }
	});

	const example: ProjectView = {
		uri: 'at://did:plc:localnetwork/computer.localnetwork.project/1',
		rkey: '1',
		cid: 'cid1',
		slug: 'neon-tide',
		profile: DEMO_PROJECTS[0],
		managers: ['did:plc:neon'],
		status: 'active',
		revision: 1,
		createdBy: 'did:plc:neon',
		createdAt: '2026-10-01T00:00:00Z',
		updatedAt: '2026-10-01T00:00:00Z',
		avatarUrl: null
	};

	const shows: ShowView[] = ['Tidal Hours', 'Rooftop Social', 'Basement Sessions'].map((name, index) => ({
		uri: `at://did:plc:neon/computer.localnetwork.show/${index + 1}`,
		cid: `show${index + 1}`,
		did: 'did:plc:neon',
		rkey: String(index + 1),
		project: example,
		record: {
			project: example.uri,
			event: { uri: `at://did:plc:neon/community.lexicon.calendar.event/${index + 1}`, cid: `event${index + 1}` },
			name,
			startsAt: `2026-11-${13 + index}T23:00:00+01:00`,
			endsAt: `2026-11-${14 + index}T05:00:00+01:00`,
			venue: ['Halle Nord', 'Klunkerkranich', 'Arches Yard'][index],
			location: example.profile.location,
			createdAt: '2026-11-01T00:00:00Z'
		}
	}));

	const tracks: SupportedTrack[] = [
		['Night Bus', 'Ferrule', 9],
		['Halogen', 'Two Rooms', 7],
		['Static Bloom', 'Kestrel Unit', 6],
		['Signal Path', 'Mira Okonkwo', 4],
		['A Very Long Track Title That Has To Truncate Somewhere', 'An Artist With A Long Name', 1]
	].map(([title, artist, supports], index) => ({ id: String(index), title: String(title), artist: String(artist), artists: [], supports: Number(supports) }));

	const captures = (findTool('locallibrary')?.cues ?? []).filter((cue) => ['Sync', 'Tracklist'].includes(cue.name)).map((cue) => ({ line: cue.line, capture: slimCapture(cue.capture) }));

	type State = Pick<PageData, 'directory' | 'shows' | 'tracks'>;

	const live: State = { directory: Promise.resolve({ example, djs: 12, cities: 10, unclaimed: 7 }), shows: Promise.resolve(shows), tracks: Promise.resolve(tracks) };

	const loading = new Promise<never>(() => {});

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

{#snippet landing(state: State)}
	{@const layout = { viewer: null, pendingRequests: 0, meta: PAGE_META }}
	<Layout data={layout} params={{}}>
		<Page data={{ ...layout, captures, ...state }} params={{}} form={null} />
	</Layout>
{/snippet}

<Story name="Full page" asChild>{@render landing(live)}</Story>

<Story name="Full page on a phone" asChild globals={phone}>{@render landing(live)}</Story>

<!-- The listing, the charts and the shows each hold their room until the directory answers. -->
<Story name="Loading" asChild>{@render landing({ directory: loading, shows: loading, tracks: loading })}</Story>

<Story name="Empty directory" asChild>{@render landing({ directory: Promise.resolve({ example: null, djs: 0, cities: 0, unclaimed: 0 }), shows: Promise.resolve([]), tracks: Promise.resolve([]) })}</Story>

<Story name="Directory offline" asChild>{@render landing({ directory: Promise.resolve(null), shows: Promise.resolve(null), tracks: Promise.resolve(null) })}</Story>

<Story name="Directory offline on a phone" asChild globals={phone}>{@render landing({ directory: Promise.resolve(null), shows: Promise.resolve(null), tracks: Promise.resolve(null) })}</Story>
