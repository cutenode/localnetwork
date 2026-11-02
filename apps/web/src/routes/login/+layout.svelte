<script lang="ts">
	import { madeUpBands } from '#lib/mix/wave.ts';
	import Mark from '#lib/ui/Mark.svelte';
	import Waveform from '#lib/ui/Waveform.svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();
</script>

<div class="container-page grid items-center gap-12 py-10 sm:py-20 lg:grid-cols-[1fr_28rem] lg:gap-20">
	<div aria-hidden="true" class="hidden flex-col gap-8 lg:flex">
		<!-- A made-up mix in its three bands, as the art beside the sign-in card. -->
		<Waveform bands={madeUpBands(1200)} class="h-40" />
		<div class="flex items-center gap-3 text-heading font-semibold"><Mark class="h-7" />Local Network</div>
		<p class="m-0 -mt-4 max-w-md text-lede text-fg-muted">Promoters find DJs by sound, distance and budget. DJs keep a profile they own.</p>
	</div>
	<div class="mx-auto w-full max-w-md">
		{@render children()}
	</div>
</div>
