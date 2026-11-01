<script lang="ts">
	import { Lock } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import { type EpkAccessState, REQUEST_ROLES, ROLE_LABELS } from '#lib/epk.ts';
	import { formatDate } from '#lib/format.ts';
	import { control } from '#lib/ui/Field.svelte';

	/**
	 * What someone without access sees where the private part would be: how to get it, that they've
	 * asked, or that it isn't open to requests. With `askHref` (the press kit's public page) asking is
	 * a link to the dashboard page that holds the form; without it (that page) it's the form.
	 */
	let { access, name, askHref, error }: { access: EpkAccessState; name: string; askHref?: string; error?: string } = $props();

	let sending = $state(false);

	const instant = $derived((access.kind === 'canAsk' || access.kind === 'signedOut') && access.rule === 'auto');
</script>

<section aria-labelledby="epk-access" class="card flex flex-col gap-4 p-5 print:hidden">
	<div class="flex flex-col gap-1.5">
		<h2 id="epk-access" class="m-0 inline-flex items-center gap-2 text-emphasis font-semibold"><Lock class="size-4" aria-hidden="true" />The rest of this press kit</h2>
		<p class="m-0 text-small text-fg-muted">The full bio, more mixes and photos, press quotes, the rider and how to book {name}.</p>
	</div>
	{#if access.kind === 'signedOut'}
		<p class="m-0 text-small"><a class="btn-primary btn-sm" href={askHref}>Sign in to {instant ? 'see it' : 'ask for it'}</a></p>
	{:else if access.kind === 'canAsk' && askHref}
		<p class="m-0 text-small"><a class="btn-primary btn-sm" href={askHref}>{instant ? 'Get access' : 'Ask for access'}</a></p>
	{:else if access.kind === 'canAsk'}
		<form
			method="POST"
			action="?/request"
			class="flex flex-col gap-3"
			use:enhance={() => {
				sending = true;

				return async ({ update }) => {
					await update();
					sending = false;
				};
			}}
		>
			<div class="grid gap-3 sm:grid-cols-[12rem_1fr]">
				<div class="flex flex-col gap-1.5">
					<label for="role" class="text-label font-medium text-fg-soft">You're a</label>
					<select id="role" name="role" class={control}>
						{#each REQUEST_ROLES as role (role)}<option value={role}>{ROLE_LABELS[role]}</option>{/each}
					</select>
				</div>
				<div class="flex flex-col gap-1.5">
					<label for="message" class="text-label font-medium text-fg-soft">Message <span class="font-normal text-fg-subtle">(optional)</span></label>
					<input id="message" name="message" maxlength="1000" class={control} placeholder="What you're booking or writing about" />
				</div>
			</div>
			{#if error}<p role="alert" class="m-0 text-label text-critical">{error}</p>{/if}
			<p class="m-0 flex flex-wrap items-center gap-3">
				<button class="btn-primary btn-sm" disabled={sending}>{instant ? 'See the press kit' : 'Ask for access'}</button>
				<span class="text-label text-fg-subtle">{instant ? 'You get in straight away.' : `${name} decides.`} Asking shares your handle with them.</span>
			</p>
		</form>
	{:else if access.kind === 'pending'}
		<p role="status" class="notice notice-info m-0">
			{#if access.approved}You're approved. The rest shows once this press kit moves to {name}, the next time the manager who made it signs in.{:else}You asked on {formatDate(access.since)}. You'll see the rest once {name} lets you in.{/if}
		</p>
	{:else if access.kind === 'declined'}
		<p class="notice notice-info m-0">{name} isn't sharing the rest of this press kit with you.</p>
	{:else if access.kind === 'closed'}
		<p class="m-0 text-small text-fg-muted">This press kit isn't taking requests. {name} shares it with people directly.</p>
	{/if}
</section>
