<script lang="ts">
	/** Says some nights are missing because a source didn't answer, with a reload of `href` to try again. */
	let { href }: { href: string } = $props();
</script>

<p class="notice notice-info m-0 flex flex-wrap items-center justify-between gap-x-4 gap-y-2">Some of your nights didn't load. Everything else here is up to date.<a class="btn-secondary btn-sm" {href} data-sveltekit-reload>Try again</a></p>
