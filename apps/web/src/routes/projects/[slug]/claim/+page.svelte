<script lang="ts">
	import ClaimFields from '#lib/components/ClaimFields.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const values = $derived(form?.values ?? {});

	const claimed = $derived(data.project.managers.length > 0);
</script>

<svelte:head><title>Claim {data.project.profile.name} · Local Network</title></svelte:head>

<div class="container-page max-w-2xl py-10">
	<p class="eyebrow">{claimed ? 'Co-manage' : 'Claim'}</p>
	<h1 class="mt-1 text-4xl">{data.project.profile.name}</h1>
	<p class="muted mt-2">
		{#if claimed}This project already has managers. Ask to join them; a moderator will check with the evidence you give.{:else}Prove this project is yours and a moderator will hand it over. Once approved you can propose edits and receive booking inquiries.{/if}
	</p>
	<form method="POST" class="card card-pad mt-8 space-y-5">
		<FormMessage {form} />
		<ClaimFields {values} />
		<div>
			<label class="label" for="message">Anything else? <span class="muted">(optional)</span></label>
			<textarea id="message" name="message" class="input" maxlength="2000">{values.message ?? ''}</textarea>
		</div>
		<div class="flex justify-end gap-3">
			<a href="/projects/{data.project.slug}" class="btn-ghost">Cancel</a>
			<button class="btn-accent">Submit claim</button>
		</div>
	</form>
</div>
