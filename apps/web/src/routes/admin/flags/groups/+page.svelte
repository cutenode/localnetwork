<script lang="ts">
	import { enhance } from '$app/forms';
	import FlagGroupTable from '#lib/components/FlagGroupTable.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<svelte:head><title>Flag groups · Local Network</title></svelte:head>

<div class="flex flex-col gap-6">
	<SectionHeader level={1} title="Flag groups" lede="Named lists of accounts that flags can turn on for." />
	<FormMessage {form} />
	<div class="grid grid-cols-[minmax(0,1fr)] items-start gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
		<FlagGroupTable groups={data.groups} />
		<form method="POST" action="?/create" class="card flex flex-col gap-4 p-5" use:enhance>
			<h2 class="text-emphasis font-semibold">New group</h2>
			<div><label class="label" for="name">Name</label><input id="name" name="name" class="input" required maxlength="64" /></div>
			<div><label class="label" for="description">Description</label><input id="description" name="description" class="input" maxlength="300" /></div>
			<div>
				<label class="label" for="members">Accounts</label>
				<textarea id="members" name="members" class="input min-h-28 py-2.5 font-mono text-small" aria-describedby="members-hint" placeholder="alice.bsky.social"></textarea>
				<p id="members-hint" class="hint">One handle or DID per line.</p>
			</div>
			<button class="btn-primary w-full">Create group</button>
		</form>
	</div>
</div>
