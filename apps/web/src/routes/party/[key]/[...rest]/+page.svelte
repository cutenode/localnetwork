<script lang="ts">
	import PartyPage from '#lib/components/PartyPage.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<PartyPage
	party={data.party}
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
