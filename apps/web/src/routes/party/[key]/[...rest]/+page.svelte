<script lang="ts">
	import EventPage from '#lib/components/EventPage.svelte';
	import PartyPage from '#lib/components/PartyPage.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

{#if data.event}
	<EventPage
		event={data.event}
		party={data.party}
		link={data.link}
		rsvp={data.rsvp}
		timeZones={data.timeZones}
		subject={data.subject}
		can={data.can}
		role={data.role}
		editing={data.editing}
		status={data.status}
		version={data.version}
		{form}
	/>
{:else}
	<PartyPage
		party={data.party}
		events={data.events}
		newEventHref={data.newEventHref}
		people={data.people}
		geocoder={data.geocoder}
		subject={data.subject}
		can={data.can}
		role={data.role}
		editing={data.editing}
		status={data.status}
		version={data.version}
		{form}
	/>
{/if}
