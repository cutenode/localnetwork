<script lang="ts">
	/**
	 * The two ways to find DJs, at the top of both: a filtered search of the directory, or one show
	 * scored against every listed DJ. `current` is the page it sits on.
	 */
	let { current }: { current: '/projects' | '/match' } = $props();

	const modes = [
		{ href: '/projects', label: 'Search' },
		{ href: '/match', label: 'Match a show' }
	];
</script>

<nav aria-label="Find DJs" class="inline-flex self-start rounded-lg border border-line bg-surface p-1">
	{#each modes as mode (mode.href)}
		<a
			href={mode.href}
			aria-current={mode.href === current ? 'page' : undefined}
			class="flex min-h-9 items-center rounded-md px-3.5 text-small text-fg-muted no-underline transition-colors hover:text-fg aria-[current=page]:bg-well aria-[current=page]:font-medium aria-[current=page]:text-fg">{mode.label}</a
		>
	{/each}
</nav>
