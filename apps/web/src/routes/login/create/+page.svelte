<script lang="ts">
	import { ExternalLink } from '@lucide/svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const withNext = $derived(data.next === '/dashboard' ? '' : `?next=${encodeURIComponent(data.next)}`);
</script>

<svelte:head>
	<title>Make an account · Local Network</title>
	<meta name="description" content="Make an Atmosphere account at a host you choose, then come back to Local Network signed in." />
</svelte:head>

<form method="POST" class="card flex flex-col gap-5 p-5 sm:p-7">
	<div class="flex flex-col gap-2">
		<h1 class="m-0 text-heading font-semibold">Where should it live?</h1>
		<p class="m-0 text-fg-muted">Your account lives with a host you choose. You'll make it on their page, then come straight back.</p>
	</div>
	<FormMessage {form} />
	<input type="hidden" name="next" value={data.next} />
	<ul class="m-0 flex list-none flex-col gap-2 p-0" aria-label="Hosts">
		{#each data.hosts as host, i (host.service)}
			<li>
				<button name="service" value={host.service} class={['flex w-full cursor-pointer items-center gap-3 rounded-lg border px-4 py-3 text-left transition-colors', i === 0 ? 'border-fg-subtle bg-raised hover:border-fg' : 'border-line-strong bg-well hover:border-fg-subtle']}>
					<span class="flex min-w-0 flex-1 flex-col">
						<span class="font-semibold text-fg">{host.name}{#if host.note}<span class="font-normal text-fg-muted">{` · ${host.note}`}</span>{/if}</span>
						<span class="text-label text-fg-muted">{host.domain} · {host.country}</span>
					</span>
					<ExternalLink aria-hidden="true" class="size-4 shrink-0 text-fg-subtle" />
				</button>
			</li>
		{/each}
	</ul>
	<div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 text-small">
		<a class="link" href="/login{withNext}">I already have an account</a>
		<span class="text-fg-subtle">You can move hosts later.</span>
	</div>
</form>
