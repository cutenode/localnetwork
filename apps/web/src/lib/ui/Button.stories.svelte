<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Button from './Button.svelte';

	const { Story } = defineMeta({
		title: 'Primitives/Button',
		component: Button,
		tags: ['autodocs'],
		argTypes: {
			variant: { control: 'inline-radio', options: ['primary', 'secondary', 'ghost'] },
			size: { control: 'inline-radio', options: ['sm', 'md', 'lg'] }
		},
		args: { variant: 'primary', size: 'md' }
	});
</script>

<Story name="Primary">
	{#snippet template(args)}<Button variant={args.variant} size={args.size}>Find DJs near you</Button>{/snippet}
</Story>

<Story name="Secondary" args={{ variant: 'secondary' }}>
	{#snippet template(args)}<Button variant={args.variant} size={args.size}>List your project</Button>{/snippet}
</Story>

<Story name="Ghost" args={{ variant: 'ghost' }}>
	{#snippet template(args)}<Button variant={args.variant} size={args.size}>Sign in</Button>{/snippet}
</Story>

<Story name="Sizes" asChild>
	<div class="flex flex-wrap items-center gap-3">
		<Button size="sm">Send inquiry</Button>
		<Button>Match DJs</Button>
		<Button size="lg">Find DJs near you</Button>
	</div>
</Story>

<Story name="As a link" asChild>
	<div class="flex flex-wrap gap-3">
		<Button href="/projects">Find DJs near you</Button>
		<Button href="/dashboard/projects/new" variant="secondary">List your project</Button>
	</div>
</Story>

<Story name="Disabled" asChild>
	<div class="flex flex-wrap gap-3">
		<Button disabled>Send inquiry</Button>
		<Button disabled variant="secondary">Save</Button>
	</div>
</Story>
