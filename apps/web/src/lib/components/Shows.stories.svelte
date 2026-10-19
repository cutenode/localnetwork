<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { EVENT_STATUS, showFormDefaults } from '#lib/shows.ts';
	import type { ProjectView, ShowView } from '#lib/types.ts';
	import FollowButton from './FollowButton.svelte';
	import ShowForm from './ShowForm.svelte';
	import ShowList from './ShowList.svelte';
	import SignInAgain from './SignInAgain.svelte';

	const { Story } = defineMeta({ title: 'Shows/Shows', component: ShowList, tags: ['autodocs'] });

	const project = (slug: string, name: string): ProjectView => ({
		uri: `at://did:plc:root/computer.localnetwork.project/${slug}`,
		rkey: slug,
		cid: 'c',
		slug,
		profile: { name, kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
		managers: ['did:plc:neon'],
		status: 'active',
		revision: 1,
		createdBy: 'did:plc:ada',
		createdAt: '2026-01-01T00:00:00Z',
		updatedAt: '2026-01-01T00:00:00Z',
		avatarUrl: null
	});

	const tide = project('neon-tide', 'Neon Tide');

	const show = (rkey: string, record: Partial<ShowView['record']>, extra: Partial<ShowView> = {}): ShowView => ({
		uri: `at://did:plc:neon/computer.localnetwork.show/${rkey}`,
		cid: 'c',
		did: 'did:plc:neon',
		rkey,
		project: tide,
		record: {
			project: tide.uri,
			event: { uri: `at://did:plc:neon/community.lexicon.calendar.event/${rkey}`, cid: 'e' },
			name: 'Tidal Hours',
			startsAt: '2026-11-07T23:00:00+01:00',
			endsAt: '2026-11-08T06:00:00+01:00',
			venue: 'Halle Nord',
			location: { city: 'Berlin', country: 'DE' },
			status: EVENT_STATUS.scheduled,
			tickets: 'https://tickets.example/tidal',
			createdAt: '2026-10-01T00:00:00Z',
			...record
		},
		...extra
	});

	const shows = [
		show('a', {}, { distanceKm: 2.4 }),
		show('b', { name: 'Saffron Social on the roof', startsAt: '2026-11-14T15:00:00+01:00', endsAt: '2026-11-14T22:00:00+01:00', venue: 'Klunkerkranich', tickets: undefined }, { project: project('saffron-social', 'Saffron Social'), distanceKm: 4.1 }),
		show('c', { name: 'Warehouse all-nighter', startsAt: '2026-11-21T22:00:00+01:00', endsAt: undefined, venue: undefined, location: { city: 'Potsdam', country: 'DE' }, status: EVENT_STATUS.rescheduled }, { distanceKm: 26.3 }),
		show('d', { name: 'Closing party', startsAt: '2026-11-28T23:00:00+01:00', status: EVENT_STATUS.cancelled }, { distanceKm: 2.4 })
	];

	const empty = { title: 'Nothing on near Berlin yet', body: 'Try a wider radius, or look everywhere.' };

	const values = {
		...showFormDefaults,
		name: 'Tidal Hours',
		date: '2026-11-07',
		start: '23:00',
		end: '06:00',
		venue: 'Halle Nord',
		city: 'Berlin',
		region: 'Berlin',
		country: 'DE',
		latE6: '52520000',
		lngE6: '13405000',
		tickets: 'https://tickets.example/tidal'
	};
</script>

<Story name="Near a city" args={{ shows, empty }} />

<Story name="Near a city on a phone" args={{ shows, empty }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />

<Story name="On a project's page, for its manager" asChild>
	<ShowList shows={shows.slice(0, 2)} project={tide.uri} editable={(entry) => (entry.did === 'did:plc:neon' ? `/projects/neon-tide/shows/${entry.rkey}` : null)} edited={[shows[0].uri]} {empty} />
</Story>

<Story name="Loading" args={{ shows: undefined, rows: 3, empty }} />

<Story name="Nothing on" args={{ shows: [], empty }} />

<Story name="Nothing on, with a next step" asChild>
	<ShowList shows={[]} empty={{ title: 'Nothing coming up from DJs you follow', body: 'Follow DJs on their pages and their shows land here.' }}>
		<a class="btn-secondary btn-sm" href="/projects">Find DJs</a>
	</ShowList>
</Story>

<Story name="Offline" args={{ shows: null, empty, retryHref: '/shows' }} />

<Story name="Offline on a phone" args={{ shows: null, empty, retryHref: '/shows' }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />

<Story name="Follow" asChild><FollowButton follow={{ followers: 12, following: false }} /></Story>

<Story name="Following" asChild><FollowButton follow={{ followers: 1, following: true }} /></Story>

<Story name="Follow, loading" asChild><FollowButton follow={undefined} /></Story>

<Story name="Follow, signed out" asChild><FollowButton follow={{ followers: 1204, following: false }} loginHref="/login" /></Story>

<Story name="Follow, offline" asChild><FollowButton follow={null} /></Story>

<Story name="Sign in again" asChild><SignInAgain did="did:plc:neon" next="/projects/neon-tide" error="Sign in again to follow DJs" /></Story>

<Story name="Announce form" asChild><ShowForm values={showFormDefaults} geocoder={false} /></Story>

<Story name="Announce form, from a booking" asChild><ShowForm {values} geocoder={false} /></Story>

<Story name="Announce form, editing" asChild><ShowForm values={{ ...values, cancelled: true }} geocoder={false} editing /></Story>

<Story name="Announce form on a phone" asChild globals={{ viewport: { value: 'mobile2', isRotated: false } }}><ShowForm {values} geocoder={false} editing /></Story>
