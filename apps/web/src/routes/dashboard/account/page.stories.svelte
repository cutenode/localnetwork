<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ComponentProps } from 'svelte';
	import AccountNav from '#lib/components/AccountNav.svelte';
	import { accountLayout, auditEntry } from '#lib/testing/account.ts';
	import { request } from '#lib/testing/requests.ts';
	import type { ChannelView, HeldRecordView } from '#lib/types.ts';
	import Activity from './activity/+page.svelte';
	import Data from './data/+page.svelte';
	import Landing from './+page.svelte';
	import Requests from './requests/+page.svelte';

	/** The Account area: a landing list, then a page each for activity, what's held, and requests to staff. */
	const { Story } = defineMeta({
		title: 'Patterns/Account',
		parameters: { sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/dashboard/account') } } } }
	});

	const loading = new Promise<never>(() => {});

	const failed = Promise.reject(new Error('disk gone'));

	failed.catch(() => {});

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const hoursAgo = (hours: number) => new Date(Date.now() - hours * 3_600_000).toISOString();

	const entry = (seq: number, action: string, summary: string, createdAt: string, outcome = 'success') => auditEntry(seq, { action, summary, createdAt, outcome, rootStatus: 'written' });

	const entries = [
		entry(6, 'booking.status', 'Denied: only the other side can confirm this', hoursAgo(1), 'denied'),
		entry(5, 'follow.create', 'Followed Saffron Social', hoursAgo(5)),
		entry(4, 'show.save', 'Announced the show “Friday Social” for Neon Tide', hoursAgo(26)),
		entry(3, 'profile.save', 'Updated their profile', hoursAgo(80)),
		entry(2, 'auth.login', 'Signed in as @neon.test', hoursAgo(90)),
		entry(1, 'made.up', 'Something a newer version wrote', hoursAgo(400))
	];

	const requests = [
		request('claim', { status: 'changes_requested', decision: { at: hoursAgo(50), by: null, reason: 'One more link?' }, rkey: 'c' }),
		request('create', { updatedAt: hoursAgo(72), rkey: 'a', uri: 'at://a' }),
		request('list', { updatedAt: hoursAgo(24), rkey: 'b' }),
		request('create', { status: 'approved', decision: { at: hoursAgo(900), by: null, reason: null }, rkey: 'd', uri: 'at://d' }),
		request('create', { status: 'withdrawn', updatedAt: hoursAgo(9000), rkey: 'e', uri: 'at://e' })
	];

	const held = (kind: HeldRecordView['kind'], uri: string): HeldRecordView => ({ uri, kind, title: 'A record', excerpt: null, createdAt: hoursAgo(4) });

	const email: ChannelView = { status: 'active', pausedReason: null, address: null, topics: [] };

	const landing: ComponentProps<typeof Landing>['data'] = { ...accountLayout, channels: [email, null], latest: [entries[1]], requests, held: [held('booking', 'a'), held('booking', 'b')] };

	const activity = (value: ComponentProps<typeof Activity>['data']['activity']) => ({ ...accountLayout, activity: value });

	const data = (records: ComponentProps<typeof Data>['data']['records']) => ({ ...accountLayout, records });

	const list = (value: ComponentProps<typeof Requests>['data']['requests']) => ({ ...accountLayout, requests: value });
</script>

<Story name="Landing" asChild><Landing params={{}} form={null} data={landing} /></Story>
<Story name="Landing, phone" asChild globals={phone}><Landing params={{}} form={null} data={landing} /></Story>
<Story name="Landing, loading" asChild><Landing params={{}} form={null} data={{ ...landing, channels: loading, latest: loading, requests: loading, held: loading }} /></Story>
<Story name="Landing, a line failed" asChild><Landing params={{}} form={null} data={{ ...landing, channels: null, latest: null, requests: null, held: null }} /></Story>

<Story name="Sub-nav" asChild><AccountNav path="/dashboard/account/activity" /></Story>
<Story name="Sub-nav, phone" asChild globals={phone}><AccountNav path="/dashboard/account/data/booking" /></Story>

<Story name="Activity" asChild><Activity params={{}} form={null} data={activity({ entries, older: 1 })} /></Story>
<Story name="Activity, phone" asChild globals={phone}><Activity params={{}} form={null} data={activity({ entries, older: 1 })} /></Story>
<Story name="Activity, loading" asChild><Activity params={{}} form={null} data={activity(loading)} /></Story>
<Story name="Activity, phone loading" asChild globals={phone}><Activity params={{}} form={null} data={activity(loading)} /></Story>
<Story name="Activity, empty" asChild><Activity params={{}} form={null} data={activity({ entries: [], older: null })} /></Story>
<Story name="Activity, error" asChild><Activity params={{}} form={null} data={activity(failed)} /></Story>

<Story name="Your data" asChild><Data params={{}} form={null} data={data([held('booking', 'a'), held('booking', 'b'), held('request', 'c')])} /></Story>
<Story name="Your data, phone" asChild globals={phone}><Data params={{}} form={null} data={data([held('booking', 'a'), held('request', 'c')])} /></Story>
<Story name="Your data, nothing held" asChild><Data params={{}} form={null} data={data([])} /></Story>
<Story name="Your data, loading" asChild><Data params={{}} form={null} data={data(loading)} /></Story>
<Story name="Your data, error" asChild><Data params={{}} form={null} data={data(failed)} /></Story>

<Story name="Requests" asChild><Requests params={{}} form={null} data={list(requests)} /></Story>
<Story name="Requests, phone" asChild globals={phone}><Requests params={{}} form={null} data={list(requests)} /></Story>
<Story name="Requests, empty" asChild><Requests params={{}} form={null} data={list([])} /></Story>
<Story name="Requests, loading" asChild><Requests params={{}} form={null} data={list(loading)} /></Story>
<Story name="Requests, error" asChild><Requests params={{}} form={null} data={list(failed)} /></Story>
