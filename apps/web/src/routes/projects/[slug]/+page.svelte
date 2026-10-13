<script lang="ts">
	import { enhance } from '$app/forms';
	import { Bookmark, BookmarkCheck, CalendarPlus, Clock, ExternalLink, Flag, Globe, Lock, MapPin, Pencil, Ruler, Users } from '@lucide/svelte';
	import Avatar from '#lib/components/Avatar.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import MixList from '#lib/components/MixList.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { feeLabel, formatDate, kindLabel, placeLabel } from '#lib/format.ts';
	import { FORMATS, LINK_KINDS, labelFor, SET_TYPES } from '#lib/taxonomy/options.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const project = $derived(data.project);

	const profile = $derived(project.profile);

	const viewer = $derived(data.viewer);

	const claimed = $derived(project.managers.length > 0);
</script>

<svelte:head>
	<title>{profile.name} · Local Network</title>
	<meta name="description" content={profile.tagline ?? `${kindLabel(profile.kind)} from ${placeLabel(profile)}`} />
</svelte:head>

<div>
	<div class="container-page pt-6">
		<div class="ansi-stripes h-3" aria-hidden="true"></div>
		<div class="card card-pad flex flex-col gap-6 sm:flex-row sm:items-end">
			<Avatar src={project.avatarUrl} name={profile.name} size="xl" />
			<div class="min-w-0 flex-1 space-y-2">
				<div class="flex flex-wrap items-center gap-2">
					<h1 class="text-3xl sm:text-4xl">{profile.name}</h1>
					{#if project.status !== 'active'}<StatusPill status={project.status} />{/if}
					{#if !claimed}<span class="chip">Unclaimed</span>{/if}
				</div>
				{#if profile.tagline}<p class="text-lg text-ansi-white">{profile.tagline}</p>{/if}
				<p class="muted flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
					<span class="inline-flex items-center gap-1"><MapPin class="size-4" />{placeLabel(profile)}</span>
					<span class="inline-flex items-center gap-1"><Users class="size-4" />{kindLabel(profile.kind)}</span>
					{#if profile.pronouns}<span>{profile.pronouns}</span>{/if}
					{#if profile.activeSince}<span class="inline-flex items-center gap-1"><Clock class="size-4" />Playing since {profile.activeSince}</span>{/if}
				</p>
			</div>
			<div class="flex flex-wrap gap-2 sm:justify-end">
				{#if viewer}
					{#if claimed && !data.isManager}
						<a href="/bookings/new?project={project.slug}" class="btn-accent"><CalendarPlus class="size-4" />Send inquiry</a>
					{/if}
					{#if data.saved !== null}
						<form method="POST" action={data.saved ? '?/unsave' : '?/save'} use:enhance>
							<button class="btn-secondary">{#if data.saved}<BookmarkCheck class="size-4" />Saved{:else}<Bookmark class="size-4" />Save{/if}</button>
						</form>
					{/if}
					{#if data.isManager}
						<a href="/projects/{project.slug}/edit" class="btn-secondary"><Pencil class="size-4" />Propose changes</a>
					{:else if !data.pendingClaim && project.status !== 'archived'}
						<a href="/projects/{project.slug}/claim" class="btn-ghost"><Flag class="size-4" />{claimed ? 'Request to co-manage' : 'Claim this project'}</a>
					{/if}
					{#if viewer.isAdmin}<a href="/admin/projects/{project.slug}" class="btn-ghost">Admin</a>{/if}
				{:else}
					<a href="/login?next=/projects/{project.slug}" class="btn-accent">Sign in to book</a>
				{/if}
			</div>
		</div>
	</div>
</div>

<div class="container-page mt-6 space-y-4">
	<FormMessage {form} />
	{#if data.pendingClaim}
		<div class="notice">
			Your claim is waiting for a moderator. <a class="link" href="/me/requests/{data.pendingClaim.rkey}">View request</a>
		</div>
	{/if}
	{#if data.pendingUpdate}
		<div class="notice">
			Your proposed changes are waiting for review. <a class="link" href="/me/requests/{data.pendingUpdate.rkey}">View request</a>
		</div>
	{/if}
</div>

<div class="container-page mt-6 grid gap-6 lg:grid-cols-[1fr_20rem]">
	<div class="space-y-6">
		{#if profile.bio}
			<section class="card card-pad">
				<h2 class="section-title mb-3">About</h2>
				<p class="whitespace-pre-line">{profile.bio}</p>
			</section>
		{/if}
		<section class="card card-pad space-y-4">
			<h2 class="section-title">Sound</h2>
			<div class="flex flex-wrap gap-2">
				{#each profile.genres as genre, index (index)}<a class="chip-accent" href="/projects?genres={encodeURIComponent(genre)}">{genre}</a>{/each}
			</div>
			{#if profile.lookingFor}
				<div>
					<h3 class="text-sm">Looking for</h3>
					<p class="muted mt-1 text-sm whitespace-pre-line">{profile.lookingFor}</p>
				</div>
			{/if}
		</section>
		{#if profile.mixes?.length}
			<section class="card card-pad">
				<h2 class="section-title mb-4">Mixes</h2>
				<MixList mixes={profile.mixes} sets={data.sets} tracklistHref={(index) => `/projects/${project.slug}/mixes/${index + 1}`} />
			</section>
		{/if}
		{#if profile.highlights?.length}
			<section class="card card-pad">
				<h2 class="section-title mb-3">Highlights</h2>
				<ul class="list-inside list-disc space-y-1 text-sm">
					{#each profile.highlights as highlight, index (index)}<li>{highlight}</li>{/each}
				</ul>
			</section>
		{/if}
		<section class="card card-pad">
			<h2 class="section-title mb-3 flex items-center gap-2"><Lock class="size-4" />Booking details</h2>
			{#if !viewer}
				<p class="muted text-sm"><a class="link" href="/login?next=/projects/{project.slug}">Sign in</a> to see booking contacts, rates, and the tech rider.</p>
			{:else if data.details && Object.keys(data.details).length}
				<dl class="grid gap-4 text-sm sm:grid-cols-2">
					{#if data.details.bookingEmail}<div><dt class="muted">Booking email</dt><dd><a class="link" href="mailto:{data.details.bookingEmail}">{data.details.bookingEmail}</a></dd></div>{/if}
					{#if data.details.bookingPhone}<div><dt class="muted">Phone</dt><dd>{data.details.bookingPhone}</dd></div>{/if}
					{#if data.details.rateNotes}<div class="sm:col-span-2"><dt class="muted">Rates</dt><dd class="whitespace-pre-line">{data.details.rateNotes}</dd></div>{/if}
					{#if data.details.techRider}<div class="sm:col-span-2"><dt class="muted">Tech rider</dt><dd class="whitespace-pre-line">{data.details.techRider}</dd></div>{/if}
					{#if data.details.hospitality}<div class="sm:col-span-2"><dt class="muted">Hospitality</dt><dd class="whitespace-pre-line">{data.details.hospitality}</dd></div>{/if}
				</dl>
			{:else}
				<p class="muted text-sm">No private booking details yet.{claimed ? ' Send an inquiry to reach the managers directly.' : ''}</p>
			{/if}
		</section>
	</div>

	<aside class="space-y-6">
		<section class="card card-pad space-y-4 text-sm">
			<h2 class="section-title">At a glance</h2>
			{#if profile.fee}<div><p class="muted">Fees</p><p class="text-ansi-white">{feeLabel(profile)}</p></div>{/if}
			{#if profile.setTypes?.length}<div><p class="muted">Slots</p><p class="text-ansi-white">{profile.setTypes.map((value) => labelFor(SET_TYPES, value)).join(', ')}</p></div>{/if}
			{#if profile.formats?.length}<div><p class="muted">Plays on</p><p class="text-ansi-white">{profile.formats.map((value) => labelFor(FORMATS, value)).join(', ')}</p></div>{/if}
			{#if profile.setLength}<div><p class="muted">Set length</p><p class="text-ansi-white">{profile.setLength.minMinutes ?? '?'}–{profile.setLength.maxMinutes ?? '?'} minutes</p></div>{/if}
			{#if profile.capacity}<div><p class="muted inline-flex items-center gap-1"><Ruler class="size-3" />Room sizes</p><p class="text-ansi-white">{profile.capacity.min ?? 0}–{profile.capacity.max ?? '∞'} people</p></div>{/if}
			{#if profile.travel}
				<div>
					<p class="muted">Travel</p>
					<p class="text-ansi-white">{profile.travel.radiusKm ? `Up to ${profile.travel.radiusKm} km` : 'Local only'}{profile.travel.international ? ' · international' : ''}</p>
					{#if profile.travel.notes}<p class="muted">{profile.travel.notes}</p>{/if}
				</div>
			{/if}
		</section>
		{#if profile.links?.length}
			<section class="card card-pad">
				<h2 class="section-title mb-3">Links</h2>
				<ul class="space-y-2 text-sm">
					{#each profile.links as link, index (index)}
						<li><a class="link inline-flex items-center gap-1.5" href={link.url} rel="noopener noreferrer nofollow" target="_blank"><Globe class="size-3.5" />{link.label ?? labelFor(LINK_KINDS, link.kind)}<ExternalLink class="size-3 opacity-60" /></a></li>
					{/each}
				</ul>
			</section>
		{/if}
		<section class="card card-pad space-y-3 text-sm">
			<h2 class="section-title">Managed by</h2>
			{#if data.managers.length}
				<ul class="space-y-1">{#each data.managers as manager (manager.did)}<li><Handle account={manager} /></li>{/each}</ul>
			{:else}
				<p class="muted">Nobody yet. Listed by Local Network staff.</p>
			{/if}
		</section>
		<section class="card card-pad space-y-2 text-xs">
			<h2 class="text-sm">On the AT Protocol</h2>
			<p class="muted">Revision {project.revision} · updated {formatDate(project.updatedAt)}</p>
			<p class="mono muted">{project.uri}</p>
		</section>
	</aside>
</div>
