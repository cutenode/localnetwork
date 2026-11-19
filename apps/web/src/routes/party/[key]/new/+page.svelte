<script lang="ts">
	import EventPage from '#lib/components/EventPage.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<EventPage party={data.party} timeZone={data.timeZone} timeZones={data.timeZones} subject={data.subject} can={data.can} role={data.role} editing status="draft" version="" {form} />
