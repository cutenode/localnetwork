<script lang="ts">
	import { ChevronDown } from '@lucide/svelte';
	import BookingRows from '#lib/components/BookingRows.svelte';
	import EmptyState from '#lib/components/EmptyState.svelte';
	import { bookingsByTurn } from '#lib/bookings.ts';
	import { resolved } from '#lib/resolved.svelte.ts';
	import Button from '#lib/ui/Button.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const viewer = $derived(data.viewer?.did ?? '');

	// Undefined until the directory answers, and null when it couldn't.
	const names = resolved(() => data.names, undefined);

	const actsPending = $derived(names.value === undefined);

	const threads = $derived(data.threads.map((thread) => ({ ...thread, project: names.value?.get(thread.space) ?? null })));

	// The acts the threads are with, for the filter; it shows once there's more than one.
	const acts = $derived([...new Map(threads.flatMap((thread) => (thread.project ? [[thread.project.slug, thread.project.name] as const] : []))).entries()]);

	// The filter goes by what only the directory knows, so without it every booking shows.
	const groups = $derived(
		bookingsByTurn(
			threads.filter((thread) => !data.filter || !names.value || thread.project?.slug === data.filter),
			viewer
		)
	);

	const managed = resolved(() => data.projects, undefined);

	const chip = 'chip min-h-9 px-3 transition-colors hover:border-fg-subtle aria-[current=page]:border-fg aria-[current=page]:bg-fg aria-[current=page]:font-semibold aria-[current=page]:text-fg-inverse';

	const ghost = 'rounded-md bg-well motion-safe:animate-pulse';
</script>

<svelte:head><title>Bookings · Local Network</title></svelte:head>

{#snippet heading(value: string, label: string, count: number)}
	<h2 id="group-{value}" class="m-0 flex items-baseline gap-2 text-emphasis font-semibold">{label}<span class="font-mono text-caption font-normal text-fg-subtle">{count}</span></h2>
{/snippet}

<div class="flex max-w-3xl flex-col gap-8 sm:gap-10">
	<header class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
		<h1 class="m-0 text-heading font-semibold">Bookings</h1>
		{#if data.threads.length}<a class="btn-secondary btn-sm" href="/projects">New inquiry</a>{/if}
	</header>

	{#if data.threads.length === 0}
		{#if managed.value === undefined}
			<EmptyState title="No bookings yet">
				<div role="status" aria-label="Loading" class="flex flex-col items-center gap-3"><span class={['h-4 w-64 max-w-full', ghost]}></span><span class={['h-11 w-28', ghost]}></span></div>
			</EmptyState>
		{:else if managed.value?.[0]}
			<EmptyState title="No bookings yet" body="When a promoter sends an inquiry, it starts here. A full page helps them find you.">
				<Button variant="secondary" href="/dashboard/projects/{managed.value[0].slug}">Edit your page</Button>
			</EmptyState>
		{:else}
			<EmptyState title="No bookings yet" body="Find a DJ and send an inquiry with your show details.">
				<Button href="/projects">Find DJs</Button>
			</EmptyState>
		{/if}
	{:else}
		<div class="flex flex-col gap-6 sm:gap-8">
			{#if actsPending && data.acts > 1}
				<div role="status" aria-label="Loading acts" class="flex flex-wrap gap-2">
					<span class={['h-9 w-12 rounded-full', ghost]}></span>
					{#each { length: data.acts }, act (act)}<span class={['h-9 w-28 rounded-full', ghost]}></span>{/each}
				</div>
			{:else if acts.length > 1}
				<nav aria-label="Filter by act" class="flex flex-wrap gap-2">
					<a href="/dashboard/bookings" aria-current={data.filter ? undefined : 'page'} class={chip}>All</a>
					{#each acts as [slug, name] (slug)}<a href="/dashboard/bookings?project={slug}" aria-current={data.filter === slug ? 'page' : undefined} class={chip}>{name}</a>{/each}
				</nav>
			{/if}

			{#if names.value === null}
				<p class="notice notice-info m-0 flex flex-wrap items-center justify-between gap-x-4 gap-y-2">Act names didn't load{data.filter ? ', so this shows every booking' : ''}.<a class="btn-secondary btn-sm" href={data.filter ? `/dashboard/bookings?project=${data.filter}` : '/dashboard/bookings'} data-sveltekit-reload>Try again</a></p>
			{/if}

			{#if data.filter && actsPending}
				<div role="status" aria-label="Loading bookings"><BookingRows loading={Math.min(data.threads.length, 5)} /></div>
			{:else}
				{#each groups as { value, label, threads } (value)}
					<section aria-labelledby="group-{value}" class="flex flex-col gap-3">
						{#if value === 'closed'}
							<details class="group/closed">
								<summary class="flex min-h-11 cursor-pointer list-none items-center gap-1.5 [&::-webkit-details-marker]:hidden">
									{@render heading(value, label, threads.length)}
									<ChevronDown class="size-4 text-fg-subtle group-open/closed:rotate-180 motion-safe:transition-transform" aria-hidden="true" />
								</summary>
								<div class="mt-1"><BookingRows {threads} group={value} {viewer} {actsPending} /></div>
							</details>
						{:else}
							{@render heading(value, label, threads.length)}
							<BookingRows {threads} group={value} {viewer} {actsPending} />
						{/if}
					</section>
				{:else}
					<EmptyState title="No bookings with this act" body="Pick another act, or show them all.">
						<Button variant="secondary" href="/dashboard/bookings">Show all</Button>
					</EmptyState>
				{/each}
			{/if}
		</div>
	{/if}
</div>
