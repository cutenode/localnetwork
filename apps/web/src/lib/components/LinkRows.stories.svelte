<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { ACCOUNT_PAGES } from '#lib/account.ts';
	import LinkRows from './LinkRows.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/Link rows', component: LinkRows, tags: ['autodocs'] });

	const details = ['Email on · Bluesky DMs off', 'You followed Saffron Social · 5 hours ago', '2 records held for you', 'Nothing open'];

	const pages = ACCOUNT_PAGES.map(({ href, label, icon }, index) => ({ href, label, icon, detail: details[index] }));

	// A row with nothing in it stays plain, so only the counted ones open.
	const counted = [
		{ label: 'Requests to staff', detail: "What you've asked staff for", count: 1, href: '/dashboard/account/data/request' },
		{ label: 'Booking messages', detail: 'Inquiries and messages', count: 2, href: '/dashboard/account/data/booking' },
		{ label: 'Press kit access', detail: 'Asks to see a press kit', count: 0 }
	];

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Pages" args={{ rows: pages }} />

<Story name="Pages, loading" args={{ rows: pages.map((row) => ({ ...row, detail: null })) }} />

<Story name="Counts" args={{ rows: counted }} />

<Story name="Counts, loading" args={{ rows: counted.map(({ label, detail }) => ({ label, detail, count: null })) }} />

<Story name="Phone" args={{ rows: pages }} globals={phone} />
