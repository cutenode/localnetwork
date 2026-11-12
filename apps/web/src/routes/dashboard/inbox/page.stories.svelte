<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { accountLayout } from '#lib/testing/account.ts';
	import { inboxRows } from '#lib/testing/inbox.ts';
	import type { PageProps } from './$types';
	import Page from './+page.svelte';

	/** The inbox: a row per booking or other subject, saying what happened since it was last read. */
	const { Story } = defineMeta({ title: 'Patterns/Inbox' });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const loading = new Promise<never>(() => {});

	const failed = Promise.reject(new Error('disk gone'));

	failed.catch(() => {});

	const data = (changes: Partial<PageProps['data']> = {}): PageProps['data'] => ({
		...accountLayout,
		unread: 3,
		inbox: { rows: inboxRows, older: undefined },
		paused: [],
		alsoSent: ['email'],
		...changes
	});

	const quiet = data({ unread: 0, inbox: { rows: [], older: undefined }, alsoSent: [] });
</script>

<Story name="Unread and read" asChild>
	<Page params={{}} form={null} data={data()} />
</Story>

<Story name="Unread and read, on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={data()} />
</Story>

<Story name="Loading" asChild>
	<Page params={{}} form={null} data={data({ inbox: loading })} />
</Story>

<Story name="Loading, on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={data({ inbox: loading })} />
</Story>

<Story name="Nothing new" asChild>
	<Page params={{}} form={null} data={quiet} />
</Story>

<Story name="Nothing new, on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={quiet} />
</Story>

<Story name="Didn't load" asChild>
	<Page params={{}} form={null} data={data({ inbox: failed })} />
</Story>

<Story name="Didn't load, on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={data({ inbox: failed })} />
</Story>

<Story name="A channel paused" asChild>
	<Page params={{}} form={null} data={data({ paused: ['dm_not_allowed'], alsoSent: ['email'] })} />
</Story>

<Story name="Both channels paused, on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={data({ paused: ['email_unverified', 'dm_not_allowed'], alsoSent: [] })} />
</Story>

<Story name="Older to page to" asChild>
	<Page params={{}} form={null} data={data({ unread: 0, inbox: { rows: inboxRows.map((row) => ({ ...row, unread: false })), older: 'd' }, alsoSent: ['email', 'dm'] })} />
</Story>
