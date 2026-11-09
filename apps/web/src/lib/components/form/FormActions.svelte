<script lang="ts">
	import { CircleAlert, CircleCheck } from '@lucide/svelte';
	import { type Snippet, tick, untrack } from 'svelte';
	import { beforeNavigate } from '$app/navigation';

	/**
	 * The end of a form: its buttons, in view all the way down a long page, beside what the last send came back with, so
	 * nobody saving from the bottom misses a message at the top. A control the server refused is marked, brought into
	 * view and given the reason. Changes not sent yet show here, and leaving with them asks first, unless they're `kept`
	 * as a draft (see FormDraft). A sign-in that ran out gets SignInAgain instead.
	 */
	let {
		form,
		submitting = false,
		kept = false,
		guardNavigation = beforeNavigate,
		children
	}: {
		form: { error?: string; success?: string; reauth?: boolean; field?: { name: string; row: number } } | null | undefined;
		submitting?: boolean;
		kept?: boolean;
		/** Where the leave guard goes: the app's navigations, unless a harness drives its own. */
		guardNavigation?: (guard: (navigation: { willUnload: boolean; cancel: () => void }) => void) => void;
		children: Snippet;
	} = $props();

	const error = $derived(form?.reauth ? undefined : form?.error);

	let dirty = $state(false);

	let status = $state<HTMLElement>();

	let element: HTMLFormElement | null = null;

	let baseline = '';

	let clearField = () => {};

	const read = (target: HTMLFormElement) =>
		JSON.stringify([...new FormData(target)].map(([name, value]) => [name, value instanceof File ? `${value.name} ${value.size}` : value]));

	async function compare() {
		await tick();

		if (element) dirty = read(element) !== baseline;
	}

	/** Takes the form's values as sent, once its own enhancements have drawn. */
	async function settle() {
		await tick();

		if (!element) return;
		baseline = read(element);
		dirty = false;
	}

	const track = (bar: HTMLElement) => {
		element = bar.closest('form');

		if (!element) return;
		void settle();
		// Rows come and go without an input event, and a restored draft redraws the fields.
		const redraws = new MutationObserver(() => void compare());
		redraws.observe(element, { childList: true, subtree: true });
		element.addEventListener('input', compare);
		element.addEventListener('change', compare);

		return () => {
			redraws.disconnect();
			element?.removeEventListener('input', compare);
			element?.removeEventListener('change', compare);
			element = null;
		};
	};

	/** The control a refused send names, in its row, or the visible input standing in for a hidden one. */
	function refused(target: HTMLFormElement, field: { name: string; row: number }) {
		const named = target.elements.namedItem(field.name);
		const control = named instanceof RadioNodeList ? named[field.row] : named;

		if (!(control instanceof HTMLElement)) return null;

		return control instanceof HTMLInputElement && control.type === 'hidden' ? target.querySelector<HTMLElement>(`#${CSS.escape(field.name)}`) : control;
	}

	function markField(control: HTMLElement, reason: string) {
		const message = document.createElement('p');
		message.id = `${control.id || control.getAttribute('name')}-refused`;
		message.className = 'order-last col-span-full mt-1.5 mb-0 basis-full text-label text-critical';
		message.textContent = reason;
		// Last in the control's row, or in the box around it for a control like the genre tags, so it wraps under them.
		(control.closest('.input') ?? control).parentElement?.append(message);
		const described = control.getAttribute('aria-describedby');
		control.setAttribute('aria-invalid', 'true');
		control.setAttribute('aria-describedby', message.id);

		clearField = () => {
			message.remove();
			control.removeAttribute('aria-invalid');

			if (described) control.setAttribute('aria-describedby', described);
			else control.removeAttribute('aria-describedby');

			control.removeEventListener('input', clearField);
			clearField = () => {};
		};

		control.addEventListener('input', clearField);
		control.scrollIntoView({ block: 'center' });
		control.focus({ preventScroll: true });
	}

	/** A disabled button loses focus, so a send that stays on the page hands it to the control at fault or the outcome. */
	async function finish() {
		await tick();

		if (!element) return;
		const control = error && form?.field ? refused(element, form.field) : null;

		if (!error) await settle();

		if (control && error) markField(control, error);
		else if (document.activeElement === document.body) status?.focus({ preventScroll: true });
	}

	let sent = false;

	$effect(() => {
		if (submitting) {
			sent = true;
			clearField();
		} else if (sent) {
			sent = false;
			void finish();
		}
	});

	// Registered once, as the component starts, the way beforeNavigate has to be.
	untrack(() => guardNavigation)((navigation) => {
		if (!dirty || kept || submitting) return;

		// Leaving the site or reloading: cancelling is what brings up the browser's own question.
		if (navigation.willUnload || !confirm('Leave without saving your changes?')) navigation.cancel();
	});
</script>

<div {@attach track} data-form-actions class="sticky bottom-0 flex flex-wrap items-center justify-end gap-x-4 gap-y-2 border-t border-line bg-canvas py-3 pb-[max(0.75rem,env(safe-area-inset-bottom))]">
	<div bind:this={status} tabindex="-1" class="min-w-0 flex-1 basis-48 text-small outline-none">
		{#if error}
			<p role="alert" class="m-0 flex items-start gap-2 text-fg"><CircleAlert aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-critical" />{error}</p>
		{:else if form?.success && !dirty}
			<p role="status" class="m-0 flex items-start gap-2 text-fg-soft"><CircleCheck aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-good" />{form.success}</p>
		{:else if dirty}
			<p class="m-0 flex items-center gap-2 text-fg-muted"><span aria-hidden="true" class="size-1.5 shrink-0 rounded-full bg-fg-muted"></span>{kept ? 'Kept in this browser' : 'Unsaved changes'}</p>
		{/if}
	</div>
	<div class="flex flex-wrap justify-end gap-2 max-sm:w-full max-sm:[&>*]:flex-1">{@render children()}</div>
</div>
