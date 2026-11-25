<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { inboxRows } from '#lib/testing/inbox.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import InboxList from './InboxList.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/Inbox list', component: InboxList, tags: ['autodocs'] });
</script>

<Story name="Unread and read" args={{ rows: inboxRows }} />

<Story name="All read" args={{ rows: inboxRows.map((row) => ({ ...row, unread: false })) }} />

<Story name="Loading" args={{ loading: 4 }} />

<Story name="Phone" args={{ rows: inboxRows }} globals={phone} />
