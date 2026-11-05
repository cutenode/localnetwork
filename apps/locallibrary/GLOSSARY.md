# locallibrary

The command line tool, terminal player and sidecar that keep rekordbox USB drives in sync, backed
up and edited, and the plugins that add to them.

## Plugins

**Built-in plugin**:
A plugin that ships with locallibrary. Its publisher is Local Network.
_Avoid_: first-party, bundled, core plugin

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
Getting drives ready before a gig.
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
