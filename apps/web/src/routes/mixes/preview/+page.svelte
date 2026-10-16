<script lang="ts">
	import { FileUp, Sparkles, X } from '@lucide/svelte';
	import MixReport from '#lib/components/mix/MixReport.svelte';
	import type { MixReport as Report } from '#lib/mix/report.ts';
	import { parseMixReport, parseMixReportText } from '#lib/mix/schema.ts';
	import { sampleReport } from '#lib/mix/sample.ts';
	import Button from '#lib/ui/Button.svelte';
	import CodeBlock from '#lib/ui/CodeBlock.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';

	let report = $state<Report | null>(null);

	let error = $state<string | null>(null);

	let dragging = $state(false);

	async function load(file: File | undefined) {
		if (!file) return;
		const result = parseMixReportText(new Uint8Array(await file.arrayBuffer()));
		report = result.ok ? result.report : null;
		error = result.ok ? null : result.error;
	}

	function loadSample() {
		const result = parseMixReport(structuredClone(sampleReport));
		report = result.ok ? result.report : null;
		error = result.ok ? null : result.error;
	}

	function ondrop(event: DragEvent) {
		event.preventDefault();
		dragging = false;
		void load(event.dataTransfer?.files[0]);
	}
</script>

<svelte:head>
	<title>{report ? `${report.mix.title} · ` : ''}Mix report preview · Local Network</title>
	<meta name="description" content="See a locallibrary mix report the way it will look on your Local Network profile." />
</svelte:head>

<div class="container-page py-10 sm:py-14">
	{#if report}
		<MixReport {report}>
			{#snippet actions()}
				<Button variant="secondary" onclick={() => (report = null)}><X class="size-4" />Close</Button>
			{/snippet}
		</MixReport>
	{:else}
		<div class="flex max-w-190 flex-col gap-8">
			<SectionHeader
				level={1}
				eyebrow="Mix reports"
				tone="mid"
				title="Preview a mix report"
				lede="See your tracklist the way it will look on your profile: when each track plays, at what tempo and key, and how you mixed between them. The file stays on this computer."
			/>
			<div class="flex flex-col gap-3">
				<p class="m-0 text-fg-muted">Export one from a recording of your set with <a class="link" href="/tools/locallibrary">locallibrary</a>:</p>
				<CodeBlock language="shell" code={'$ locallibrary mix "My set.wav" --drive Main --export my-set.mix.json'} />
			</div>
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div
				class={['flex flex-col items-center gap-3 rounded-2xl border bg-well px-4 py-10 text-center transition-colors', dragging ? 'border-fg' : 'border-line-strong']}
				ondragover={(event) => {
					event.preventDefault();
					dragging = true;
				}}
				ondragleave={() => (dragging = false)}
				{ondrop}
			>
				<FileUp class="size-8 text-fg-muted" aria-hidden="true" />
				<p class="m-0">Drop a <span class="font-mono text-label">.mix.json</span> file here, or</p>
				<label class="btn-secondary cursor-pointer">
					Choose a file
					<input type="file" accept=".json,application/json" class="sr-only" onchange={(event) => load(event.currentTarget.files?.[0])} />
				</label>
			</div>
			{#if error}<p class="notice notice-error m-0" role="alert">{error}</p>{/if}
			<p class="m-0 text-small text-fg-muted">
				No export yet? <button type="button" class="link inline-flex cursor-pointer items-center gap-1" onclick={loadSample}><Sparkles class="size-3.5" />See a sample</button>. To show a report on your profile, add the file to a mix
				when you <a class="link" href="/projects/new">list</a> or edit your project.
			</p>
		</div>
	{/if}
</div>
