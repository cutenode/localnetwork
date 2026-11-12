<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { inboxRows } from '#lib/testing/inbox.ts';
	import InboxList from './InboxList.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/Inbox', component: InboxList, tags: ['autodocs'] });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Unread and read" args={{ rows: inboxRows }} />

<Story name="All read" args={{ rows: inboxRows.map((row) => ({ ...row, unread: false })) }} />

<Story name="Loading" args={{ loading: 4 }} />

<Story name="Phone" args={{ rows: inboxRows }} globals={phone} />
