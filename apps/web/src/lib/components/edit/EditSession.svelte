<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { enhance as enhanceForm, type SubmitFunction } from '$app/forms';
	import { beforeNavigate } from '$app/navigation';
	import { FormDraft } from '#lib/form-draft.svelte.ts';
	import LeaveDialog from './LeaveDialog.svelte';
	import { type FieldEntries, fieldValues, mergeConflict, readFields, writeFields } from './fields.ts';
	import type { ConflictField, EditActivity, EditRefusal, Navigate, PageStatus } from './types.ts';

	/**
	 * What edit mode does once its code has loaded, for as long as the page is in it: it keeps what's typed in this
	 * browser as a draft (FormDraft), tells the bar what's unsaved, saves the page's form to its `?/save` action and
	 * takes the answer, asks before a link inside the site leaves changes behind, and saves on Cmd/Ctrl+S. A save that
	 * met a newer version takes in what only the other person changed and hands the bar the fields both changed
	 * (`conflict`); a refused field gets its reason (`refusal`) and focus. `onfinish` says how the edit ended, with
	 * what was typed when a discard can be undone.
	 */
	let {
		form,
		name,
		status,
		labels,
		draftKey,
		restore,
		viewHref,
		activity = $bindable(),
		dirty = $bindable(),
		conflict = $bindable(),
		refusal = $bindable(),
		navigate,
		guardNavigation = beforeNavigate,
		enhance = enhanceForm,
		onfinish
	}: {
		form: HTMLFormElement;
		name: string;
		status: PageStatus;
		/** Each field's label, by name, for a conflict's choices. */
		labels: Record<string, string>;
		draftKey: string | undefined;
		/** What was typed before a discard that's being undone. */
		restore?: FieldEntries;
		viewHref: string;
		activity: EditActivity;
		dirty: boolean | undefined;
		conflict: ConflictField[];
		refusal: EditRefusal | null;
		navigate: Navigate;
		guardNavigation?: (guard: (navigation: { willUnload: boolean; to: { url: URL } | null; cancel: () => void }) => void) => void;
		/** How the form is sent and its answer taken: SvelteKit's, unless a harness answers for the server. */
		enhance?: typeof enhanceForm;
		onfinish: (message: string | null, typed?: FieldEntries) => void;
	} = $props();

	/** The page as the form opened on it, or as the last conflict found it. */
	let base: FieldEntries = [];

	/** Where a link was taking someone with unsaved changes, while they're asked. */
	let leaveTo = $state<URL | null>(null);

	/** Where to go once a save from the leave question goes through. */
	let afterSave: URL | null = null;

	/** Set once the edit is over, so going back to the page isn't asked about. */
	let done = false;

	const draft = new FormDraft<FieldEntries>(() => draftKey, readFields);

	const measure = () => {
		dirty = JSON.stringify(readFields(form)) !== JSON.stringify(base);
	};

	// A restored draft goes into the fields as if typed.
	$effect(() => {
		if (!draft.revision) return;
		untrack(() => {
			if (draft.values) writeFields(form, draft.values, true);
			measure();
		});
	});

	// A conflict's picks go into their fields as they're made, so Save sends them.
	$effect(() => {
		const picks = conflict.map((field): [string, string] => [field.name, field.keep === 'theirs' ? field.theirs : field.yours]);

		if (picks.length)
			untrack(() => {
				writeFields(form, picks);
				measure();
			});
	});

	$effect(() => {
		// The fields are drawn by now, so this is what the form opened on.
		base = readFields(form);
		dirty = false;
		const detachDraft = untrack(() => draft.attach(form));
		const enhanced = enhance(form, submit);
		// Ticked boxes and picked options fire `input` too; `change` would also fire as a field leaves the page.
		form.addEventListener('input', measure);

		void (async () => {
			await tick();

			if (restore) writeFields(form, restore, true);

			measure();

			// Typing starts at once in the first field to type in (a picture's file input or its Remove box can come first),
			// unless a refused save already put focus on its field.
			if (!refusal?.field)
				form.querySelector<HTMLElement>('textarea, input:not([type=hidden], [type=file], [type=checkbox], [type=radio], [type=submit], [type=button]), select')?.focus();
		})();

		return () => {
			detachDraft?.();
			enhanced.destroy();
			form.removeEventListener('input', measure);
		};
	});

	const submit: SubmitFunction<undefined, EditRefusal> = ({ submitter }) => {
		const publishing = submitter?.getAttribute('name') === 'publish';
		activity = 'saving';
		refusal = null;

		return async ({ result }) => {
			if (result.type === 'redirect') {
				draft.sent(form);
				done = true;
				await navigate(afterSave?.href ?? result.location, { refreshAll: true, reset: !!afterSave });
				activity = 'idle';
				conflict = [];
				onfinish(publishing ? 'Published' : status === 'draft' ? 'Draft saved' : 'Saved');

				return;
			}

			afterSave = null;

			// An error, or a refusal from something other than savePage, has nothing to show but the bar's own words.
			const refused = result.type === 'failure' ? result.data : undefined;

			if (!refused) {
				activity = 'failed';

				return;
			}

			if (refused.conflict) {
				const { version, values: theirs } = refused.conflict;
				const merged = mergeConflict(fieldValues(base), theirs, fieldValues(readFields(form)), labels);
				writeFields(form, [...merged.take, ['cid', version]]);
				base = base.map(([field, value]) => [field, field === 'cid' ? version : (theirs[field] ?? value)]);
				conflict = merged.fields;
				activity = 'conflict';
				measure();

				return;
			}

			activity = refused.reauth ? 'reauth' : 'failed';
			refusal = refused;
			await tick();

			if (refused.field) form.querySelectorAll<HTMLElement>(`[name="${CSS.escape(refused.field.name)}"]`)[refused.field.row]?.focus();
		};
	};

	/** Drops what's typed and goes back to the page, handing the typing on so the toast can undo it. */
	export async function discard() {
		const typed = readFields(form);
		const changed = dirty;
		draft.discard();
		done = true;
		activity = 'idle';
		conflict = [];
		refusal = null;
		await navigate(viewHref, { reset: false });
		onfinish(changed ? 'Changes discarded' : null, changed ? typed : undefined);
	}

	function onkeydown(event: KeyboardEvent) {
		if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey || event.key.toLowerCase() !== 's') return;
		// The browser would save the page as a file.
		event.preventDefault();

		if (activity !== 'saving') form.requestSubmit();
	}

	// Registered once, as the component starts, the way beforeNavigate has to be.
	untrack(() => guardNavigation)((navigation) => {
		if (done || !dirty || activity === 'saving') return;
		// Leaving the site or reloading: cancelling is what brings up the browser's own question.
		navigation.cancel();

		if (!navigation.willUnload && navigation.to) leaveTo = navigation.to.url;
	});

	async function leaveDiscarding() {
		const to = leaveTo;
		leaveTo = null;
		draft.discard();
		done = true;

		if (to) await navigate(to.href);
	}

	function leaveSaving() {
		afterSave = leaveTo;
		leaveTo = null;
		form.requestSubmit();
	}
</script>

<svelte:window {onkeydown} />

<LeaveDialog open={!!leaveTo} {name} onstay={() => (leaveTo = null)} ondiscard={leaveDiscarding} onsave={leaveSaving} />
