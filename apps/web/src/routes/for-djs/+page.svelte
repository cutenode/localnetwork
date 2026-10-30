<script lang="ts">
	import { CalendarHeart, ClipboardCheck, Lock, Map, Radio, Users } from '@lucide/svelte';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import Section from '#lib/ui/Section.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';

	const features = [
		{ icon: Map, title: 'Found by distance', body: 'Set your city once. Promoters searching within 25 km of their venue see you first, and your travel radius brings in gigs further out.' },
		{ icon: Radio, title: 'Matched by sound', body: 'Genres are matched by family, so a promoter looking for house finds your deep house and afro house sets too.' },
		{ icon: CalendarHeart, title: 'Inquiries with details', body: 'Every inquiry arrives with the date, venue, capacity, budget and slot. Reply and confirm in a private thread.' },
		{ icon: Users, title: 'Crews and agents', body: 'Up to ten managers per project. Duos, collectives and booking agents can all answer inquiries.' },
		{ icon: Lock, title: 'Private details stay private', body: 'Your booking email, rates and tech rider show only to people you let into a press kit, and you see when they look.' },
		{ icon: ClipboardCheck, title: 'Reviewed changes', body: 'Moderators review every claim and edit, so nobody else can take over or deface your profile.' }
	];
</script>

<svelte:head><title>For DJs · Local Network</title></svelte:head>

{#snippet calls()}
	<Button size="lg" href="/dashboard/projects/new">List your project</Button>
	<Button size="lg" variant="secondary" href="/projects?unclaimed=1">Find and claim yours</Button>
{/snippet}

<Section tone="grid" labelledby="hero-title">
	<SectionHeader
		id="hero-title"
		level={1}
		size="display"
		align="center"
		eyebrow="For DJs"
		title="Get booked by promoters near you."
		lede="Promoters search by sound, distance, slot, room size and budget, and your profile covers all of them. You decide what gets published."
		actions={calls}
	/>
</Section>

<Section labelledby="features-title">
	<div class="flex flex-col gap-8 sm:gap-12">
		<SectionHeader id="features-title" eyebrow="Your profile" tone="low" title="Built around how promoters book." />
		<ul class="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(min(100%,18rem),1fr))] gap-4 p-0">
			{#each features as feature (feature.title)}
				<Card as="li" padding="lg" radius="2xl" class="flex flex-col gap-3 max-sm:p-5">
					<feature.icon class="size-6 text-fg-muted" aria-hidden="true" />
					<h3 class="m-0 text-emphasis font-semibold">{feature.title}</h3>
					<p class="m-0 text-fg-muted">{feature.body}</p>
				</Card>
			{/each}
		</ul>
	</div>
</Section>

<Section tone="sunken" labelledby="mixes-title">
	<SectionHeader
		id="mixes-title"
		align="split"
		eyebrow="Mixes"
		tone="mid"
		title="Tracklists for your mixes."
		lede="Export a mix from locallibrary and attach it to the mix on your profile. Visitors see what you played, the tempo, the key moves and every transition."
	>
		{#snippet actions()}
			<Button variant="secondary" href="/mixes/preview">Preview an export</Button>
			<Button variant="ghost" href="/tools/locallibrary">About locallibrary</Button>
		{/snippet}
	</SectionHeader>
</Section>

<Section tone="grid" labelledby="closing-title">
	<SectionHeader id="closing-title" align="center" title="Already on Local Network?" lede="An admin may have added your project already. Claim it, or list a new one. Moderators review both." actions={calls} />
</Section>
