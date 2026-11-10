<script lang="ts">
	import { ArrowLeft } from '@lucide/svelte';
	import { enhance } from '$app/forms';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import { formFields } from '#lib/form-fields.svelte.ts';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const flag = $derived(data.flag);

	const fields = $derived(
		formFields({
			off: !flag.enabled,
			everyone: flag.everyone,
			percentage: flag.percentage,
			groups: flag.groups.map(({ rkey }) => rkey),
			accounts: flag.accounts.map((account) => account.handle ?? account.did).join('\n')
		})
	);
</script>

<svelte:head><title>{flag.key} · Flags · Local Network</title></svelte:head>

<div class="flex flex-col gap-6">
	<a href="/admin/flags" class="inline-flex items-center gap-1.5 text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />Flags</a>
	<SectionHeader level={1} title={flag.key} lede={flag.description} />
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
				<span><span class="font-medium">On for everyone</span><span id="everyone-hint" class="hint block">Signed-out visitors too.</span></span>
			</label>
			<div>
				<label class="label" for="percentage">Percentage of accounts</label>
				<input id="percentage" name="percentage" type="number" min="0" max="100" step="1" class="input max-w-32" bind:value={fields.percentage} aria-describedby="percentage-hint" />
				<p id="percentage-hint" class="hint">Accounts stay in as this grows. Signed-out visitors aren't counted.</p>
			</div>
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
</div>
