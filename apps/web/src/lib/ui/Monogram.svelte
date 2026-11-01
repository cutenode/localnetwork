<script module lang="ts">
	import { hueFor, initials } from '#lib/format.ts';
	import colors from '../../../../../packages/tokens/colors.json' with { type: 'json' };
	import { onColour, type Swatch } from './contrast.ts';

	const palette = Object.values(colors.trackColors);

	/** A name's monogram colours. The colour follows the name, so a project keeps the same one everywhere. */
	export const monogramSwatch = (name: string): Swatch => onColour(palette[hueFor(name) % palette.length]);
</script>

<script lang="ts">
	/** A project's initials on one of the track colours, the stand-in wherever there's no photo. */
	let { name, size = 'md' }: { name: string; size?: 'sm' | 'md' | 'lg' | 'xl' } = $props();

	const sizes = { sm: 'size-8 rounded-[9px] text-caption', md: 'size-10 rounded-lg text-label', lg: 'size-11 rounded-lg text-small', xl: 'size-16 rounded-xl text-heading' };

	const tile = $derived(monogramSwatch(name));
</script>

<span aria-hidden="true" class={['grid shrink-0 place-items-center font-bold', sizes[size]]} style:background-color={tile.background} style:color={tile.ink}>{initials(name)}</span>
