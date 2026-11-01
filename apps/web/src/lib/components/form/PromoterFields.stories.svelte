<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import PromoterFields from './PromoterFields.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Promoter fields', component: PromoterFields, tags: ['autodocs'] });

	const values = {
		cities: [
			{ city: 'Berlin', region: '', country: 'DE' },
			{ city: 'Leipzig', region: 'Saxony', country: 'DE' }
		]
	};

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="No cities yet" />

<Story name="Two cities" args={{ values }} />

<Story name="Phone, two cities" args={{ values }} globals={phone} />
