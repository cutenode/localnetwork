# My Settings

`locallibrary settings schema` lists all 46 settings with their values. They fall into three groups:
player (quantize, sync, auto cue, jog mode, tempo range, hot cue auto load, beat jump, vinyl
speed adjust...), display (language, LCD/jog/pad brightness, time mode, waveform colour and
position, key format...), and mixer (channel and cross fader curves, headphones, talk over, MIDI,
beat FX quantize...).

```bash
locallibrary settings import                        # start from rekordbox's own My Settings on this computer
locallibrary settings import REKORDBOX              # ...or from a drive
locallibrary settings set quantize=off jog_mode=cdj crossfader_curve=constant-power
locallibrary settings set --drive SD language=japanese
locallibrary settings show SD                       # current values and drift from the profile
locallibrary settings apply                         # every mounted drive but archive and retired ones
locallibrary settings apply Home                    # each of the library's drives plugged in
locallibrary settings publish                       # put the profile on your Atmosphere account...
locallibrary settings import --from-handle dj.example.com   # ...and bring it back on another computer, or start from someone else's
locallibrary settings import --from-account         # the account this computer is signed in to
```

`settings publish` writes the profile's base (not the per-drive overrides) to your
[Atmosphere account](account.md#publishing), where any computer can import it with `--from-handle`
without signing in (`--from-account` imports from the account that computer is signed in to).
Importing sets the values it brings and keeps the rest of the base, and the per-drive overrides;
with `--from-handle` or `--from-account`, `--replace` makes the published settings the whole base.

`apply`, `import` and `publish` show what they'd change and ask first (`--dry-run` stops there).
`apply` backs each drive's settings files up before writing them, so `locallibrary undo` takes
them back.
The profile only enforces the keys it contains, so a partial profile leaves everything else
alone. `apply` patches individual bytes in the existing `*SETTING.DAT` files and recomputes their
checksums, so bytes this tool doesn't understand (including anything newer firmware adds) are
preserved. My Settings are each drive's own: library sync and edits never touch settings files, so
per-drive overrides survive a sync, and `settings apply` writes each drive.
`settings show` lists such bytes: rekordbox 7.2 writes one more display setting
(`DEVSETTING.DAT` data byte 15) and five more mixer settings (`DJMMYSETTING.DAT` bytes 25-29)
than rekordbox 6. Profiles don't manage them until their meaning is known.
