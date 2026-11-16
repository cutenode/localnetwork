<script lang="ts">
	import { ACCOUNT_PAGES } from '#lib/account.ts';
	import { auditSentence } from '#lib/audit-sentences.ts';
	import LinkRows from '#lib/components/LinkRows.svelte';
	import { accountLabel, pluralize, timeAgo } from '#lib/format.ts';
	import { resolved } from '#lib/resolved.svelte.ts';
	import type { ChannelView } from '#lib/types.ts';
	import Button from '#lib/ui/Button.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const channels = resolved(() => data.channels, undefined);

	const latest = resolved(() => data.latest, undefined);

	const requests = resolved(() => data.requests, undefined);

	const held = resolved(() => data.held, undefined);

	const onOrOff = (channel: ChannelView | null) => (channel ? (channel.status === 'active' ? 'on' : 'paused') : 'off');

	/** A row's line: nothing yet while its source is out, and what the page holds in general if that source failed. */
	const line = <T,>(value: T | null | undefined, about: string, say: (value: T) => string) => {
		if (value === undefined) return null;

		return value === null ? about : say(value);
	};

	const lines = $derived<(string | null)[]>([
		line(channels.value, ACCOUNT_PAGES[0].about, ([email, dm]) => `Email ${onOrOff(email)} · Bluesky DMs ${onOrOff(dm)}`),
		line(latest.value, ACCOUNT_PAGES[1].about, ([entry]) => (entry ? `${auditSentence(entry)} · ${timeAgo(entry.createdAt)}` : 'Nothing yet')),
		line(held.value, ACCOUNT_PAGES[2].about, (records) => (records.length ? `${pluralize(records.length, 'record')} held for you` : 'Nothing held for you')),
		line(requests.value, ACCOUNT_PAGES[3].about, (list) => {
			const backWithYou = list.filter(({ status }) => status === 'changes_requested').length;
			const withStaff = list.filter(({ status }) => status === 'pending').length;

			return [backWithYou && `${backWithYou} ${backWithYou === 1 ? 'needs' : 'need'} you`, withStaff && `${withStaff} with staff`].filter(Boolean).join(' · ') || 'Nothing open';
		})
	]);

	const rows = $derived(ACCOUNT_PAGES.map(({ href, label, icon }, index) => ({ href, label, icon, detail: lines[index] })));
</script>

<svelte:head><title>Account · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-8 sm:gap-10">
	<h1 class="m-0 text-heading font-semibold">Account</h1>
	<LinkRows {rows} />
	<form method="POST" action="/logout" class="flex items-center justify-between gap-3 px-4 sm:px-5">
		<p class="m-0 min-w-0 truncate text-small text-fg-muted">Signed in as {accountLabel(data.viewer)}</p>
		<Button type="submit" variant="secondary" size="sm" class="shrink-0">Sign out</Button>
	</form>
</div>
