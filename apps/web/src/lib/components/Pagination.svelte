<script lang="ts">
	import { page as pageState } from '$app/state';

	let { page, pageSize, total }: { page: number; pageSize: number; total: number } = $props();

	const pages = $derived(Math.max(1, Math.ceil(total / pageSize)));

	function hrefFor(target: number) {
		const url = new URL(pageState.url.href);
		url.searchParams.set('page', String(target));

		return `${url.pathname}${url.search}`;
	}
</script>

{#if pages > 1}
	<nav class="mt-8 flex items-center justify-center gap-2" aria-label="Pagination">
		{#if page > 1}<a class="btn-secondary btn-sm" href={hrefFor(page - 1)} rel="prev">Previous</a>{/if}
		<span class="muted text-sm">Page {page} of {pages}</span>
		{#if page < pages}<a class="btn-secondary btn-sm" href={hrefFor(page + 1)} rel="next">Next</a>{/if}
	</nav>
{/if}
