# locallibrary

The command line tool, terminal player and sidecar that keep rekordbox USB drives in sync, backed
up and edited, and the plugins that add to them.

## Changing a library

**Plan**:
What a command is going to change, worked out and shown before anything is written. It's applied
only as shown: if the drive has moved on since, it's refused and worked out again.
_Avoid_: preview, dry run, diff

**Clone**:
A drive holding a copy of a library. Clones of one library stay alike through syncs.
_Avoid_: mirror, replica

**Lead clone**:
The clone a command given a library's name uses: the newest one plugged in, or the primary when
they're identical or can't be put in order.
_Avoid_: source drive, main drive

**Lineage**:
The states of a library a drive has held, oldest first. It's how one drive can tell it's behind
another rather than different from it.
_Avoid_: history (a drive's history is what a player recorded on it), ancestry

**Journal**:
The edits a drive remembers making, so they can be put back after rekordbox exports over them.
_Avoid_: log, edit history

## Plugins

**Built-in plugin**:
A plugin that ships with locallibrary. Its publisher is Local Network.
_Avoid_: first-party, bundled, core plugin

**Capability**:
What a plugin's manifest asks of locallibrary, such as reaching a server or hearing an event. An
account's scopes are a separate thing, given through access.
_Avoid_: permission, feature

**Guard**:
A plugin's check on a plan that edits a library, run before it's applied. It warns unless the user
makes it blocking.
_Avoid_: validator, hook, lint

**Account plugin**:
A plugin that provides one account type by describing how to sign in to it, and does nothing
else. Anyone can publish one.
_Avoid_: auth plugin, authenticator, sign-in provider

**Publisher**:
The person or organization behind a plugin, named whenever locallibrary asks you to trust it.
_Avoid_: author, developer, vendor, maintainer

**Panel**:
A section a plugin adds to a track's details.
_Avoid_: tab, widget, card

**Suggestion**:
An action a plugin proposes, offered and never applied by itself.
_Avoid_: recommendation, auto-fix, hint

## Accounts

**Account**:
One person's sign-in at another service, made through one account plugin and kept in one store.
The Atmosphere account is an account like any other.
_Avoid_: login, session, connection, linked account, identity

**Account type**:
The kind of account a plugin asks for, such as Google, SoundCloud or Atmosphere. Several account
plugins can provide the same type.
_Avoid_: service, provider, platform

**DJ account**:
The one Atmosphere account a store publishes its DJ profile, sets and settings to, and files bug
reports from. Other Atmosphere accounts in the store are only for plugins.
_Avoid_: main account, primary account, default account, your account

**Default account**:
The account a plugin uses for an account type unless a command names another. Each plugin has at
most one per type.
_Avoid_: selected account, preferred account, current account

**Scope**:
One named thing an account type lets a plugin do with an account, such as uploading videos.
_Avoid_: local scope, permission, right

**Access**:
The scopes one plugin may use on one account. The user gives it to the plugin and can take it
away without signing out; signing out leaves it, unused until the account is signed in again.
locallibrary's own features don't need it.
_Avoid_: grant (the website's word for press kits), consent, permission, authorization

**Keys**:
What some services give you to sign in to an account with instead of the browser, such as the
Internet Archive's S3 keys or a plyr.fm developer token. You enter them once, and the store keeps
them as it keeps any account's sign-in.
_Avoid_: credentials, API key, password, secret setting

**Sign in**:
To add an account, in the browser or by entering its keys, or to give one more scopes. Signing
out removes the account. The player and the docs say sign in and sign out; `login` and `logout`
stay only as the DJ account's CLI subcommand names.
_Avoid_: log in, log out, authenticate, connect, set keys, add credentials

## Drives

**Drive**:
A rekordbox USB or SD drive that locallibrary reads and writes.
_Avoid_: stick, device, volume, USB

**Snapshot**:
A saved copy of a drive's library in the store.
_Avoid_: backup, version, commit

**History**:
A play history list a player recorded.
_Avoid_: session, play log

**Prepare**:
Getting drives ready before a gig, and the player's view of those jobs.
_Avoid_: set prep, gig prep, pre-flight

## The player

**Crate**:
A working list of tracks: the CDJ's tag list, not My Tags.
_Avoid_: tag list, cart, queue

**Deck**:
The player that plays one loaded track.
_Avoid_: channel, turntable

**Pads**:
The deck's hot cue pads.
_Avoid_: hot cue buttons, performance pads

**Recording**:
The audio of a played set.
_Avoid_: mix, session, take
