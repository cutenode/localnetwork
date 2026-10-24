<script lang="ts">
	import { ArrowRight, ExternalLink, FileText, Megaphone, Pencil } from '@lucide/svelte';
	import { page } from '$app/state';
	import Avatar from '#lib/components/Avatar.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import ShowList from '#lib/components/ShowList.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import Card from '#lib/ui/Card.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const project = $derived(data.project);

	const base = $derived(`/dashboard/projects/${project.slug}`);

	const visibility = { active: 'Public. Anyone can find it.', hidden: 'Not public yet. Only its managers and staff can see it.', archived: 'Archived. Only its managers and staff can see it.' } as const;

	const editable = $derived((show: { did: string; rkey: string }) => (show.did === data.viewer?.did ? `${base}/shows/${show.rkey}` : null));
</script>

<svelte:head><title>{project.profile.name} · Dashboard</title></svelte:head>

<div class="flex flex-col gap-8">
	<header class="flex flex-wrap items-center justify-between gap-x-6 gap-y-4">
		<div class="flex min-w-0 items-center gap-4">
			<Avatar src={project.avatarUrl} name={project.profile.name} size="lg" />
			<div class="flex min-w-0 flex-col gap-1.5">
				<div class="flex flex-wrap items-center gap-2.5">
					<h1 class="m-0 text-title font-semibold break-words">{project.profile.name}</h1>
					<StatusPill status={project.status} />
				</div>
				<p class="m-0 text-small text-fg-muted">{visibility[project.status]}</p>
			</div>
		</div>
		<a href="/projects/{project.slug}" class="btn-secondary btn-sm"><ExternalLink class="size-4" />View public page</a>
	</header>

	<div class="grid gap-4 md:grid-cols-2">
		<Card as="section" class="flex flex-col gap-3">
			<h2 class="m-0 text-emphasis font-semibold">Profile</h2>
			{#if data.pendingUpdate}
				<p class="m-0 text-small text-fg-muted">Your proposed changes are waiting for a moderator.</p>
				<a href="/dashboard/requests/{data.pendingUpdate.rkey}" class="btn-secondary btn-sm self-start">View request<ArrowRight class="size-4" /></a>
			{:else}
				<p class="m-0 text-small text-fg-muted">Changes go to a moderator before they're published.</p>
				<a href="{base}/profile" class="btn-secondary btn-sm self-start"><Pencil class="size-4" />Propose changes</a>
			{/if}
		</Card>
		<Card as="section" class="flex flex-col gap-3">
			<h2 class="m-0 text-emphasis font-semibold">Press kits</h2>
			<p class="m-0 text-small text-fg-muted">One for each kind of booking, and who can see the rest of each.</p>
			<a href="{base}/press-kits" class="btn-secondary btn-sm self-start"><FileText class="size-4" />Press kits</a>
		</Card>
	</div>

	<section id="shows" aria-labelledby="shows-title" class="flex scroll-mt-20 flex-col gap-4">
		<div class="flex flex-wrap items-center justify-between gap-2">
			<h2 id="shows-title" class="m-0 text-heading font-semibold">Upcoming shows</h2>
			{#if project.status === 'active'}<a href="{base}/shows/new" class="btn-secondary btn-sm"><Megaphone class="size-4" />Announce a show</a>{/if}
		</div>
		{#await Promise.all([data.shows, data.edited])}
			<ShowList shows={undefined} project={project.uri} empty={{ title: '' }} />
		{:then [shows, edited]}
			<ShowList {shows} project={project.uri} {editable} {edited} retryHref={page.url.pathname} empty={{ title: 'No upcoming shows', body: 'Announce one and it shows on your page and to your followers.' }} />
		{/await}
	</section>

	<div class="grid gap-4 md:grid-cols-2">
		<Card as="section" class="flex flex-col gap-3">
			<h2 class="m-0 text-emphasis font-semibold">Managers</h2>
			<ul class="m-0 flex list-none flex-col gap-1.5 p-0 text-small">{#each data.managers as manager (manager.did)}<li><Handle account={manager} /></li>{/each}</ul>
			<p class="m-0 text-caption text-fg-subtle">Staff add and remove managers. Someone can ask to join from the project's page.</p>
		</Card>
		<Card as="section" class="flex flex-col gap-3">
			<h2 class="m-0 text-emphasis font-semibold">How you play</h2>
			<p class="m-0 text-small text-fg-muted">Publish your playing stats and sets from <a class="link" href="/tools/locallibrary">locallibrary</a> and they appear on your page and in your press kits.</p>
		</Card>
	</div>
</div>
