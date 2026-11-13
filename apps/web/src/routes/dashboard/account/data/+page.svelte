<script lang="ts">
	import EmptyState from '#lib/components/EmptyState.svelte';
	import { HELD_GROUPS } from '#lib/components/HeldRecordList.svelte';
	import LinkRows from '#lib/components/LinkRows.svelte';
	import type { HeldRecordView } from '#lib/types.ts';
	import Button from '#lib/ui/Button.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	/** A row per kind, so what's here reads the same whether it's none or many. Only kinds with records open. */
	const rowsFor = (records: HeldRecordView[] | null) =>
		HELD_GROUPS.map(({ kind, label, about }) => {
			const count = records && records.filter((record) => record.kind === kind).length;

			return { label, detail: about, count, href: count ? `/dashboard/account/data/${kind}` : undefined };
		});

	const CAN_DO = [
		['See', 'Every record Local Network holds for you, by type.'],
		['Delete', 'Any of them, one at a time or a whole type.'],
		['Keep', 'Anything you wrote with your own account stays on that account.'],
		["Can't delete", 'Activity, since each entry builds on the last, and booking status changes, which both sides rely on.']
	];
</script>

<svelte:head><title>Your data · Local Network</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-8 sm:gap-10">
	<header class="flex flex-col gap-1">
		<h1 class="m-0 text-heading font-semibold">Your data</h1>
		<p class="m-0 text-body text-fg-muted">What Local Network wrote for you when your account's server couldn't.</p>
	</header>
	<section aria-labelledby="held" class="flex flex-col gap-3">
		<h2 id="held" class="m-0 text-emphasis font-semibold">Held for you</h2>
		{#await data.records}
			<div role="status" aria-label="Loading your data"><LinkRows rows={rowsFor(null)} /></div>
		{:then records}
			<LinkRows rows={rowsFor(records)} />
		{:catch}
			<EmptyState title="Couldn't load your data" body="Something went wrong on our side. Try again in a minute.">
				<Button variant="secondary" size="sm" href="/dashboard/account/data" data-sveltekit-reload>Try again</Button>
			</EmptyState>
		{/await}
	</section>
	<section aria-labelledby="can-do" class="flex flex-col gap-3">
		<h2 id="can-do" class="m-0 text-emphasis font-semibold">What you can do</h2>
		<dl class="m-0 flex flex-col gap-3 text-small">
			{#each CAN_DO as [verb, what] (verb)}
				<div class="grid gap-x-6 sm:grid-cols-[8rem_minmax(0,1fr)]">
					<dt class="font-medium text-fg">{verb}</dt>
					<dd class="m-0 text-fg-muted">{what}</dd>
				</div>
			{/each}
		</dl>
	</section>
</div>
