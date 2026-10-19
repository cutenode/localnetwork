<script lang="ts">
	import { ArrowLeft, Trash2 } from '@lucide/svelte';
	import { page } from '$app/state';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import ShowForm from '#lib/components/ShowForm.svelte';
	import SignInAgain from '#lib/components/SignInAgain.svelte';
	import { type ShowFormValues, showFormDefaults } from '#lib/shows.ts';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const project = $derived(data.project);

	// After a failed save, the form shows what was typed instead of the saved show.
	// SAFETY: the save action echoes `{ ...stringValues(form), cancelled }`, the form's text fields and its checkbox.
	const values = $derived(form?.values ? ({ ...showFormDefaults, ...form.values } as ShowFormValues) : data.values);
</script>

<svelte:head><title>{data.isNew ? 'Announce a show' : 'Edit show'} · {project.profile.name}</title></svelte:head>

<div class="container-page flex max-w-3xl flex-col gap-8 py-10 sm:py-14">
	<a href="/projects/{project.slug}" class="inline-flex items-center gap-1.5 self-start text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />{project.profile.name}</a>
	<header class="flex flex-col gap-3">
		<Eyebrow>{data.isNew ? 'New show' : 'Show'}</Eyebrow>
		<h1 class="m-0 text-title font-semibold">{data.isNew ? 'Announce a show' : data.values.name}</h1>
		<p class="m-0 text-fg-muted">It shows on {project.profile.name}'s page and under Shows, and calendar apps can see it too.</p>
	</header>
	<!-- Its own form posts to sign-in, so it sits outside the show's form: forms can't nest. -->
	{#if form?.reauth && form.error && data.viewer}<SignInAgain did={data.viewer.did} next={page.url.pathname + page.url.search} error={form.error} />{/if}
	<form method="POST" action="?/save" class="flex flex-col gap-6">
		{#if !form?.reauth}<FormMessage {form} />{/if}
		<ShowForm {values} geocoder={data.geocoder} editing={!data.isNew} />
		<div class="flex sm:justify-end"><button class="btn-primary w-full sm:w-auto">{data.isNew ? 'Announce show' : 'Save changes'}</button></div>
	</form>
	{#if !data.isNew}
		<form method="POST" action="?/delete" class="flex flex-col gap-3 border-t border-line pt-6 sm:flex-row sm:items-center sm:justify-between">
			<p class="m-0 text-small text-fg-muted">Deleting takes it off every listing. To tell people it's off, mark it cancelled instead.</p>
			<button class="btn-danger shrink-0"><Trash2 class="size-4" />Delete show</button>
		</form>
	{/if}
</div>
