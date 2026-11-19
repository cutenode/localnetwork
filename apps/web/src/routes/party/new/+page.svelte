<script lang="ts">
	import PartyPage from '#lib/components/PartyPage.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<PartyPage geocoder={data.geocoder} subject={data.subject} can={data.can} role={data.role} editing status="draft" version="" {form} />
