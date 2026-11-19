# Plugins are published to Atmosphere and installed from a reviewed registry

The plugin registry is where people find, install and update locallibrary plugins: third-party
ones, and newer releases of the built-in ones. Publishers release plugins from their own Atmosphere
account. DJs find them on localnetwork.computer or with `locallibrary plugin search`, and install
them with `locallibrary plugin install`. v1 holds locallibrary plugins only. A release's artifact
type names the tool it's for, so another tool can join later with a type of its own. Until the
registry exists, plugins run as [plugins.md](../plugins.md) describes.

## Publishing and listing

Anyone with an Atmosphere account can publish a plugin: a release is a record in the publisher's
own repo. Review decides whether a release is listed, which means it shows in search and on the
plugin's page. Marking a release for listing files a review request in the staff inbox on its own.
A release that isn't listed, or isn't yet, still installs from its direct link
(`locallibrary plugin install at://...`), with a "not reviewed" warning.

## Installing needs an Atmosphere account

Installing or updating a plugin from the registry needs an Atmosphere account, signed in. That's
on purpose. In Tierney's words: "a core part of my personal goals with this project is to increase
Atmosphere adoption by providing good services built on top of it that aren't necessarily for
people who are already deeply invested in it". The registry is one of those services, and the
install record lives in that account.

Built-in plugins need no account: they ship with locallibrary and work signed out. Their updates
come through the registry like any plugin's, so getting one needs an account too. Signed out, a
built-in plugin stays on the release that shipped with locallibrary, and locallibrary says "sign in
to get plugin updates" when a newer one exists. Signed in, the newer compatible release runs,
whether it shipped with locallibrary or came from the registry.

## Releases

A release carries a prebuilt program for each target it supports, as one blob per target. A blob's
CID is its sha2-256 hash, so locallibrary refuses bytes that don't match the release. An account
plugin's release has no program, since an account plugin is only a manifest
([ADR 0001](0001-account-plugins-are-manifests.md)). Each artifact is at most 16 MiB in v1, the
size HappyView is proven to mirror. The limit goes up once a larger one has been tested.

Versions are semver, and a version is never reused. Prereleases (`-alpha` and `-beta`) are told
apart by their version instead of by separate channels, and locallibrary's plugin management and
the website show them with an alpha or beta badge. A plugin follows its stable releases unless you
opt in to prereleases for that plugin. Once you have, you move on to the final release when it's
out, by semver's order, and keep getting prereleases after it.

## Safety in v1

A plugin runs as you and isn't sandboxed, as now, and locallibrary says so plainly before it first
runs. v1 relies on review, on showing a plugin's capabilities, and on asking you to agree to them
before it first runs. An update that asks for more capabilities asks again. Sandboxing plugins with
the operating system is follow-up work.

Every release states the locallibrary releases and plugin protocol versions it works with: a
minimum locallibrary version and the protocol range are required, and a maximum locallibrary
version is optional. Both the release record and the plugin's `locallibrary-plugin.toml` carry
them, and they must agree. A guard in locallibrary checks them and refuses to enable a plugin that
doesn't fit, wherever it came from.

## Review

Every release is reviewed on its own, prereleases included, because any release can change what
the program does. An unreviewed prerelease installs only when you opt in, with the warning. An
approval pins the release's CID, so it covers exactly what was reviewed, and any change is a new
release with a new review. The publisher chooses per release how it goes public: automatic, where
it's listed the moment staff approve it, or managed, where it's approved and the publisher makes it
public whenever they choose, with nothing more for staff to do.

## Rollback and pinning

You can roll a plugin back to the release you had installed before, or pin it to any release that's
compatible and hasn't been withdrawn. Both go through the same guard as an install.

## Records

Releases are [software.release.*](https://release.software) records. Tierney is one of its
maintainers, and it was designed for this: "I want locallibrary plugins to be a leader in this
space. If there are gaps, we should identify and improve software.release.* rather than adapting
our own." We build on the state its authority has published (dot record keys), or on main's if
main is published first.

It doesn't yet have everything a plugin release needs, and we've sent the other maintainers
proposals for the gaps: requirements, permissions and per-release facts on the manifest, libc and
variant on a target, more statuses, project metadata, review and install records, and publishing
main's schemas. v1 doesn't depend on any of them. In Tierney's words: "Let's build for none of
these features being added, and can improve the records later if they are." What v1 needs and
software.release.* lacks goes in records of ours, each shaped like its proposal:

- The artifact type `computer.localnetwork.locallibrary.plugin` carries the plugin's
  `locallibrary-plugin.toml`, whether it's a `program` or an `account` plugin, and the fields the
  proposals would put on the manifest: requirements (the host and protocol ranges), permissions,
  licence, readme, source and provenance. The permissions must agree with the TOML, which stays
  the authority. Each release has exactly one.
- `computer.localnetwork.locallibrary.review` is the root account's review of one release, with
  where the approved bytes are mirrored and the proof of the publisher's commit.
- `computer.localnetwork.locallibrary.install` is a person's install, pinning the exact release and
  the capabilities they agreed to. It lives in a personal space rather than their public repo, so
  what someone has installed isn't public.
- `computer.localnetwork.locallibrary.about` is an optional sidecar for how a plugin is presented,
  with the same record key as its project, pointing at a site.standard.publication. The registry
  still indexes releases by the artifact type.

software.release.status isn't published yet, so a release's status is a
`computer.localnetwork.locallibrary.status` record, written by the publisher about their own
releases. It has exactly the values main's status has (active, yanked, deprecated, archived and
retracted) plus the proposed revoked and withdrawn, and main's fields plus the proposed
`supersededBy`, `withdraws` and `subjectCid`. It moves to software.release.status once that's
published. Local Network's "do not run" is a review whose decision is revoked, not a status, and
locallibrary enforces it. Linux builds are
glibc only, so no libc field is needed yet; a musl build later is told apart by its file name or a
field on our artifact type.

A program's binaries sit beside the artifact type as plain per-target blob artifacts, so any
release viewer can list and check them. locallibrary's own releases are published as
software.release records from the root account, and a plugin's host requirement points at that
project.

If a proposal is accepted, locallibrary moves to the shared field or record: it reads both for a
while, then writes only the shared one.

## Hosting

The registry runs on [HappyView v3](https://happyview.dev), as the directory does, with no separate
registry service. HappyProto's own plugin registry is a HappyView v3 instance, so indexing, the
queries, mirroring by CID and serving the bytes all run there. We add the scripts that index
releases, the queries and a copy job. The copy job keeps each release's `sync.getRecord` proof,
since HappyView checks only CIDs, and locallibrary verifies the commit signature and MST path from
it. Approved binaries are uploaded again as blobs on the root account's review record, so the
reviewed bytes outlast the publisher's PDS and the hosted HappyView, whose beta terms promise no
uptime. The root account's PDS blob limit gets checked before that's built.

There's no append-only log in v1. The review records are the history of what was approved. If the
registry is down, installed plugins keep running.

## Out of scope

Paid plugins, donations and licence keys, for now. They may come later.

## Considered options

- A separate registry service for proofs, mirroring and an append-only log: HappyView covers all of
  it but the log with small additions, and a service is one more thing to run.
- Publishing into HappyProto's registry: it indexes only HappyView's own WASM plugins.
- Our own `computer.localnetwork.plugin.*` records for releases too: a second set of release
  records beside a shared one.
- Building on the proposals, or waiting until they're accepted: the registry would depend on other
  maintainers' decisions and timing, and records written to a draft could stop validating when it
  changes.
- Review before publishing: a publisher can write any record to their own repo, so review decides
  what's listed instead.
- No installs of unreviewed releases: a publisher couldn't try or share a release before review.
- Installing without an account: works against the adoption goal, and leaves the install record
  nowhere to live.
- Updating built-in plugins without an account: updates would follow one rule for built-in
  plugins and another for the rest.
- Building plugins from source on install: every DJ's computer would need the toolchain.
- 50 MB per artifact: HappyView passes bytes through Lua as base64 under a 64 MiB limit, so it's
  untested and likely fails.
- An operating system sandbox in v1: it's follow-up work, so v1 says plainly that there's none.
- Release channels (latest, beta, alpha), as HappyProto's registry has: the version already says
  which releases are prereleases.
- Reviewing only stable releases, or only a plugin's first: any release can change what the
  program does.
- Install records in the public repo: they'd show everyone what each person runs.
- Keeping the approved bytes only in HappyView's blob store: they'd last only as long as a beta
  host does.
- Fetching the commit proof from the publisher's PDS at install: installs would fail while that
  PDS is down. Skipping it would trust the CID and the approval alone.
- An append-only log in v1: HappyView's event log is capped by retention, and the review records
  already say what was approved.

## Open questions

- Whether a labeler takes over from staff review later.
- Whether provenance (build attestations) becomes required.
- Whether reviewed releases stay installable once the publisher's account is deleted or taken
  down. That needs a legal review.
- Whether v1 requires `did:plc`, since checking key rotation depends on the PLC audit log.
- The root account's PDS blob limit.
- Which of the software.release.* proposals are accepted, and when locallibrary moves to each.
