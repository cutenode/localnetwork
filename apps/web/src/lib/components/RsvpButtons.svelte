<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Check, Star } from '@lucide/svelte';
	import { enhance as enhanceForm, type SubmitFunction } from '$app/forms';
	import { RSVP_ANSWERS, type RsvpAnswer, type RsvpState } from '#lib/events.ts';
	import FormMessage from './FormMessage.svelte';
	import SignInAgain from './SignInAgain.svelte';

	type Refusal = { error: string; reauth: boolean };

	/**
	 * Going and Interested on an event, each with how many said so, the viewer's own answer pressed. Pressing it again
	 * takes the answer back. Each is a button posting to the page's `?/rsvp`, so it works without scripts; signed out
	 * (`loginHref`), each links to sign-in, which comes back here. Why an answer didn't go through shows under them, and a
	 * session from before the app asked to write RSVPs gets "Sign in again" (`did`, back to `next`). `children` go in the
	 * same row after them, so a message under them doesn't push the page's other actions apart.
	 */
	let {
		rsvp,
		loginHref,
		did = null,
		next,
		refused = null,
		enhance = enhanceForm,
		children
	}: {
		rsvp: RsvpState;
		loginHref?: string;
		did?: string | null;
		next: string;
		/** A refusal the page came back with, sent without scripts. */
		refused?: Refusal | null;
		/** How the form is sent: SvelteKit's enhance, unless a harness answers for the server. */
		enhance?: typeof enhanceForm;
		children?: Snippet;
	} = $props();

	const LABELS: Record<RsvpAnswer, string> = { going: 'Going', interested: 'Interested' };

	let sending = $state(false);

	/** Why the last answer sent with scripts didn't go through, which stays until the next. */
	let problem = $state<Refusal | null>(null);

	const shown = $derived(problem ?? refused);

	const send: SubmitFunction<undefined, Refusal> = () => {
		sending = true;

		return async ({ result, update }) => {
			problem = result.type === 'failure' && result.data ? result.data : null;

			if (result.type !== 'failure') await update({ reset: false });
			sending = false;
		};
	};
</script>

{#snippet content(answer: RsvpAnswer, pressed: boolean)}
	{#if answer === 'going'}<Check aria-hidden="true" class="size-4" />{:else}<Star aria-hidden="true" class="size-4" />{/if}{LABELS[answer]}
	<!-- The pressed one's count takes its button's color, which the muted one doesn't meet the contrast of. -->
	<span class={['font-mono tabular-nums', !pressed && 'text-fg-muted']}>{rsvp[answer].toLocaleString('en')}<span class="sr-only">{rsvp[answer] === 1 ? ' person' : ' people'}</span></span>
{/snippet}

<div id="rsvp" class="flex flex-col gap-2 sm:items-end">
	{#if loginHref}
		<div class="flex flex-wrap gap-2">
			{#each RSVP_ANSWERS as answer (answer)}<a class="btn-secondary" href={loginHref}>{@render content(answer, false)}</a>{/each}
			{@render children?.()}
		</div>
	{:else}
		<form method="POST" action="?/rsvp" class="flex flex-wrap gap-2" use:enhance={send}>
			{#each RSVP_ANSWERS as answer (answer)}
				{@const pressed = rsvp.answer === answer}
				<button name="answer" value={pressed ? '' : answer} aria-pressed={pressed} class={pressed ? 'btn-primary' : 'btn-secondary'} disabled={sending}>{@render content(answer, pressed)}</button>
			{/each}
			{@render children?.()}
		</form>
	{/if}
	{#if shown?.reauth && did}<SignInAgain {did} {next} error={shown.error} />{:else}<FormMessage form={shown} />{/if}
</div>
