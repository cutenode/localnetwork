<script lang="ts">
	import { ExternalLink, KeyRound, Lock, Pencil, Plus } from '@lucide/svelte';
	import { presetFor } from '#lib/epk.ts';
	import { formatDate } from '#lib/format.ts';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const project = $derived(data.project);
</script>

<svelte:head><title>Press kits · {project.profile.name}</title></svelte:head>

<div class="flex max-w-208 flex-col gap-6">
	{#snippet add()}<a class="btn-primary btn-sm" href="/dashboard/projects/{project.slug}/press-kits/new"><Plus class="size-4" />New press kit</a>{/snippet}
	<SectionHeader level={1} eyebrow={project.profile.name} title="Press kits" lede="One for each kind of booking. Each is saved to the PDS of the manager who made it, and only they can change it." actions={add} />
	{#if data.deleted}<p role="status" class="notice notice-ok m-0">Press kit deleted.</p>{/if}
	{#await data.epks}
		<div aria-busy="true" aria-label="Loading press kits" class="h-24 animate-pulse rounded-xl bg-surface"></div>
	{:then epks}
		{#if epks.length}
			<ul class="m-0 flex list-none flex-col gap-3 p-0">
				{#each epks as epk (epk.uri)}
					{@const preset = presetFor(epk.record.access)}
					{@const isOwner = epk.did === data.viewer?.did}
					<li class="card flex flex-wrap items-center justify-between gap-x-6 gap-y-3 p-4 sm:px-5">
						<div class="flex min-w-0 flex-col gap-1">
							<p class="m-0 text-emphasis font-semibold">{epk.record.title}</p>
							{#if epk.record.headline}<p class="m-0 text-small text-fg-soft">{epk.record.headline}</p>{/if}
							<p class="m-0 inline-flex flex-wrap items-center gap-1.5 text-caption text-fg-subtle"><Lock class="size-3" aria-hidden="true" />{preset?.label ?? 'Custom access'} · Updated {formatDate(epk.record.updatedAt ?? epk.record.createdAt)}{#if !isOwner}{' '}· By {epk.owner.handle ? `@${epk.owner.handle}` : epk.did}{/if}</p>
						</div>
						<div class="flex flex-wrap gap-2">
							{#if isOwner}
								<a class="btn-secondary btn-sm" href="/dashboard/projects/{project.slug}/press-kits/{epk.did}/{epk.rkey}" aria-label="Edit {epk.record.title}"><Pencil class="size-4" />Edit</a>
								<a class="btn-secondary btn-sm" href="/dashboard/projects/{project.slug}/press-kits/{epk.did}/{epk.rkey}/access" aria-label="Access to {epk.record.title}"><KeyRound class="size-4" />Access</a>
							{/if}
							<a class="btn-ghost btn-sm" href="/epk/{epk.did}/{epk.rkey}" aria-label="View {epk.record.title}"><ExternalLink class="size-4" />View</a>
						</div>
					</li>
				{/each}
			</ul>
		{:else}
			<p class="card m-0 p-5 text-small text-fg-muted">None yet. Make one to choose which mixes and photos everyone sees, and who gets the rest.</p>
		{/if}
	{/await}
</div>
