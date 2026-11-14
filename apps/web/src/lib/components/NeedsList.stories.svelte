<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { NeedsRow } from '#lib/home.ts';
	import NeedsList from './NeedsList.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/Needs list', component: NeedsList, tags: ['autodocs'] });

	const need = (key: string, scope: string, title: string, detail: string, action = 'Reply'): NeedsRow => ({ key, scope, title, detail, href: '/dashboard/bookings/3k1', action });

	const rows = [
		need('1', 'Neon Tide', 'Reply to Rooftop Rave', 'Sat 14 Nov · New inquiry from @pat.test · 2 hours ago'),
		need('2', 'Saffron Social', 'Reply to Walkthrough Night, a long show name that runs past the edge of a phone', 'No date yet · From @jonas.test · 3 hours ago'),
		need('3', 'Second Act', 'Update your request for Second Act', 'Staff asked for changes · yesterday', 'Update')
	];

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Waiting" args={{ rows }} />

<Story name="One thing" args={{ rows: rows.slice(0, 1) }} />

<!-- Under another section's primary action, so none of these leads. -->
<Story name="No lead" args={{ rows, lead: false }} />

<Story name="Loading" args={{ loading: 3 }} />

<Story name="Phone" args={{ rows }} globals={phone} />
