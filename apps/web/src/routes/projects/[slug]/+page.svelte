<script lang="ts">
	import { enhance } from '$app/forms';
	import { Bookmark, BookmarkCheck, CalendarPlus, Clock, ExternalLink, FileText, Flag, Lock, MapPin, Users } from '@lucide/svelte';
	import { page } from '$app/state';
	import Avatar from '#lib/components/Avatar.svelte';
	import BookingDetails from '#lib/components/BookingDetails.svelte';
	import DjSets from '#lib/components/DjSets.svelte';
	import DjStats from '#lib/components/DjStats.svelte';
	import FollowButton from '#lib/components/FollowButton.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import Handle from '#lib/components/Handle.svelte';
	import ManageLink from '#lib/components/ManageLink.svelte';
	import MixList from '#lib/components/MixList.svelte';
	import PreviewBanner from '#lib/components/PreviewBanner.svelte';
	import ProjectFacts from '#lib/components/ProjectFacts.svelte';
	import ShowList from '#lib/components/ShowList.svelte';
	import SignInAgain from '#lib/components/SignInAgain.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDate, kindLabel, placeLabel } from '#lib/format.ts';
	import { LINK_KINDS, labelFor } from '#lib/taxonomy/options.ts';
	import Tag from '#lib/ui/Tag.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const project = $derived(data.project);

	const profile = $derived(project.profile);

	const viewer = $derived(data.viewer);

	const claimed = $derived(project.managers.length > 0);

	const loginHref = $derived(`/login?next=/projects/${project.slug}`);

	const followLogin = $derived(viewer ? undefined : loginHref);
</script>

<svelte:head>
	<title>{profile.name} · Local Network</title>
	<meta name="description" content={profile.tagline ?? `${kindLabel(profile.kind)} from ${placeLabel(profile)}`} />
</svelte:head>

<div class="border-b border-line">
	<div class="container-page flex flex-col gap-6 py-10 sm:py-14">
		<PreviewBanner status={project.status} />
		<div class="flex flex-col gap-5 sm:flex-row sm:items-end">
			<Avatar src={project.avatarUrl} name={profile.name} size="xl" />
			<div class="flex min-w-0 flex-1 flex-col gap-2">
				<div class="flex flex-wrap items-center gap-2.5">
					<h1 class="m-0 text-title font-semibold">{profile.name}</h1>
					{#if project.status !== 'active'}<StatusPill status={project.status} />{/if}
					{#if !claimed}<span class="pill bg-raised text-fg-muted">Unclaimed</span>{/if}
				</div>
				{#if profile.tagline}<p class="m-0 text-lede text-fg-soft sm:text-intro">{profile.tagline}</p>{/if}
				<p class="m-0 flex flex-wrap items-center gap-x-4 gap-y-1 text-small text-fg-muted">
					<span class="inline-flex items-center gap-1.5"><MapPin class="size-4" />{placeLabel(profile)}</span>
					<span class="inline-flex items-center gap-1.5"><Users class="size-4" />{kindLabel(profile.kind)}</span>
					{#if profile.pronouns}<span>{profile.pronouns}</span>{/if}
					{#if profile.activeSince}<span class="inline-flex items-center gap-1.5"><Clock class="size-4" />Playing since {profile.activeSince}</span>{/if}
				</p>
			</div>
		</div>
		{#if profile.genres.length}
			<ul class="m-0 flex list-none flex-wrap gap-1.5 p-0" aria-label="Sound">
				{#each profile.genres as genre, index (index)}<li><Tag href="/projects?genres={encodeURIComponent(genre)}">{genre}</Tag></li>{/each}
			</ul>
		{/if}
		<div class="flex flex-wrap gap-2">
			{#if !viewer}
				<a href={loginHref} class="btn-primary"><CalendarPlus class="size-4" />Sign in to book</a>
			{:else if claimed && !data.isManager}
				<a href="/dashboard/bookings/new?project={project.slug}" class="btn-primary"><CalendarPlus class="size-4" />Send inquiry</a>
			{/if}
			{#if data.follow}
				{#await data.follow}<FollowButton follow={undefined} loginHref={followLogin} />{:then follow}<FollowButton {follow} loginHref={followLogin} />{/await}
			{/if}
			{#if viewer && data.saved !== null}
				<form method="POST" action={data.saved ? '?/unsave' : '?/save'} use:enhance>
					<button class="btn-secondary">{#if data.saved}<BookmarkCheck class="size-4" />Saved{:else}<Bookmark class="size-4" />Save{/if}</button>
				</form>
			{/if}
			<a href="/projects/{project.slug}/epk" class="btn-secondary"><FileText class="size-4" />Press kit</a>
			<ManageLink {viewer} {project} />
			{#if viewer && !data.isManager && !data.pendingClaim && project.status !== 'archived'}
				<a href="/dashboard/claim/{project.slug}" class="btn-ghost"><Flag class="size-4" />{claimed ? 'Request to co-manage' : 'Claim this project'}</a>
			{/if}
		</div>
	</div>
</div>

{#if form || data.pendingClaim || data.pendingUpdate}
	<div class="container-page mt-6 flex flex-col gap-3">
		{#if form?.reauth && form.error && viewer}<SignInAgain did={viewer.did} next={page.url.pathname} error={form.error} />{:else}<FormMessage {form} />{/if}
		{#if data.pendingClaim}<p class="notice m-0">Your claim is waiting for a moderator. <a class="link" href="/dashboard/requests/{data.pendingClaim.rkey}">View request</a></p>{/if}
		{#if data.pendingUpdate}<p class="notice m-0">Your proposed changes are waiting for review. <a class="link" href="/dashboard/requests/{data.pendingUpdate.rkey}">View request</a></p>{/if}
	</div>
{/if}

<div class="container-page flex flex-wrap gap-x-10 gap-y-8 py-10">
	<div class="flex min-w-0 flex-[999_1_34rem] flex-col gap-10">
		{#if profile.bio || profile.lookingFor}
			<section aria-labelledby="about" class="flex flex-col gap-3">
				<h2 id="about" class="m-0 text-heading font-semibold">About</h2>
				{#if profile.bio}<p class="m-0 max-w-[65ch] whitespace-pre-line text-fg-soft">{profile.bio}</p>{/if}
				{#if profile.lookingFor}
					<div class="flex flex-col gap-1">
						<h3 class="m-0 eyebrow text-fg-subtle">Looking for</h3>
						<p class="m-0 max-w-[65ch] whitespace-pre-line text-small text-fg-muted">{profile.lookingFor}</p>
					</div>
				{/if}
			</section>
		{/if}

		<section id="shows" aria-labelledby="shows-title" class="flex scroll-mt-20 flex-col gap-4">
			<h2 id="shows-title" class="m-0 text-heading font-semibold">Upcoming shows</h2>
			{#await data.shows}
				<ShowList shows={undefined} project={project.uri} empty={{ title: '' }} />
			{:then shows}
				<ShowList {shows} project={project.uri} retryHref={page.url.pathname} empty={{ title: 'No upcoming shows', body: `Follow ${profile.name} to see new ones under Shows.` }} />
			{/await}
		</section>

		{#if profile.mixes?.length}
			<section aria-labelledby="mixes" class="flex flex-col gap-4">
				<h2 id="mixes" class="m-0 text-heading font-semibold">Mixes</h2>
				<MixList mixes={profile.mixes} sets={data.sets} tracklistHref={(index) => `/projects/${project.slug}/mixes/${index + 1}`} />
			</section>
		{/if}

		{#await data.dj}
			<div aria-busy="true" aria-label="Loading how they play" class="flex flex-col gap-4">
				<div class="h-7 w-48 animate-pulse rounded-md bg-raised"></div>
				<div class="h-28 animate-pulse rounded-xl bg-surface"></div>
			</div>
		{:then accounts}
			{#each accounts as account (account.did)}
				{#if account.profile}
					<section aria-labelledby="stats-{account.did}" class="flex flex-col gap-4">
						<div class="flex flex-wrap items-baseline justify-between gap-2">
							<h2 id="stats-{account.did}" class="m-0 text-heading font-semibold">How they play</h2>
							{#if accounts.length > 1 && account.handle}<span class="text-small text-fg-muted">@{account.handle}</span>{/if}
						</div>
						<DjStats profile={account.profile} />
					</section>
				{/if}
				{#if account.sets.length}
					<section aria-labelledby="sets-{account.did}" class="flex flex-col gap-4">
						<h2 id="sets-{account.did}" class="m-0 text-heading font-semibold">Recent sets</h2>
						<DjSets sets={account.sets} />
					</section>
				{/if}
			{/each}
		{/await}

		{#if profile.highlights?.length}
			<section aria-labelledby="highlights" class="flex flex-col gap-3">
				<h2 id="highlights" class="m-0 text-heading font-semibold">Highlights</h2>
				<ul class="m-0 flex list-none flex-col gap-2 p-0">
					{#each profile.highlights as highlight, index (index)}<li class="border-l-2 border-line-strong pl-3 text-fg-soft">{highlight}</li>{/each}
				</ul>
			</section>
		{/if}

		<section aria-labelledby="booking" class="card flex flex-col gap-3 p-5">
			<h2 id="booking" class="m-0 inline-flex items-center gap-2 text-emphasis font-semibold"><Lock class="size-4" />Booking details</h2>
			<BookingDetails details={data.details} signedIn={!!viewer} {claimed} {loginHref} />
		</section>
	</div>

	<aside class="flex min-w-0 flex-[1_1_18rem] flex-col gap-6">
		<section aria-labelledby="glance" class="card flex flex-col gap-4 p-5">
			<h2 id="glance" class="m-0 eyebrow text-fg-subtle">At a glance</h2>
			<ProjectFacts {profile} />
		</section>
		{#if profile.links?.length}
			<section aria-labelledby="links" class="card flex flex-col gap-3 p-5">
				<h2 id="links" class="m-0 eyebrow text-fg-subtle">Links</h2>
				<ul class="m-0 flex list-none flex-col gap-2 p-0 text-small">
					{#each profile.links as link, index (index)}
						<li><a class="link inline-flex items-center gap-1.5" href={link.url} rel="noopener noreferrer nofollow" target="_blank">{link.label ?? labelFor(LINK_KINDS, link.kind)}<ExternalLink class="size-3 opacity-70" /></a></li>
					{/each}
				</ul>
			</section>
		{/if}
		<section aria-labelledby="managers" class="card flex flex-col gap-3 p-5 text-small">
			<h2 id="managers" class="m-0 eyebrow text-fg-subtle">Managed by</h2>
			{#if data.managers.length}
				<ul class="m-0 flex list-none flex-col gap-1.5 p-0">{#each data.managers as manager (manager.did)}<li><Handle account={manager} /></li>{/each}</ul>
			{:else}
				<p class="m-0 text-fg-muted">Nobody yet. Listed by Local Network staff.{#if viewer && project.status !== 'archived'}{' '}<a class="link" href="/dashboard/claim/{project.slug}">Is this you?</a>{/if}</p>
			{/if}
		</section>
		<section aria-labelledby="record" class="flex flex-col gap-1.5 px-1 text-caption text-fg-subtle">
			<h2 id="record" class="m-0 eyebrow">On the AT Protocol</h2>
			<p class="m-0">Revision {project.revision}, updated {formatDate(project.updatedAt)}</p>
			<p class="m-0 font-mono break-all">{project.uri}</p>
		</section>
	</aside>
</div>
