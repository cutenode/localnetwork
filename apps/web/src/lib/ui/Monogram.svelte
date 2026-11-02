<script lang="ts">
	import { initials } from '#lib/format.ts';
	import { monogramSwatch } from './monogram.ts';

	/** A project's initials on one of the track colours, the stand-in wherever there's no photo. */
	let { name, size = 'md' }: { name: string; size?: 'sm' | 'md' | 'lg' | 'xl' } = $props();

	const sizes = { sm: 'size-8 rounded-[9px] text-caption', md: 'size-10 rounded-lg text-label', lg: 'size-11 rounded-lg text-small', xl: 'size-16 rounded-xl text-heading' };

	const tile = $derived(monogramSwatch(name));
</script>

<span aria-hidden="true" class={['grid shrink-0 place-items-center font-bold', sizes[size]]} style:background-color={tile.background} style:color={tile.ink}>{initials(name)}</span>
