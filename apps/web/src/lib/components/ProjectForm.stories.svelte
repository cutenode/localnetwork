<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ProjectFormValues } from '#lib/types.ts';
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
</script>

<Story name="New project" args={{ geocoder: false }} />

<Story name="Fee levels" args={{ values, geocoder: false }} />

<Story name="Phone, new project" args={{ geocoder: false }} globals={phone} />

<Story name="Phone, fee levels" args={{ values, geocoder: false }} globals={phone} />
