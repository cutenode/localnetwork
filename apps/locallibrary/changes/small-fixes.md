### Fixed

- The bug report form `bug` opens stays within its length limit when the title fills it, where
  the message's field name was still added after it.
- `check` and `doctor` call an audio file cut so short it won't open truncated, as they do an
  MP3 or FLAC cut short: an M4A whose audio runs past its end, or a file smaller than the
  database's size whose headers stop partway. `--audio` said it can't be opened, and without it
  the file wasn't reported.
- The sidecar refuses an option a plugin's library source, exporter or importer doesn't declare
  (`plugins.source.plan`, `plugins.export.plan`, `plugins.import.plan`), in the words `--option`
  refuses it with, where it passed it on to the plugin.
