<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import ResponsiveTable from './ResponsiveTable.svelte';

	const { Story } = defineMeta({ title: 'Primitives/Responsive table', component: ResponsiveTable, tags: ['autodocs'] });

	type Show = { id: string; name: string; venue: string; date: string; capacity: number };

	const shows: Show[] = [
		{ id: 'a', name: 'Warehouse night', venue: 'Tresor, Berlin', date: 'Fri 16 Oct', capacity: 900 },
		{ id: 'b', name: 'Halloween at the Depot, a long name that runs past the edge of a phone', venue: 'The Depot, Leipzig', date: 'Sat 31 Oct', capacity: 1500 },
		{ id: 'c', name: 'Sunday garden', venue: 'Else, Berlin', date: 'Sun 1 Nov', capacity: 400 }
	];

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const tablet = { viewport: { value: 'tablet', isRotated: false } };
</script>

{#snippet showTable(from: 'xl' | '3xl', wide: boolean)}
	<ResponsiveTable rows={shows} key={(show) => show.id} {from}>
		{#snippet head()}<th>Show</th><th>Date</th>{#if wide}<th>Venue</th><th>Capacity</th>{/if}{/snippet}
		{#snippet row(show)}
			<td>{show.name}</td><td class="whitespace-nowrap text-fg-muted">{show.date}</td>
			{#if wide}<td>{show.venue}</td><td class="text-fg-muted">{show.capacity}</td>{/if}
		{/snippet}
		{#snippet card(show)}
			<p class="font-medium">{show.name}</p>
			<p class="text-label text-fg-muted">{show.date}{#if wide} · {show.venue} · {show.capacity}{/if}</p>
		{/snippet}
	</ResponsiveTable>
{/snippet}

<Story name="Table" asChild>{@render showTable('3xl', true)}</Story>

<Story name="Phone" globals={phone} asChild>{@render showTable('3xl', true)}</Story>

<!-- Two short columns fit from xl (36rem), so a tablet already gets the table. -->
<Story name="Few columns, tablet" globals={tablet} asChild>{@render showTable('xl', false)}</Story>
