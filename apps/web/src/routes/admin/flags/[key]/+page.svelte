<script lang="ts">
	import { ArrowLeft } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import { SHOWS_ON } from '#lib/flags/shows-on.ts';
	import { formFields } from '#lib/form-fields.svelte.ts';
	import { formatShowDate } from '#lib/format.ts';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const flag = $derived(data.flag);

	/** The kinds of page the flag changes, as its catalog file lists them. */
	const showsOn = $derived(flag.showsOn.map((kind) => SHOWS_ON[kind]).join(', ') || 'None listed');

	/** What this browser sees for the flag instead of what's saved, if anything. */
	const previewing = $derived(data.flagPreview?.[flag.key]);

	/** Signed-out visitors come last, so their switch waits until what's saved has the flag on for every account. */
	const everyAccount = $derived(flag.everyone || flag.percentage === 100);

	const fields = $derived(
		formFields({
			off: !flag.enabled,
			everyone: flag.everyone,
			percentage: flag.percentage,
			signedOut: flag.signedOut,
			groups: flag.groups.map(({ rkey }) => rkey),
			accounts: flag.accounts.map((account) => account.handle ?? account.did).join('\n')
		})
	);

	/** Why the signed-out switch is held, or what lowering the flag does to it. */
	const signedOutHint = $derived.by(() => {
		if (everyAccount) return 'Turns off if it drops below every account.';

		if (fields.everyone || fields.percentage === 100) return 'Save first, then this opens.';

		return "Only once it's on for every account.";
	});
</script>

<svelte:head><title>{flag.key} · Flags · Local Network</title></svelte:head>

<div class="flex flex-col gap-6">
	<a href="/admin/flags" class="inline-flex items-center gap-1.5 text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />Flags</a>
	<div class="flex flex-col gap-3">
		<SectionHeader level={1} title={flag.key} lede={flag.description} />
		{#if flag.readyToRemove}
			<p class="flex flex-wrap items-center gap-2 text-small text-fg-muted"><span class="pill bg-warning/10 text-warning">Ready to remove</span><span>Delete <code class="font-mono break-all text-fg">flags/catalog/{flag.key}.json</code> and the code it gates.</span></p>
		{/if}
		<div class="flex flex-col items-start gap-1 text-small text-fg-muted">
			<p>Shows on: <span class="text-fg">{showsOn}</span></p>
			<!-- removeBy is a calendar date, so it's read in UTC: local time would show it a day early west of Greenwich. -->
			{#if flag.removeBy}<p>Remove by: <span class="text-fg">{formatShowDate(flag.removeBy)}</span></p>{/if}
			<a class="link" href="/admin/audit?action=flag&q={flag.key}">Changes in the audit log</a>
		</div>
	</div>
	<FormMessage {form} />
	<form method="POST" action="?/save" class="card flex max-w-2xl flex-col gap-5 p-5" use:enhance={() => ({ update }) => update({ reset: false })}>
		<label class="flex items-start gap-2.5">
			<input type="checkbox" role="switch" class="checkbox mt-0.5" name="off" bind:checked={fields.off} aria-describedby="off-hint" />
			<span><span class="font-medium">Off for everyone</span><span id="off-hint" class="hint block">Signed in or not, whatever's set below.</span></span>
		</label>
		<!-- Set apart, not disabled: disabled fields aren't sent, and saving would clear them. Not faded either, so it stays readable. -->
		<div class={['flex flex-col gap-5', fields.off ? 'rounded-lg border border-dashed border-line-strong bg-sunken p-4' : 'border-t border-line pt-5']}>
			{#if fields.off}<p class="hint -mb-2">Kept for when it's back on. None of it applies while it's off.</p>{/if}
			<label class="flex items-start gap-2.5">
				<input type="checkbox" class="checkbox mt-0.5" name="everyone" bind:checked={fields.everyone} aria-describedby="everyone-hint" />
				<span><span class="font-medium">On for everyone</span><span id="everyone-hint" class="hint block">{flag.type === 'ops' ? 'Signed-out visitors too.' : 'Every signed-in account.'}</span></span>
			</label>
			<div>
				<label class="label" for="percentage">Percentage of accounts</label>
				<input id="percentage" name="percentage" type="number" min="0" max="100" step="1" class="input max-w-32" bind:value={fields.percentage} aria-describedby="percentage-hint" />
				<p id="percentage-hint" class="hint">Accounts stay in as this grows.</p>
			</div>
			{#if flag.type === 'release'}
				<!-- Release flags only: an ops flag reaches signed-out visitors once every account has it, so a kill switch reaches callers with no
				     account. Disabled until saved at every account, so it isn't sent; the server turns it off below that anyway. -->
				<label class="flex items-start gap-2.5 has-disabled:text-fg-muted">
					<input type="checkbox" class="checkbox mt-0.5 disabled:cursor-not-allowed disabled:opacity-50" name="signedOut" bind:checked={fields.signedOut} disabled={!everyAccount} aria-describedby="signed-out-hint" />
					<span><span class="font-medium">Signed-out visitors</span><span id="signed-out-hint" class="hint block">{signedOutHint}</span></span>
				</label>
			{/if}
			<fieldset>
				<legend class="label">Groups</legend>
				{#if data.groups.length === 0}
					<p class="hint">No groups yet.{#if data.grants?.includes('flagGroups')}{' '}<a class="link" href="/admin/flags/groups">Create one</a>{/if}</p>
				{:else}
					<ul class="flex flex-col gap-1.5">
						{#each data.groups as group (group.rkey)}
							<li><label class="flex items-center gap-2.5"><input type="checkbox" class="checkbox" name="groups" value={group.rkey} bind:group={fields.groups} />{group.name}<span class="text-label text-fg-subtle">{group.members.length}</span></label></li>
						{/each}
					</ul>
				{/if}
			</fieldset>
			<div>
				<label class="label" for="accounts">Accounts</label>
				<textarea id="accounts" name="accounts" class="input min-h-28 py-2.5 font-mono text-small" aria-describedby="accounts-hint" placeholder="alice.bsky.social" bind:value={fields.accounts}></textarea>
				<p id="accounts-hint" class="hint">One handle or DID per line.</p>
			</div>
		</div>
		<button class="btn-primary self-start">Save</button>
	</form>
	<section class="card flex max-w-2xl flex-col gap-3 p-5" aria-labelledby="preview-title">
		<div>
			<h2 id="preview-title" class="font-medium">Preview in this browser</h2>
			<p class="hint">Only you see it, here, for an hour. Everyone else gets what's saved above.</p>
		</div>
		<form method="POST" action="?/preview" class="flex flex-wrap gap-2" use:enhance>
			<button name="state" value="on" class="btn-secondary btn-sm aria-pressed:border-fg aria-pressed:bg-fg aria-pressed:text-fg-inverse" aria-pressed={previewing === true}>On for me</button>
			<button name="state" value="off" class="btn-secondary btn-sm aria-pressed:border-fg aria-pressed:bg-fg aria-pressed:text-fg-inverse" aria-pressed={previewing === false}>Off for me</button>
			{#if previewing !== undefined}<button name="state" value="clear" class="btn-ghost btn-sm">Stop previewing</button>{/if}
		</form>
	</section>
</div>
