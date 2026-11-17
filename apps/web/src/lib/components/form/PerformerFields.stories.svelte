<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { DjSetView, ProjectFormValues } from '#lib/types.ts';
	import PerformerFields from './PerformerFields.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Performer fields', component: PerformerFields, tags: ['autodocs'] });

	const closing: DjSetView = { uri: 'at://did:plc:neon/computer.localnetwork.dj.set/3kset', cid: 'bafyclosing', title: 'Closing', date: '2026-09-01', trackCount: 24 };

	const lowWater = { title: 'Low Water', url: 'https://www.mixcloud.com/neontide/low-water/' };

	const values: ProjectFormValues = {
		tagline: 'Hypnotic, rolling techno for late rooms',
		genres: 'techno, dub techno',
		travelRadiusKm: 800,
		international: true,
		travelNotes: 'Has a car; needs a hotel for gigs over 200 km',
		setTypes: ['peak', 'closing'],
		formats: ['vinyl', 'cdj'],
		setMin: 90,
		setMax: 240,
		capacityMin: 100,
		capacityMax: 1500,
		activeSince: 2016,
		fees: [
			{ label: 'Club nights', tier: 'established', currency: 'EUR', minAmount: '600', maxAmount: '1500' },
			{ label: 'Festivals', tier: 'headliner', currency: 'EUR', minAmount: '2000', maxAmount: '' }
		],
		mixes: [
			{
				title: 'Tidal Hours',
				url: 'https://soundcloud.com/neon-tide/tidal-hours',
				tracklist: { $type: 'blob', ref: { $link: 'bafytracklist' }, mimeType: 'application/json', size: 2048 },
				set: { uri: closing.uri, cid: closing.cid }
			},
			lowWater
		],
		highlights: 'Resident at Tresor\nBoiler Room Berlin, 2024',
		lookingFor: 'Closing slots at warehouse parties, outdoor daytime events'
	};

	/** The account's sets, still on their way. */
	const loading = new Promise<DjSetView[] | null>(() => {});

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Empty" />

<Story name="Filled in" args={{ values }} />

<Story name="With sets" args={{ values, sets: Promise.resolve([closing, { ...closing, uri: `${closing.uri}2`, cid: 'bafyopening', title: undefined, venue: 'Tresor', date: '2026-10-17' }]) }} />

<!-- The linked set's CID isn't the published one's, so the picker names it the earlier version. -->
<Story name="Linked set changed since" args={{ values, sets: Promise.resolve([{ ...closing, cid: 'bafynewer' }]) }} />

<Story name="Sets loading" args={{ values, sets: loading }} />

<Story name="Sets unavailable" args={{ values, sets: Promise.resolve(null) }} />

<Story name="No sets yet" args={{ values: { ...values, mixes: [lowWater] }, sets: Promise.resolve([]) }} />

<Story name="Phone" args={{ values, sets: Promise.resolve([closing]) }} globals={phone} />
