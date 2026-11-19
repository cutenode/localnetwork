<script lang="ts">
	import type { ComponentProps } from 'svelte';
	import { ExternalLink, MapPin } from '@lucide/svelte';
	import { roleLabel } from '#lib/capabilities.ts';
	import type { EventSummary } from '#lib/events.ts';
	import { accountLabel, asShowDate, nightLabel, placeLabel, profileHref } from '#lib/format.ts';
	import { EVENT_STATUS, showHours } from '#lib/shows.ts';
	import { LINK_KINDS, labelFor } from '#lib/taxonomy/options.ts';
	import type { AccountView, BasicsSection } from '#lib/types.ts';
	import { webHref } from '#lib/web-url.ts';
	import Avatar from './Avatar.svelte';
	import PlacePicker from './PlacePicker.svelte';
	import AddSlot from './edit/AddSlot.svelte';
	import EditMode from './edit/EditMode.svelte';
	import InlineText from './edit/InlineText.svelte';
	import PictureField from './edit/PictureField.svelte';
	import LinkFields from './form/LinkFields.svelte';

	/**
	 * A party's page, built in place: its name, picture, bio, area, links and the people who run it, and in edit mode the
	 * same layout with each part as its field. A party being started has no `party` yet and draws every field empty.
	 * Its upcoming events are listed, its drafts, private ones and ones never confirmed marked for its people, and in edit
	 * mode those who can add one get a slot that goes to where it's made (`newEventHref`).
	 */
	let {
		party = null,
		events = [],
		newEventHref = null,
		people = [],
		geocoder = true,
		...edit
	}: Omit<ComponentProps<typeof EditMode>, 'children' | 'name' | 'labels' | 'missing'> & {
		party?: { basics: Omit<BasicsSection, 'avatar'>; avatarUrl: string | null } | null;
		events?: EventSummary[];
		newEventHref?: string | null;
		/** Its hosts and cohosts who show to everyone. */
		people?: (AccountView & { role: string })[];
		geocoder?: boolean;
	} = $props();

	const basics = $derived(party?.basics);

	const name = $derived(basics?.name ?? 'New party');

	const place = $derived(basics?.location && placeLabel({ location: basics.location }));

	const labels = { name: 'Name', bio: 'Bio', city: 'City', region: 'Region', country: 'Country' };
</script>

{#snippet eventList(shown: EventSummary[])}
	<ul class="m-0 flex list-none flex-col gap-2.5 p-0">
		{#each shown as item (item.href)}
			{@const isCancelled = item.status === EVENT_STATUS.cancelled}
			<li class="card flex items-start gap-4 p-4">
				<div class="flex min-w-0 flex-1 flex-col gap-1">
					<p class="m-0 flex flex-wrap items-baseline gap-x-2.5 gap-y-1">
						<a class={['entity-link text-emphasis font-semibold break-words', isCancelled && 'text-fg-muted line-through']} href={item.href}>{item.name}</a>
						{#if item.draft}<span class="pill bg-well text-fg-muted">Draft</span>{/if}
						{#if item.private}<span class="pill bg-well text-fg-muted">Private</span>{/if}
						{#if item.neverConfirmed}<span class="pill bg-well text-fg-muted">Never confirmed</span>{/if}
						{#if isCancelled}<span class="pill bg-raised text-critical">Cancelled</span>{:else if item.status === EVENT_STATUS.rescheduled}<span class="pill bg-raised text-fg-soft">New date</span>{/if}
					</p>
					{#if item.startsAt || item.venue}
						<p class="m-0 flex flex-wrap gap-x-1.5 text-small text-fg-muted">
							{#if item.startsAt}<span>{nightLabel(asShowDate(item.startsAt))}</span><span class="font-mono">{showHours({ startsAt: item.startsAt })}</span>{/if}
							{#if item.startsAt && item.venue}<span aria-hidden="true">·</span>{/if}
							{#if item.venue}<span>{item.venue}</span>{/if}
						</p>
					{/if}
				</div>
			</li>
		{/each}
	</ul>
{/snippet}

<EditMode {...edit} {name} {labels} missing={party ? [] : ['a name']}>
	{#snippet children(view)}
		{@const shownEvents = view.visitor ? events.filter((item) => !item.draft && !item.private && !item.neverConfirmed) : events}
		{#if edit.status === 'draft' && !view.visitor}<div class="container-page pt-6"><p class="m-0 notice notice-info">This is a draft. Only its people can see it.</p></div>{/if}
		<div class="border-b border-line">
			<div class="container-page flex flex-col gap-5 py-10 sm:flex-row sm:items-end sm:py-14">
				{#if view.editing}
					<PictureField src={party?.avatarUrl} error={view.error('avatar')} readonly={view.busy} />
				{:else}
					<Avatar src={party?.avatarUrl} {name} size="xl" />
				{/if}
				<div class="flex min-w-0 flex-1 flex-col gap-2">
					<h1 class="m-0 text-title font-semibold">
						{#if view.editing}<InlineText name="name" label="Name" required placeholder="Party name" value={view.value('name', basics?.name ?? '')} error={view.error('name')} readonly={view.busy} class="text-title font-semibold" />{:else}{basics?.name}{/if}
					</h1>
					{#if place && !view.editing}<p class="m-0 inline-flex items-center gap-1.5 text-small text-fg-muted"><MapPin class="size-4" />{place}</p>{/if}
				</div>
			</div>
		</div>

		<div class="container-page flex flex-wrap gap-x-10 gap-y-8 py-10">
			<div class="flex min-w-0 flex-[999_1_34rem] flex-col gap-8">
				{#if view.editing}
					<InlineText name="bio" label="Bio" multiline placeholder="What the night is about" value={view.value('bio', basics?.bio ?? '')} error={view.error('bio')} readonly={view.busy} class="text-body text-fg-soft" />
					{#if shownEvents.length || newEventHref}
						<section aria-labelledby="events" class="flex flex-col gap-3">
							<h2 id="events" class="m-0 eyebrow text-fg-subtle">Upcoming events</h2>
							{#if shownEvents.length}{@render eventList(shownEvents)}{/if}
							{#if newEventHref}<AddSlot label="Add an event" hint="Each event you add shows here" href={newEventHref} />{/if}
						</section>
					{/if}
					<section aria-labelledby="area" class="card flex flex-col gap-3 p-5">
						<h2 id="area" class="m-0 eyebrow text-fg-subtle">Area</h2>
						<PlacePicker values={basics?.location} {geocoder} required={false} missing="Pick a city from the search so people nearby find it." />
					</section>
					<section aria-labelledby="links" class="card flex flex-col gap-3 p-5">
						<h2 id="links" class="m-0 eyebrow text-fg-subtle">Links</h2>
						<LinkFields saved={basics?.links} blank="website" />
					</section>
				{:else}
					{#if basics?.bio}<p class="m-0 text-body whitespace-pre-line text-fg-soft">{basics.bio}</p>{/if}
					{#if shownEvents.length}
						<section aria-labelledby="events" class="flex flex-col gap-3">
							<h2 id="events" class="m-0 eyebrow text-fg-subtle">Upcoming events</h2>
							{@render eventList(shownEvents)}
						</section>
					{/if}
				{/if}
			</div>

			<aside class="flex min-w-0 flex-[1_1_18rem] flex-col gap-6">
				{#if people.length}
					<section aria-labelledby="people" class="card flex flex-col gap-3 p-5">
						<h2 id="people" class="m-0 eyebrow text-fg-subtle">Hosted by</h2>
						<ul class="m-0 flex list-none flex-col gap-2 p-0 text-small">
							{#each people as person (person.did)}
								<li class="flex items-center justify-between gap-3"><a class="link break-all" href={profileHref(person)}>{accountLabel(person)}</a><span class="shrink-0 text-fg-muted">{roleLabel(person.role)}</span></li>
							{/each}
						</ul>
					</section>
				{/if}
				{#if !view.editing && basics?.links?.length}
					<section aria-labelledby="links" class="card flex flex-col gap-3 p-5">
						<h2 id="links" class="m-0 eyebrow text-fg-subtle">Links</h2>
						<ul class="m-0 flex list-none flex-col gap-2 p-0 text-small">
							{#each basics.links as link, index (index)}
								<li><a class="link inline-flex items-center gap-1.5" href={webHref(link.url)} rel="noopener noreferrer nofollow" target="_blank">{link.label ?? labelFor(LINK_KINDS, link.kind)}<ExternalLink class="size-3 opacity-70" /></a></li>
							{/each}
						</ul>
					</section>
				{/if}
			</aside>
		</div>
	{/snippet}
</EditMode>
