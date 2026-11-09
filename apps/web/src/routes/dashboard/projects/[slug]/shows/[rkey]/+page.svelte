<script lang="ts">
	import { ArrowLeft } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import { page } from '$app/state';
	import DeleteRow from '#lib/components/form/DeleteRow.svelte';
	import FormActions from '#lib/components/form/FormActions.svelte';
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

	let saving = $state(false);
</script>

<svelte:head><title>{data.isNew ? 'Announce a show' : 'Edit show'} · {project.profile.name}</title></svelte:head>

<div class="flex max-w-3xl flex-col gap-8">
	<a href="/dashboard/projects/{project.slug}" class="inline-flex items-center gap-1.5 self-start text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />{project.profile.name}</a>
	<header class="flex flex-col gap-3">
		<Eyebrow>{data.isNew ? 'New show' : 'Show'}</Eyebrow>
		<h1 class="m-0 text-title font-semibold">{data.isNew ? 'Announce a show' : data.values.name}</h1>
		<p class="m-0 text-fg-muted">It shows on {project.profile.name}'s page and under Shows, and calendar apps can see it too.</p>
	</header>
	<!-- Its own form posts to sign-in, so it sits outside the show's form: forms can't nest. -->
	{#if form?.reauth && form.error && data.viewer}<SignInAgain did={data.viewer.did} next={page.url.pathname + page.url.search} error={form.error} />{/if}
	{#if data.saved && !form}
		<p role="status" class="notice notice-ok m-0">{data.saved === 'announced' ? 'Announced.' : 'Saved.'} <a class="link" href="/projects/{project.slug}">See it on {project.profile.name}'s page</a></p>
	{/if}
	<form
		method="POST"
		action="?/save"
		class="flex flex-col gap-6"
		use:enhance={() => {
			saving = true;

			return async ({ update }) => {
				await update({ reset: false });
				saving = false;
			};
		}}
	>
		<ShowForm {values} geocoder={data.geocoder} editing={!data.isNew} />
		<FormActions {form} submitting={saving}>
			<a href="/dashboard/projects/{project.slug}#shows" class="btn-ghost">Cancel</a>
			<button class="btn-primary" disabled={saving}>{saving ? 'Saving…' : data.isNew ? 'Announce show' : 'Save changes'}</button>
		</FormActions>
	</form>
	{#if !data.isNew}
		<DeleteRow action="?/delete" label="Delete show" note="Deleting takes it off every listing. To tell people it's off, mark it cancelled instead." warning="This takes it off every listing, and it can't be undone." />
	{/if}
</div>
