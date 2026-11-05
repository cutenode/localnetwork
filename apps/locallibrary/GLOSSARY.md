# locallibrary

The command line tool, terminal player and sidecar that keep rekordbox USB drives in sync, backed
up and edited, and the plugins that add to them.

## Writes to drives

**Plan**:
What a command will change, worked out and shown before it changes anything, then applied exactly
as shown. Making one writes nothing to a drive, with one exception: a plan of a big write may time
a new drive, or one whose drive speed has gone stale, and says so.
_Avoid_: dry run, which is stopping at the plan

**Drive speed**:
What a drive costs to write to, measured on it: its sustained rate, what a file costs to make and
what a folder costs to make. Kept on the drive.
_Avoid_: benchmark, port speed

**Write estimate**:
How long a write to a drive will take, worked out from its drive speed and what the write asks of
the drive. A best case when the write runs past what the rate was seen to hold for.
_Avoid_: ETA, which is the time left once a write is under way

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
out removes the account.
_Avoid_: log in, log out, authenticate, connect, set keys, add credentials
