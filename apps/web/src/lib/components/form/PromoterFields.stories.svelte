<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import PromoterFields from './PromoterFields.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Promoter fields', component: PromoterFields, tags: ['autodocs'] });

	const values = {
		cities: [
			{ city: 'Berlin', region: '', country: 'DE' },
			{ city: 'Leipzig', region: 'Saxony', country: 'DE' }
		]
	};
</script>

<Story name="No cities yet" />

<Story name="Two cities" args={{ values }} />

<Story name="Phone, two cities" args={{ values }} globals={phone} />
