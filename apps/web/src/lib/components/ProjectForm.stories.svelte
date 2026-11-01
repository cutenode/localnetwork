<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { draftKey } from '#lib/form-draft.svelte.ts';
	import type { DjSetView, ProjectFormValues } from '#lib/types.ts';
	import ProjectForm from './ProjectForm.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Project form', component: ProjectForm, tags: ['autodocs'] });

	const values: ProjectFormValues = {
		name: 'Neon Tide',
		kind: 'solo',
		genres: 'techno, dub techno',
		city: 'Berlin',
		country: 'DE',
		fees: [
			{ label: 'Club nights', tier: 'established', currency: 'EUR', minAmount: '600', maxAmount: '1500' },
			{ label: 'Festivals', tier: 'headliner', currency: 'EUR', minAmount: '2000', maxAmount: '' },
			{ label: 'Community and benefit events', tier: 'community', currency: '', minAmount: '', maxAmount: '' }
		]
	};

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const mixes = { ...values, mixes: [{ title: 'Tidal Hours', url: 'https://soundcloud.com/neon-tide/tidal-hours' }] };

	/** The managers' sets, still on their way. */
	const loading = new Promise<DjSetView[] | null>(() => {});

	const draft = draftKey({ did: 'did:plc:story' }, 'project:story');

	/** Seeds an unsent draft for the story's form, saved before or after the form's own values last changed. */
	const withDraft = (base: string) => () => {
		const saved = { savedAt: new Date(Date.now() - 20 * 60_000).toISOString(), base, values: { ...values, tagline: 'Hypnotic, rolling techno for late rooms', bio: 'Half-written bio, still going.' } };

		localStorage.setItem(draft!, JSON.stringify(saved));

		return () => localStorage.removeItem(draft!);
	};
</script>

<Story name="New project" args={{ geocoder: false }} />

<Story name="Fee levels" args={{ values, geocoder: false }} />

<Story name="Phone, new project" args={{ geocoder: false }} globals={phone} />

<Story name="Phone, fee levels" args={{ values, geocoder: false }} globals={phone} />

<Story name="Sets loading" args={{ values: mixes, sets: loading, geocoder: false }} />

<Story name="Sets unavailable" args={{ values: mixes, sets: Promise.resolve(null), geocoder: false }} />

<Story name="No sets yet" args={{ values: mixes, sets: Promise.resolve([]), geocoder: false }} />

<!-- The seeded base matches nothing the form reads, so these also show the note that the saved version changed. -->
<Story name="Unsent draft" args={{ values, draft, geocoder: false }} beforeEach={withDraft('a version before this one')} />

<Story name="Phone, unsent draft" args={{ values, draft, geocoder: false }} beforeEach={withDraft('a version before this one')} globals={phone} />
