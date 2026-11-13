# Local Network

Tools for DJs and the people who book them. This glossary covers the website at localnetwork.computer.

## Site areas

**Public page**:
A page anyone can read without signing in. Viewers allowed to read more, like members or people granted a press kit, see more on it, but nobody sees controls for changing it there, apart from following, saving or saying they're going. Every profile, act, party, venue, event, set, mix, press kit, artist and track has one, and each links to the others the link matrix lists for it.
_Avoid_: profile view, read-only page

**Link matrix**:
The list of which other public pages each kind of public page links to whenever the data for that link exists.
_Avoid_: sitemap, related links

**Dashboard**:
The signed-in area where every change to what public pages show happens. There's one per account. Its sections follow who the switcher says you're acting as, and its home starts with the onboarding checklist until that's done.
_Avoid_: app, account area, my page, DJ dashboard, promoter dashboard

**Switcher**:
Choosing who you're acting as: yourself, or an act, party or venue you have capabilities for. It's the same on every page, the header always shows the choice, and the dashboard follows it.
_Avoid_: account switcher (the account doesn't change), context, workspace

**Onboarding**:
A short flow on first sign-in, then a checklist on the dashboard home with one step per piece of data and what filling it in gets you. A feature that turns on for someone adds its own steps to the checklist, marked new.
_Avoid_: setup wizard, tour, getting started

**Feed**:
The signed-in home page's list of what your network did, in date order and filtered by kind. Following shows only your network; For you adds suggestions.
_Avoid_: timeline, stream, activity (that's your own audit log)

**Area**:
A city that events are browsed by, taken from each venue's location. Everyone has a home area, from their profile or from onboarding.
_Avoid_: region, scene, location (when meaning the city)

**Private spaces**:
The four spaces staff can read from any client: audit, moderation, staff and industry. Losing a staff role means leaving all of them, and anyone left in one is a stray member until they're out.
_Avoid_: staff spaces, root spaces (those include public ones)

**Staff console**:
The separate area where staff moderate and run the site. Only staff can see it. Staff can add or remove people from any act or party, and every action they take there is logged.
_Avoid_: admin dashboard, admin panel

**Flag**:
A switch that turns a feature on for some people before everyone. Staff set who it's on for in the staff console. Flags are for rolling something out, not for keeping anyone out. Turning one off for everyone keeps who it was on for, ready for when it's back on.
_Avoid_: feature toggle, experiment, setting

**Flag group**:
A named list of accounts a flag can be turned on for. It has nothing to do with an act, and the people in it don't know they're in it.
_Avoid_: segment, cohort, group (on its own)

**Flag preview**:
A staff member seeing a flag on or off in their own browser for an hour, with nothing changing for anyone else.
_Avoid_: override, test mode

**Rollout**:
The share of signed-in accounts a flag is on for. An account stays in as the share grows, and has the same chance of being in on every flag. Signed-out visitors are never counted.
_Avoid_: percentage (in copy), canary, staged release

## People and acts

**Profile**:
A person's page on the site. Every account has exactly one, and it's who others follow. Being a DJ or a promoter is something a profile opts into, which adds to the same profile.
_Avoid_: account (when meaning what others see), user page, DJ page

**Atmosphere account**:
An AT Protocol account, at Bluesky or any other host, that someone signs in with. An act, party or venue can link one of its own.
_Avoid_: Bluesky account (it may be hosted elsewhere), PDS account

**Follow**:
Choosing to see what a profile, act, party or venue does in your feed. Each of them shows who follows it and who it follows.
_Avoid_: subscribe, like, fan (as a verb)

**Network**:
The profiles, acts, parties and venues you follow.
_Avoid_: friends, connections, following (when meaning the people)

**DJ**:
A profile with the performer sections turned on.
_Avoid_: project (when meaning the person)

**Act**:
A name DJs play under other than their own: an alias with one member, or a duo, group or collective with several. A DJ playing under their own name is just their profile. Members are public unless one chooses to hide; then only the act's other members, promoters granted its press kits, and staff can see they're in it.
_Avoid_: project, side project, artist

**Performer**:
A DJ or an act: whoever gets booked, credited, and has press kits. Copy still says "DJs".
_Avoid_: project, billing

**Credit**:
Who a set or show was played as: a DJ, an act, or several of either for a b2b. A b2b partner is only credited once they confirm it. A credit on someone else's event lineup shows on the performer's page at once, marked unconfirmed until they confirm it from their inbox, and they can remove it. A one-off b2b is only a credit; a pairing that keeps playing together can become an act.
_Avoid_: played with, lineup entry

**Section**:
One piece of what a profile, act, party or venue shows, like performer details, booking details, or the cities a promoter books in. Any of them can add any section; opting in as a DJ or promoter adds the sections that suit it.
_Avoid_: module, facet, field group

**Capability**:
One thing someone can do for a performer or a party, like editing its page, answering bookings, or checking guests in. Everything people can do for a performer or party is a capability.
_Avoid_: permission, right

**Role**:
A named starting set of capabilities, like member, manager, host, cohost or door. A person's capabilities can be adjusted after a role is given.
_Avoid_: permission level, rank

**Member**:
A DJ who plays in an act and is credited with it. Members get a set of capabilities.
_Avoid_: manager (when they also play), owner

**Manager**:
Someone who works for a performer without playing as it, such as a manager or agent. Managers get a set of capabilities, never the credit.
_Avoid_: owner, admin, agent (as a separate role)

**Promoter**:
A profile with the promoter sections turned on: the cities they book in, the parties they run, and the venues they work with. Anyone can send a booking inquiry; promoters are who press kits can be shared with as a group.
_Avoid_: booker, organizer

**Party**:
A named night or series that anyone can start. People can follow a party, and events belong to it. Its people are hosts, cohosts and door staff.
_Avoid_: series, brand, collective (collectives are acts)

**Guest**:
Someone who has RSVP'd to an event, whether going, interested, can't go, waitlisted, or waiting for approval, here or in another AT Protocol app. Going and interested show on the event and to their followers unless they hide them, and the events they went to show on their profile unless they turn that off.
_Avoid_: attendee, ticket holder

**Venue**:
A real place events happen, shared by everyone who links to it. Promoters say how they relate to each venue they work with: owner, booker, resident or regular promoter, staff, or something else in their own words. Owners are confirmed by staff. Each venue chooses whether events naming it show right away, marked unconfirmed until an owner approves, or only after approval. Owners can block profiles, acts and parties from listing events there, claiming a relationship to it, or naming it in booking inquiries.
_Avoid_: club, location (when meaning the place itself)

**Inbox**:
Where the site tells someone about guests, bookings, waitlist spots and new events, and asks them to decide things: claim and listing decisions, press kit access requests, lineup credits to confirm and community photos to approve. Every item opens what it's about. Inquiries and messages from strangers wait under its Requests tab. Each person can also opt into getting these by email or Bluesky DM.
_Avoid_: notifications page, feed

**Event**:
One night: a date, a venue, often a party, and a lineup of credits. Public events are listed and anyone can see them; private ones only open from their link. An event belongs to its party, or to the account that posted it when it has none. An event made in another AT Protocol calendar app counts when it names a venue, party or performer that's on Local Network.
_Avoid_: gig, party (the party is the series)

**Host picture**:
A picture the people running an event, party or venue post of it: a thumbnail, poster, flyer or header.
_Avoid_: banner, cover, artwork

**Community photo**:
A photo someone posts from their own account of an event, party or venue they went to. It shows, credited to them, once someone running what it's of approves it.
_Avoid_: fan photo, user upload, gallery

**Show**:
One performer's appearance on an event's lineup. What they played there, once published, is a set.
_Avoid_: gig, booking (a booking is the conversation that leads to one), set (the set is what was played)

**Plan**:
An event you're going to or interested in, or a show you're on the lineup for. Your plans are one list on the dashboard, and you can keep them in a space when you don't want them public.
_Avoid_: planner (that's where promoters work out shows), RSVP, saved show

**Booking**:
The conversation between a promoter and an act about a show, in a thread only they can read. A booking starts open, can go through negotiating, and can be confirmed. Once declined or cancelled it's settled, and nothing reopens it.
_Avoid_: gig, inquiry (the inquiry is what opens one)

**Review request**:
Asking staff to list a DJ, act or party in the directory, or to confirm someone owns a venue. Profiles go live as soon as they're saved, but the directory only shows what staff have approved.
_Avoid_: request (on its own, since the inbox's Requests tab holds inquiries and messages from strangers), proposal, submission, ticket

**Press kit**:
A performer's pitch to promoters, with a public part anyone can read and a private part only granted viewers can. A performer can have several.
_Avoid_: EPK (in copy), media kit

**Backstage**:
A performer's private area, where the people working on it keep work in progress such as drafts.
_Avoid_: workspace, managers' space, team space

**Draft**:
One person's unpublished changes to a press kit, kept in backstage and visible to the others there. Publishing applies only what it changed onto the published press kit.
_Avoid_: edit, revision, review request (those go to staff)

**Grant**:
Permission for one account to read one press kit's private part.
_Avoid_: access, approval

## Music

**Set**:
One time a DJ played, start to finish: at a show, on the radio, on a stream, or at home with nobody listening. It records when, where and the tracks in order. Most sets are never shared as mixes.
_Avoid_: mix (unless it's on a performer's page), session, performance

**Mix**:
A set a performer chose to put on their page for people to listen to, with a title and somewhere to listen. A mix always has a way to hear it.
_Avoid_: set (when meaning what's on the page), upload, track

**Recording**:
The audio of a set, as a file people can play. A listen link to Mixcloud or SoundCloud is somewhere to hear a mix, not its recording.
_Avoid_: mix (when meaning the audio), upload, file

**Artist**:
A producer credited in DJs' tracklists. Artists are not DJs or acts.
_Avoid_: DJ, act

**Track**:
A piece of music by one or more artists, as DJs' tracklists name it. Its page shows its artists, the sets that played it and the DJs who played it.
_Avoid_: song, tune, recording (a recording is a set's audio)

## Behind the site

**Root account**:
The site's own account. It holds what several people share, and writes for people who can't write for themselves.
_Avoid_: site account, service account, admin account

**Space**:
A private place for records, with a list of who can read it and who can write to it. Press kits' private parts, booking threads and backstage are all spaces.
_Avoid_: group, room, channel

**Verification**:
A record saying a profile, act, manager, party or venue is who it says it is. Without the record, nothing counts as verified.
_Avoid_: badge, blue check, claim (a claim is a review request)

**Directory**:
The list of DJs, acts and parties people browse and search, which only shows what staff have approved.
_Avoid_: index, catalogue

**Staff**:
The people who moderate and run the site. A moderator decides review requests, hides what shouldn't show and changes who's in a space. An admin can do all that, edit directly, and decide who's staff.
_Avoid_: mod, team, superuser
