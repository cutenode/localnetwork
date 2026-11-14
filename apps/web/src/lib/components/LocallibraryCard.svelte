<script lang="ts">
	import type { LocallibraryStatus } from '#lib/dj.ts';
	import { formatDateTime, timeAgo } from '#lib/format.ts';

	/**
	 * The dashboard home's locallibrary card: whether the viewer's stats and sets are published and when they last were,
	 * with a link to their newest set, or a way to get locallibrary while they have neither. Null while it loads.
	 */
	let { status }: { status: LocallibraryStatus | null } = $props();

	const PUBLISHING = '/tools/locallibrary/docs/account#publishing';
</script>

{#snippet published(at: string | null)}
	{#if at}Published <time datetime={at} title={formatDateTime(at)}>{timeAgo(at)}</time>{:else}Published{/if}
{/snippet}

{#snippet unpublished()}
	<span class="text-fg-muted">Not published yet.</span> <a class="link" href={PUBLISHING}>How to publish</a>
{/snippet}

<section aria-labelledby="locallibrary" class="card flex flex-col gap-4 p-5 sm:p-6">
	<h2 id="locallibrary" class="m-0 text-emphasis font-semibold">locallibrary</h2>
	{#if !status}
		<div role="status" aria-label="Loading what you published" class="flex flex-col gap-2">
			<span class="h-4 w-2/5 rounded-md bg-well motion-safe:animate-pulse"></span>
			<span class="h-4 w-3/5 rounded-md bg-well motion-safe:animate-pulse"></span>
		</div>
	{:else if status.statsAt || status.sets}
		<dl class="m-0 grid grid-cols-[auto_1fr] gap-x-6 gap-y-2">
			<dt class="text-fg-muted">Stats</dt>
			<dd class="m-0">{#if status.statsAt}{@render published(status.statsAt)}{:else}{@render unpublished()}{/if}</dd>
			<dt class="text-fg-muted">Sets</dt>
			<dd class="m-0">{#if status.sets}{@render published(status.sets.at)}{:else}{@render unpublished()}{/if}</dd>
			{#if status.sets}
				<dt class="text-fg-muted">Newest set</dt>
				<dd class="m-0 min-w-0 truncate"><a class="link" href={status.sets.href}>{status.sets.name}</a></dd>
			{/if}
		</dl>
	{:else}
		<p class="m-0 text-fg-muted">Publish your playing stats and sets from locallibrary and they show on your page and in your press kits.</p>
		<a class="btn-primary self-start" href="/tools/locallibrary">Get locallibrary</a>
	{/if}
	<p class="m-0 text-small"><a class="link" href="/mixes/preview">Preview a mix export</a></p>
</section>
