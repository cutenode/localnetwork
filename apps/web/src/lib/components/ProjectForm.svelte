<script lang="ts">
	import type { Snippet } from 'svelte';
	import { enhance } from '$app/forms';
	import { PROJECT_KINDS } from '#lib/taxonomy/options.ts';
	import { FormDraft } from '#lib/form-draft.svelte.ts';
	import { formValues } from '#lib/project-form.ts';
	import type { DjSetView, ProjectFormValues } from '#lib/types.ts';
	import DraftNotice from './DraftNotice.svelte';
	import BasicsFields from './form/BasicsFields.svelte';
	import FormSection from './form/FormSection.svelte';
	import PerformerFields from './form/PerformerFields.svelte';

	/** A project's profile: the basics and performer fields every profile has, its kind, and its private booking details. */
	let {
		values = {},
		action = '',
		submitLabel = 'Submit for review',
		showDetails = true,
		showMessage = false,
		avatarUrl = null,
		geocoder = true,
		draft,
		sets,
		children,
		footer
	}: {
		values?: ProjectFormValues;
		action?: string;
		submitLabel?: string;
		showDetails?: boolean;
		showMessage?: boolean;
		avatarUrl?: string | null;
		geocoder?: boolean;
		/** Where to keep the unsent form as a draft in this browser (see `draftKey`). No key, no draft. */
		draft?: string;
		/** The managers' published sets (null when they can't be loaded), for a picker on each mix. No picker without. */
		sets?: Promise<DjSetView[] | null>;
		/** Extra fields, drawn from the values the form shows (a restored draft's, or `values`). */
		children?: Snippet<[ProjectFormValues]>;
		footer?: Snippet;
	} = $props();

	let submitting = $state(false);

	// The fields redraw under `revision`, so their rows start over from `shown` with nothing else to reset.
	const formDraft = new FormDraft(
		() => draft,
		(form) => formValues(new FormData(form))
	);

	/** What the fields show: the restored draft, or the values the page passed. */
	const shown = $derived(formDraft.values ?? values);

	const v = (key: string) => String(shown[key] ?? '');
</script>

{#snippet kind()}
	<div>
		<label class="label" for="kind">Type</label>
		<select id="kind" class="input" name="kind">
			{#each PROJECT_KINDS as option (option.value)}<option value={option.value} selected={v('kind') === option.value}>{option.label}</option>{/each}
		</select>
	</div>
{/snippet}

<form
	method="POST"
	{action}
	enctype="multipart/form-data"
	class="flex flex-col gap-6"
	{@attach formDraft.attach}
	use:enhance={({ formElement }) => {
		submitting = true;
		return async ({ result, update }) => {
			if (result.type === 'success' || result.type === 'redirect') formDraft.sent(formElement);
			await update({ reset: false });
			submitting = false;
		};
	}}
>
	<DraftNotice draft={formDraft} files />
	{#key formDraft.revision}
		<BasicsFields values={shown} {avatarUrl} {geocoder} nameLabel="Project name" description="How promoters will see the project in search results." placeRequired nameAside={kind} />
		<PerformerFields values={shown} {sets} />

		{#if showDetails}
			<FormSection title="Private booking details" description="Only signed-in members see these. They're stored in a private atproto space, not your public repo.">
				<div class="grid gap-4 sm:grid-cols-2">
					<div>
						<label class="label" for="bookingEmail">Booking email</label>
						<input id="bookingEmail" class="input" type="email" name="bookingEmail" value={v('bookingEmail')} />
					</div>
					<div>
						<label class="label" for="bookingPhone">Booking phone</label>
						<input id="bookingPhone" class="input" name="bookingPhone" value={v('bookingPhone')} />
					</div>
				</div>
				<div>
					<label class="label" for="rateNotes">Rates</label>
					<textarea id="rateNotes" class="input py-2.5" name="rateNotes">{v('rateNotes')}</textarea>
				</div>
				<div class="grid gap-4 sm:grid-cols-2">
					<div>
						<label class="label" for="techRider">Tech rider</label>
						<textarea id="techRider" class="input min-h-28 py-2.5" name="techRider" placeholder="2× CDJ-3000, DJM-900NXS2, booth monitors">{v('techRider')}</textarea>
					</div>
					<div>
						<label class="label" for="hospitality">Hospitality</label>
						<textarea id="hospitality" class="input min-h-28 py-2.5" name="hospitality">{v('hospitality')}</textarea>
					</div>
				</div>
			</FormSection>
		{/if}

		{#if children}{@render children(shown)}{/if}

		{#if showMessage}
			<div class="card p-5 sm:p-6">
				<label class="label" for="message">Note to moderators <span class="font-normal text-fg-subtle">(optional)</span></label>
				<textarea id="message" class="input py-2.5" name="message" maxlength="2000">{v('message')}</textarea>
			</div>
		{/if}
	{/key}

	<div class="flex flex-wrap items-center justify-end gap-3">
		{#if footer}{@render footer()}{/if}
		<button class="btn-primary" disabled={submitting}>{submitting ? 'Saving…' : submitLabel}</button>
	</div>
</form>
