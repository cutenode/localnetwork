<script lang="ts">
	import { enhance } from '$app/forms';
	import { page } from '$app/state';
	import { ExternalLink } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import BasicsFields from '#lib/components/form/BasicsFields.svelte';
	import FormSection from '#lib/components/form/FormSection.svelte';
	import PerformerFields from '#lib/components/form/PerformerFields.svelte';
	import PromoterFields from '#lib/components/form/PromoterFields.svelte';
	import SignInAgain from '#lib/components/SignInAgain.svelte';
	import { profileHref } from '#lib/format.ts';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const values = $derived(form?.values ?? data.values);

	// What the boxes say now, from the saved profile until ticked: the fields of a section show while its box is.
	let performer = $derived(data.sectionsOn.performer);

	let promoter = $derived(data.sectionsOn.promoter);

	let submitting = $state(false);

	const reauth = $derived(form?.reauth && form.error ? form.error : data.needsSignIn ? 'Sign in again to save your profile' : null);

	const SECTION_NAMES = { performer: 'DJ', promoter: 'promoter' } as const;
</script>

<svelte:head><title>Your profile · Local Network</title></svelte:head>

<div class="flex max-w-4xl flex-col gap-8">
	<div class="flex flex-wrap items-end justify-between gap-4">
		<SectionHeader level={1} eyebrow="Profile" title={data.profile ? 'Your profile' : 'Set up your profile'} />
		{#if data.profile && data.viewer}<a class="btn-secondary" href={profileHref(data.viewer)}><ExternalLink class="size-4" />View profile</a>{/if}
	</div>

	{#if reauth && data.viewer}
		<SignInAgain did={data.viewer.did} next={page.url.pathname} error={reauth} />
	{:else if page.url.searchParams.has('saved') && !form}
		<FormMessage form={{ success: 'Saved. Your profile shows the changes now.' }} />
	{:else if page.url.searchParams.has('created') && !form}
		<FormMessage form={{ success: 'Your profile is up. Add more here whenever you like.' }} />
	{:else}
		<FormMessage {form} />
	{/if}

	<form
		method="POST"
		action="?/save"
		enctype="multipart/form-data"
		class="flex flex-col gap-6"
		use:enhance={({ cancel }) => {
			const dropped = (['performer', 'promoter'] as const).filter((section) => data.sectionsOn[section] && !{ performer, promoter }[section]);

			if (dropped.length && !confirm(`Turning off your ${dropped.map((section) => SECTION_NAMES[section]).join(' and ')} section deletes what's in it. Save anyway?`)) {
				cancel();

				return;
			}

			submitting = true;

			return async ({ update }) => {
				await update({ reset: false });
				submitting = false;
			};
		}}
	>
		<input type="hidden" name="cid" value={data.profile?.cid ?? ''} />
		<BasicsFields {values} avatarUrl={data.profile?.avatarUrl} geocoder={data.geocoder} description="How people see you across Local Network." />

		<FormSection title="What you do" description="Each adds its own part of your profile. You can turn them on and off any time.">
			<div class="flex items-start gap-3">
				<input id="section-performer" type="checkbox" class="checkbox mt-0.5" name="section" value="performer" aria-describedby="section-performer-hint" bind:checked={performer} />
				<div class="flex flex-col gap-0.5">
					<label class="text-fg" for="section-performer">I play out as a DJ</label>
					<p id="section-performer-hint" class="m-0 text-small text-fg-muted">Your sound, sets, fees and mixes, for promoters looking to book.</p>
				</div>
			</div>
			<div class="flex items-start gap-3">
				<input id="section-promoter" type="checkbox" class="checkbox mt-0.5" name="section" value="promoter" aria-describedby="section-promoter-hint" bind:checked={promoter} />
				<div class="flex flex-col gap-0.5">
					<label class="text-fg" for="section-promoter">I put on shows</label>
					<p id="section-promoter-hint" class="m-0 text-small text-fg-muted">The cities you book in.</p>
				</div>
			</div>
		</FormSection>

		{#if performer}<PerformerFields {values} sets={data.sets} />{/if}
		{#if promoter}<PromoterFields {values} />{/if}

		<div class="flex flex-wrap items-center justify-end gap-3">
			<button class="btn-primary" disabled={submitting}>{submitting ? 'Saving…' : 'Save profile'}</button>
		</div>
	</form>
</div>
