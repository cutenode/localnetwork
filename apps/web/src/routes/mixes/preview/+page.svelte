<script lang="ts">
	import { FileUp, Sparkles, X } from '@lucide/svelte';
	import MixReport from '#lib/components/mix/MixReport.svelte';
	import { parseMixReport, parseMixReportText, type MixReport as Report } from '#lib/mix/report.ts';
	import { sampleReport } from '#lib/mix/sample.ts';

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

<div class="container-page mt-6 space-y-6">
	{#if report}
		<MixReport {report}>
			{#snippet actions()}
				<button type="button" class="btn-ghost" onclick={() => (report = null)}><X class="size-4" />Close</button>
			{/snippet}
		</MixReport>
	{:else}
		<section class="card card-pad max-w-3xl space-y-4">
			<h1 class="text-3xl">Preview a mix report</h1>
			<p>
				Your tracklist, as <a class="link" href="/tools/locallibrary">locallibrary</a> found it in a recording of your set: when each track plays, at what tempo and key, and how you mixed
				between them. Open the file here to see it the way it will look on your profile. It stays on this computer; nothing is uploaded.
			</p>
			<pre class="code-block">locallibrary mix "My set.wav" --drive Main --export my-set.mix.json</pre>
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div
				class="flex flex-col items-center gap-3 border-2 border-dashed px-4 py-8 text-center {dragging ? 'border-ansi-yellow' : 'border-ansi-cyan'}"
				ondragover={(event) => {
					event.preventDefault();
					dragging = true;
				}}
				ondragleave={() => (dragging = false)}
				{ondrop}
			>
				<FileUp class="size-8 text-ansi-bright-cyan" />
				<p>Drop a <span class="text-ansi-white">.mix.json</span> file here, or</p>
				<label class="btn-secondary cursor-pointer">
					Choose a file
					<input type="file" accept=".json,application/json" class="sr-only" onchange={(event) => load(event.currentTarget.files?.[0])} />
				</label>
			</div>
			{#if error}<p class="notice notice-error" role="alert">{error}</p>{/if}
			<p class="text-sm">
				No export yet? <button type="button" class="link inline-flex items-center gap-1" onclick={loadSample}><Sparkles class="size-3" />See a sample</button>
				· To show a report on your profile, add the file to a mix when you <a class="link" href="/projects/new">list</a> or edit your project.
			</p>
		</section>
	{/if}
</div>
