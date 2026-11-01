<script lang="ts">
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
</script>

<svelte:head>
	<title>How it works · Local Network</title>
	<meta name="description" content="How Local Network keeps projects, requests, bookings and its audit log on the AT Protocol." />
</svelte:head>

<div class="container-page flex flex-col gap-8 py-10 sm:gap-10 sm:py-14">
	<SectionHeader level={1} eyebrow="How it works" title="Built on the AT Protocol" />
	<div class="prose prose-site">
		<p>Local Network stores its data on the <a href="https://atproto.com">AT Protocol</a>, not in a private database. Public profiles are ordinary atproto records; everything private uses <a href="https://atproto.com/blog/atproto-spaces-alpha">atproto spaces</a>, the protocol's permissioned-data extension. The app keeps a local index for fast search, and it can rebuild that index from the network.</p>

		<h2 id="projects">Projects</h2>
		<p>A <em>project</em> is a DJ act: a solo DJ, a b2b duo, a group, or a whole collective. Published projects are <code>computer.localnetwork.project</code> records in the Local Network root account's public repo. Only the root account writes them, after an admin adds a project or a moderator approves a request, so nobody can change a profile without review.</p>

		<h2 id="requests">Requests and claims</h2>
		<p>Anyone with an atproto account can list a new project, claim one an admin added, or propose changes to a project they manage. Each request is written <strong>into your own repo</strong>, inside the root account's private moderation space. Only staff can read that space. Decisions are recorded by the root account in the same space.</p>

		<h2 id="spaces">Spaces we use</h2>
		<ul>
			<li><strong>Audit log</strong>: anyone may write their own audit entries; staff read.</li>
			<li><strong>Moderation</strong>: anyone may file requests; staff read.</li>
			<li><strong>Staff</strong>: role assignments, written by the root account.</li>
			<li><strong>Industry</strong>: private booking details, readable by any signed-in member.</li>
			<li><strong>Booking threads</strong>: one space per inquiry, shared only by the promoter and the project's managers.</li>
			<li><strong>Your planner</strong>: a personal space under your own DID, like bookmarks. We keep no copy.</li>
		</ul>
		<p>Spaces provide access control, not end-to-end encryption: the services that host and sync a space (your PDS and Local Network) can read it.</p>

		<h2 id="bookings">Bookings</h2>
		<p>Sending an inquiry creates a new booking space with exactly two kinds of members: you and the project's managers. Each person's messages live in their own repo within that space.</p>

		<h2 id="audit">The audit log</h2>
		<p>Every action is recorded twice, with the same record key: once by the acting account in its own repo inside the audit space, and once by the root account. The root's copy carries a SHA-256 hash chain linking each entry to the one before it, so edits, deletions, and reordering are detectable. Staff can verify the chain and export the whole log at any time.</p>

		<h2>If your PDS doesn't support spaces yet</h2>
		<p>Spaces are new. If your PDS hasn't adopted them, you can still sign in; the root account files your requests and booking messages on your behalf, marking each record with <code>onBehalfOf</code>, and your audit trail notes that your own copy couldn't be written.</p>
	</div>
</div>
