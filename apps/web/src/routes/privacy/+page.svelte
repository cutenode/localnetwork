<script lang="ts">
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<div class="container-page flex flex-col gap-8 py-10 sm:gap-10 sm:py-14">
	<SectionHeader level={1} eyebrow="Privacy" title="What we store and where" lede="Where each kind of data lives, and who can read it." />
	<div class="prose prose-site">
		<h2 id="public">Public</h2>
		<p>Published project profiles (name, genres, city, links, mixes, fee tier, managers' DIDs) are public atproto records anyone can read.</p>
		<p>A bug report you send while signed in is a public post on userinput.app, kept in your own repo, with the page you sent it from and your browser.</p>
		<h2 id="booking-details">Booking details</h2>
		<p>Private booking details (email, phone, rates, tech rider) are stored in a space only Local Network and its staff can read. The project's managers see them, and so does anyone they let into one of their press kits, inside that press kit.</p>
		<h2 id="staff-only">Staff only</h2>
		<p>Requests, claim evidence, decisions, staff roles, and the audit log live in spaces only staff can read. Audit entries record what happened (who, what, when, and which fields changed); booking message contents are never copied into the audit log.</p>
		<h2 id="press-kits">Press kits</h2>
		<p>A press kit's public part is a record in Local Network's repo that anyone can read. The rest sits in a space under Local Network's account that only the project's managers, Local Network, and the people they let in can read. Asking to see it shares your handle, and any role and message you give, with the project's managers only: requests sit in a separate space that only they and Local Network can read. Each person let in gets a record saying so in that same space, which any of the managers can have deleted to take access away. Drafts of a press kit sit in each manager's repo in a space only the project's current managers can read.</p>
		<p>When someone let in opens a press kit on this site, its project's managers see who and when. Local Network keeps that receipt for {data.retentionDays} days, and any of the managers can delete it sooner. The audit log keeps a record that the press kit was viewed, but not by whom.</p>
		<h2 id="spaces">Acts, parties and venues</h2>
		<p>Each act, party and venue has a space under Local Network's account with who's in it, their roles, invites and requests to join, and whether it's listed in the directory. Only its people are members of the space, and staff see it through this site.</p>
		<h2 id="booking-threads">Booking threads and your planner</h2>
		<p>Each booking thread is a space under Local Network's account whose only members are the promoter and the project's managers. Local Network indexes the thread so the site can show it to them. Your planner is readable only by you.</p>
		<h2 id="server">On our server</h2>
		<p>We keep a search index derived from the records above, OAuth tokens needed to act on your behalf (encrypted at rest by your host's disk encryption), hashed browser session tokens, and an outbox of audit entries waiting to be written. Signing out revokes our OAuth session.</p>
		<p>Your inbox lives only on our server. Each item says what happened and where (a new inquiry or message in one of your booking threads, say) but not what anyone wrote. Read items are deleted after 90 days and unread ones after 180.</p>
		<p>If you turn on email for your inbox, we keep the email address your account's host shares with us, to send it to, and delete it as soon as you <a href="/dashboard/account/notifications">turn email off</a>. We send one email with a link to confirm the address is yours, and nothing else until you open it. Each email goes through our email provider, Resend, and has a link that turns email off without signing in.</p>
		<p>If you <a href="/dashboard/account/notifications">turn on Bluesky DMs</a> for your inbox, they come from Local Network's own Bluesky account. Bluesky's chat service carries and keeps them like any other DM, and all we keep is that you turned them on.</p>
		<h2 id="cookies">Cookies</h2>
		<!-- Focusable so a keyboard can scroll the table sideways on a phone. -->
		<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
		<div role="region" aria-label="Cookies" tabindex="0" class="overflow-x-auto">
			<table>
				<thead><tr><th>Name</th><th>What it's for</th><th>Kept for</th></tr></thead>
				<tbody>
					<tr><td><code>ln_session</code></td><td>Keeps you signed in. We store only a hash of it, with the browser you signed in from.</td><td>{data.sessionDays} days, or until you sign out</td></tr>
					<tr><td><code>ln_locale</code></td><td>The language you see the site in.</td><td>400 days</td></tr>
					<tr><td><code>ln_shows_place</code></td><td>The city you last picked on Shows or the home page, so both open on it.</td><td>1 year</td></tr>
					<tr><td><code>ln_signed_out</code></td><td>Set when you sign out, naming the account, so the next page clears its form drafts from this browser.</td><td>Until the next page reads it</td></tr>
					<tr><td><code>ln_flag_preview</code></td><td>Staff only: the features they're previewing in this browser.</td><td>1 hour</td></tr>
				</tbody>
			</table>
		</div>
		<p>Your browser also keeps drafts of long forms, and whether keyboard shortcuts are on, in its local storage. Signing out clears that account's drafts.</p>
		<h2 id="deleting">Deleting data</h2>
		<p>Anything you wrote with your own account is in your own repo, where any atproto tool can delete it. What Local Network wrote for you when your account's server couldn't, you can delete from <a href="/dashboard/account/data">Your data</a>; audit log entries and booking status changes stay, and so do the decisions on requests you delete. Ask an admin to archive a project you manage.</p>
	</div>
</div>
