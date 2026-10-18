<script lang="ts">
	import BugReportForm from '#lib/components/BugReportForm.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const step = $derived(form?.posted ? { kind: 'sent' as const, url: form.posted } : { kind: data.canPost ? ('form' as const) : ('consent' as const) });
</script>

<svelte:head><title>Report a bug · Local Network</title></svelte:head>

<div class="container-page flex max-w-3xl flex-col gap-8 py-10 sm:py-14">
	<SectionHeader level={1} eyebrow="Report a bug" title="What went wrong?" lede="It's posted from your Atmosphere account to Local Network's board on userinput.app, where anyone can read it." />
	<BugReportForm {step} github={form?.github ?? data.github} facts={data.facts} values={form?.values} error={form?.error} />
</div>
