<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { ACCESS_PRESETS, epkFormDefaults } from '#lib/epk.ts';
	import AccessMatrix from './AccessMatrix.svelte';
	import EpkAccessCard from './EpkAccessCard.svelte';
	import EpkForm from './EpkForm.svelte';
	import EpkMixes from './EpkMixes.svelte';

	const { Story } = defineMeta({ title: 'Press kits/Press kit', component: EpkAccessCard, tags: ['autodocs'] });

	const card = { name: 'Neon Tide', askHref: '/dashboard/access/did:plc:neon/3k' };

	const mixes = [
		{ title: 'Tidal Hours 004', url: 'https://soundcloud.com/example-neon-tide/tidal-hours-004', durationMinutes: 62, year: 2026 },
		{ title: 'Halle Nord closing set (promo)', url: 'https://soundcloud.com/example-neon-tide/halle-nord', onRequest: true }
	];

	const values = {
		...epkFormDefaults,
		title: 'Clubs',
		headline: 'Rolling, hypnotic techno for late rooms',
		bio: 'Neon Tide plays long, patient techno sets that build from dub textures into peak-time pressure.',
		mixes: [{ title: 'Tidal Hours 004', url: mixes[0].url, visibility: 'public' as const }],
		extraMixes: [{ title: 'Halle Nord closing set (promo)', url: mixes[1].url, visibility: 'private' as const }],
		quotes: [{ text: 'The best closing set we had all year.', source: 'Halle Nord' }],
		email: 'bookings@neon-tide.test'
	};
</script>

<Story name="Signed out" args={{ ...card, access: { kind: 'signedOut', rule: 'review' } }} />

<Story name="Signed out, straight in" args={{ ...card, access: { kind: 'signedOut', rule: 'auto' } }} />

<Story name="Can ask" args={{ ...card, access: { kind: 'canAsk', rule: 'review' } }} />

<Story name="Can ask, straight in" args={{ ...card, access: { kind: 'canAsk', rule: 'auto' } }} />

<Story name="Asking, on the dashboard" args={{ name: card.name, access: { kind: 'canAsk', rule: 'review' } }} />

<Story name="Asking, straight in" args={{ name: card.name, access: { kind: 'canAsk', rule: 'auto' } }} />

<Story name="Asked" args={{ ...card, access: { kind: 'pending', since: '2026-10-13T12:00:00Z', approved: false } }} />

<Story name="Approved, waiting for the move" args={{ ...card, access: { kind: 'pending', since: '2026-10-13T12:00:00Z', approved: true } }} />

<Story name="Declined" args={{ ...card, access: { kind: 'declined' } }} />

<Story name="Not taking requests" args={{ ...card, access: { kind: 'closed' } }} />

<Story name="Error asking" args={{ name: card.name, access: { kind: 'canAsk', rule: 'review' }, error: "This press kit isn't taking requests" }} />

<Story name="Access rules" asChild>
	<AccessMatrix access={ACCESS_PRESETS[2].access} preset={ACCESS_PRESETS[2].id} />
</Story>

<Story name="Access rules, your own mix" asChild>
	<AccessMatrix access={{ bookedWith: 'auto', following: 'none', promoters: 'review', anyone: 'none' }} preset="custom" />
</Story>

<Story name="Mixes" asChild>
	<EpkMixes {mixes} tracklistHref={(url) => (url === mixes[0].url ? '/projects/neon-tide/mixes/1' : null)} />
</Story>

<Story name="Form" asChild>
	<EpkForm {values} photoBase={null} editHref="/dashboard/projects/neon-tide/profile" submitLabel="Publish press kit" />
</Story>

<Story name="Form, nothing yet" asChild>
	<EpkForm values={epkFormDefaults} photoBase={null} editHref="/dashboard/projects/neon-tide/profile" submitLabel="Publish press kit" />
</Story>
