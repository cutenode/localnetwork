<script lang="ts">
	import { CircleCheck } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import { REPORT_LIMITS, type ReportFacts } from '#lib/bug-report.ts';
	import FormMessage from './FormMessage.svelte';

	/**
	 * A bug report posted from the viewer's Atmosphere account to userinput.app. `consent` asks them to
	 * approve posting there first; an error offers GitHub's form, filled in with what they wrote.
	 */
	let {
		step,
		github,
		facts = {},
		values = {},
		error,
		sending = $bindable(false)
	}: {
		step: { kind: 'consent' } | { kind: 'form' } | { kind: 'sent'; url: string };
		github: string;
		/** The page the report is about and the browser, which go into the post with what they write. */
		facts?: ReportFacts;
		values?: { title?: string; body?: string };
		error?: string;
		sending?: boolean;
	} = $props();

	const from = $derived(facts.page ?? '');

	// The actions keep `from` in the address, so the page stays known through a failure and a retry.
	const action = (name: string) => (from ? `?from=${encodeURIComponent(from)}&/${name}` : `?/${name}`);

	const added = $derived([facts.page, facts.browser].filter(Boolean).join(', '));

	const submit = () => {
		sending = true;

		return async ({ update }: { update: (options?: { reset?: boolean }) => Promise<void> }) => {
			await update({ reset: false });
			sending = false;
		};
	};
</script>

{#if step.kind === 'sent'}
	<div class="card flex flex-col items-start gap-4 p-5 sm:p-6">
		<CircleCheck aria-hidden="true" class="size-6 text-good" />
		<div role="status" class="flex flex-col gap-1.5">
			<h2 class="m-0 text-emphasis font-semibold">Thanks, it's posted</h2>
			<p class="m-0 text-small text-fg-muted">Follow replies on userinput.app.</p>
		</div>
		<div class="flex w-full flex-col gap-3 sm:w-auto sm:flex-row">
			<a class="btn-primary" href={step.url}>View your report</a>
			<a class="btn-ghost" href={from || '/'}>Back to the site</a>
		</div>
	</div>
{:else if step.kind === 'consent'}
	<!-- A full page POST: it redirects to the account's server, which client-side navigation can't follow. -->
	<form method="POST" action={action('allow')} class="card flex flex-col gap-4 p-5 sm:p-6" onsubmit={() => (sending = true)}>
		<FormMessage form={error ? { error } : null} />
		<div class="flex flex-col gap-1.5">
			<h2 class="m-0 text-emphasis font-semibold">Approve posting first</h2>
			<p class="m-0 text-small text-fg-muted">Your account's server asks once. Local Network can only add new posts to userinput.app, nothing else.</p>
		</div>
		<div class="flex flex-col-reverse gap-3 sm:flex-row sm:justify-end">
			<a class="btn-ghost" href={github}>Use GitHub instead</a>
			<button class="btn-primary" disabled={sending}>{sending ? 'Opening…' : 'Approve'}</button>
		</div>
	</form>
{:else}
	<form method="POST" action={action('send')} class="card flex flex-col gap-5 p-5 sm:p-6" use:enhance={submit}>
		<FormMessage form={error ? { error } : null} />
		<div>
			<label class="label" for="title">Title</label>
			<input id="title" name="title" class="input" required maxlength={REPORT_LIMITS.title} value={values.title ?? ''} placeholder="The map doesn't load on a show" />
		</div>
		<div>
			<label class="label" for="body">What happened?</label>
			<textarea id="body" name="body" class="input min-h-36 py-2.5" maxlength={REPORT_LIMITS.body} aria-describedby={added ? 'body-hint' : undefined} placeholder="What you did, what you expected, and what happened instead.">{values.body ?? ''}</textarea>
			{#if added}<p id="body-hint" class="hint m-0">Also sent: {added}</p>{/if}
		</div>
		<div class="flex flex-col-reverse gap-3 sm:flex-row sm:justify-end">
			{#if error}
				<a class="btn-secondary" href={github}>Send it on GitHub</a>
				<button class="btn-primary" disabled={sending}>{sending ? 'Posting…' : 'Try again'}</button>
			{:else}
				<a class="btn-ghost" href={github}>Use GitHub instead</a>
				<button class="btn-primary" disabled={sending}>{sending ? 'Posting…' : 'Post report'}</button>
			{/if}
		</div>
	</form>
{/if}
