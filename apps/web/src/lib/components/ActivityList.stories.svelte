<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { auditEntry } from '#lib/testing/account.ts';
	import ActivityList from './ActivityList.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/Activity list', component: ActivityList, tags: ['autodocs'] });

	const entries = [
		auditEntry(3, { action: 'booking.status', summary: 'Denied: only the other side can confirm this', outcome: 'denied' }),
		auditEntry(2, { action: 'follow.create', summary: 'Followed Saffron Social', rootStatus: 'unsupported' }),
		auditEntry(1, { action: 'made.up', summary: 'Something a newer version wrote', actorStatus: 'pending', rootStatus: 'written' })
	];

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Entries" args={{ entries }} />

<Story name="Loading" args={{ loading: true }} />

<Story name="Phone" args={{ entries }} globals={phone} />
