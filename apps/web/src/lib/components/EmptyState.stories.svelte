<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import Button from '#lib/ui/Button.svelte';
	import EmptyState from './EmptyState.svelte';

	const { Story } = defineMeta({ title: 'Primitives/Empty state', component: EmptyState, tags: ['autodocs'] });
</script>

<Story name="Title only" args={{ title: 'Nothing on this profile yet' }} />

<Story name="With a reason" args={{ title: 'Nothing here', body: 'No requests match this filter.' }} />

<Story name="With a next step" asChild>
	<EmptyState title="No submissions yet" body="New projects, claims and profile changes you send to the moderators show up here.">
		<Button variant="secondary" size="sm" href="/dashboard/projects/new">List a project</Button>
	</EmptyState>
</Story>

<Story name="Error" asChild>
	<EmptyState title="Couldn't load your data" body="Something went wrong on our side. Try again in a minute.">
		<Button variant="secondary" size="sm" href="/dashboard/data">Try again</Button>
	</EmptyState>
</Story>

<Story name="Inside a card" asChild>
	<section class="card">
		<EmptyState framed={false} title="No submissions yet" body="New projects, claims and profile changes you send to the moderators show up here." />
	</section>
</Story>

<Story name="Phone" asChild globals={phone}>
	<EmptyState title="No submissions yet" body="New projects, claims and profile changes you send to the moderators show up here.">
		<Button variant="secondary" size="sm" href="/dashboard/projects/new">List a project</Button>
	</EmptyState>
</Story>
