<script lang="ts">
	import EmptyState from '#lib/components/EmptyState.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { shortDid } from '#lib/format.ts';
	import { FIELD_HOMES, type MigrationBlocker, type MigrationPlan, type PlannedPerson } from '#lib/project-migration.ts';
	import Card from '#lib/ui/Card.svelte';

	/** What moving every project to a performer would do, for staff to read before anything moves. */
	let { plan }: { plan: MigrationPlan } = $props();

	const RULES = { self: 'claimed as self', createdBy: 'created it', override: 'picked by staff', unresolved: 'not found' };

	const who = (person: { did: string | null; handle: string | null }) =>
		person.handle ? `@${person.handle}` : person.did ? shortDid(person.did) : 'nobody';

	const count = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

	function blocker(found: MigrationBlocker): string {
		switch (found.kind) {
			case 'legacyPressKit':
				return `A press kit is still in ${shortDid(found.did)}'s repo`;
			case 'unresolvedDj':
				return "No DJ found: nobody claimed it as themselves, and its creator doesn't manage it";
			case 'slugTaken':
				return `Another act already has /act/${found.slug}`;
			case 'djSpaceTaken':
				return 'The DJ already has a space of their own';
			case 'noManagers':
				return 'Nobody manages it';
		}
	}

	const roleOf = (person: PlannedPerson) => (person.relationship ? `${person.role}, claimed as ${person.relationship}` : person.role);
</script>

<section class="flex flex-col gap-3" aria-labelledby="migration-plan">
	<div class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
		<h2 id="migration-plan" class="section-title">Project migration plan</h2>
		<p class="text-label text-fg-subtle">{count(plan.totals.acts, 'act')} · {count(plan.totals.djs, 'DJ')} · {plan.totals.blocked} blocked</p>
	</div>
	<p class="text-small text-fg-muted">Nothing has moved. Follows, shows and planner saves stay in their owners' repos and get rewritten when next used.{#if plan.pendingCreates.length}{' '}{count(plan.pendingCreates.length, 'create request')} would be declined.{/if}</p>

	{#each plan.projects as project (project.uri)}
		<Card as="article" padding="sm" class="flex flex-col gap-3">
			<div class="flex flex-wrap items-center gap-2">
				<h3 class="min-w-0 font-medium [overflow-wrap:anywhere] text-fg">{project.name}</h3>
				<span class="pill bg-link/15 text-link">{project.target === 'act' ? 'Act' : 'DJ'}</span>
				<StatusPill status={project.listed ? 'listed' : 'unlisted'} />
				{#if project.blockers.length}<StatusPill status="blocked" />{/if}
			</div>
			{#if project.blockers.length}
				<ul class="flex flex-col gap-1 text-small text-critical">
					{#each project.blockers as found, index (index)}<li>{blocker(found)}</li>{/each}
				</ul>
			{/if}
			<dl class="grid gap-x-6 gap-y-1 text-small sm:grid-cols-[9rem_minmax(0,1fr)] sm:gap-y-2">
				<dt class="text-label text-fg-subtle">Redirect</dt>
				<dd class="font-mono text-label break-all max-sm:mb-2">{project.redirect.from}/** → {project.redirect.to ? `${project.redirect.to}/**` : 'nowhere yet'}</dd>
				{#if project.dj}
					<dt class="text-label text-fg-subtle">DJ</dt>
					<dd class="max-sm:mb-2">{who(project.dj)} ({RULES[project.dj.rule]})</dd>
				{/if}
				<dt class="text-label text-fg-subtle">People</dt>
				<dd class="max-sm:mb-2">{#each project.people as person, index (person.did)}{index ? ', ' : ''}{who(person)} <span class="text-fg-muted">({roleOf(person)})</span>{:else}none{/each}</dd>
				<dt class="text-label text-fg-subtle">Moves</dt>
				<dd class="max-sm:mb-2">{project.hasDetails ? 'booking details, ' : ''}{count(project.drafts.length, 'draft')}, {count(project.pressKits.length, 'press kit')}, {count(project.bookingThreads.length, 'booking thread')}</dd>
				<dt class="text-label text-fg-subtle">Left for later</dt>
				<dd class="max-sm:mb-2">{count(project.lazy.follows, 'follow')}, {count(project.lazy.upcomingShows, 'upcoming show')}</dd>
				{#if project.decline.length}
					<dt class="text-label text-fg-subtle">Declines</dt>
					<dd class="max-sm:mb-2">{project.decline.map((request) => `${request.kind} request`).join(', ')}</dd>
				{/if}
				{#if project.unmapped.length}
					<dt class="text-label text-fg-subtle">Not carried</dt>
					<dd>{#each project.unmapped as field (field.field)}<span class="block"><span class="font-mono text-label">{field.field}</span>: {field.reason}</span>{/each}</dd>
				{/if}
			</dl>
		</Card>
	{:else}
		<EmptyState title="No projects to move" body="The root's repo holds no projects." />
	{/each}

	<details class="card p-4 text-small">
		<summary class="cursor-pointer font-medium text-fg">Where each profile field goes</summary>
		<dl class="mt-3 grid grid-cols-[minmax(0,9rem)_minmax(0,1fr)] gap-x-6 gap-y-1">
			{#each Object.entries(FIELD_HOMES) as [field, home] (field)}<dt class="font-mono text-label">{field}</dt><dd class="font-mono text-label text-fg-muted">{home}</dd>{/each}
		</dl>
	</details>
</section>
