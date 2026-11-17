<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { migrationPlan } from '#lib/testing/migration-plan.ts';
	import MigrationPlan from './MigrationPlan.svelte';

	const { Story } = defineMeta({ title: 'Staff/Project migration plan', component: MigrationPlan, tags: ['autodocs'] });

	const empty = { projects: [], pendingCreates: [], totals: { acts: 0, djs: 0, blocked: 0 } };
</script>

<Story name="Plan" args={{ plan: migrationPlan }} />

<Story name="Plan on a phone" args={{ plan: migrationPlan }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />

<Story name="No projects" args={{ plan: empty }} />
