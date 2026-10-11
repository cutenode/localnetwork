<script lang="ts">
	import { CalendarHeart, ClipboardCheck, ListMusic, Lock, Map, Radio, Users } from '@lucide/svelte';
</script>

<svelte:head><title>For DJs · Local Network</title></svelte:head>

<div class="container-page py-12">
	<div class="max-w-3xl">
		<p class="eyebrow">For DJs</p>
		<h1 class="mt-2 text-5xl">Get booked by promoters near you</h1>
		<p class="muted mt-4 text-lg">Promoters search by sound, distance, slot, room size, and budget, and your profile covers all of them. You decide what gets published.</p>
		<div class="mt-8 flex flex-wrap gap-3">
			<a href="/projects/new" class="btn-accent">List your project</a>
			<a href="/projects?unclaimed=1" class="btn-secondary">Find and claim yours</a>
		</div>
	</div>
	<div class="mt-16 grid gap-6 md:grid-cols-3">
		{#each [
			{ icon: Map, title: 'Found by distance', body: 'Set your city once. Promoters searching within 25 km of their venue see you first, and your travel radius brings in gigs further out.' },
			{ icon: Radio, title: 'Matched by sound', body: 'Genres are matched by family, so a promoter looking for house finds your deep house and afro house sets too.' },
			{ icon: CalendarHeart, title: 'Inquiries with details', body: 'Every inquiry arrives with the date, venue, capacity, budget, and slot. Reply and confirm in a private thread.' },
			{ icon: Users, title: 'Crews and agents', body: 'Up to ten managers per project. Duos, collectives, and booking agents can all answer inquiries.' },
			{ icon: Lock, title: 'Private details stay private', body: 'Your booking email, rates, and tech rider are visible only to signed-in members, stored in an atproto space.' },
			{ icon: ClipboardCheck, title: 'Reviewed changes', body: 'Moderators review every claim and edit, so nobody else can take over or deface your profile.' }
		] as item (item.title)}
			<div class="card card-pad space-y-3">
				<item.icon class="size-6 text-ansi-bright-magenta" />
				<h2 class="text-lg">{item.title}</h2>
				<p class="muted text-sm">{item.body}</p>
			</div>
		{/each}
	</div>
	<div class="card card-pad mt-6 flex flex-wrap items-center gap-4">
		<ListMusic class="size-6 shrink-0 text-ansi-bright-magenta" />
		<div class="min-w-0 flex-1">
			<h2 class="text-lg">Tracklists for your mixes</h2>
			<p class="muted text-sm">Export a mix from <a class="link" href="/tools/locallibrary">locallibrary</a> and attach it to the mix on your profile. Visitors see what you played, the tempo, the key moves, and every transition.</p>
		</div>
		<a href="/mixes/preview" class="btn-secondary">Preview an export</a>
	</div>
</div>
