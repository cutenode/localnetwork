### Added

- **Plugins can write Atmosphere records you confirm.** A plugin with an
  `atmosphere:write:<nsid>` capability writes records of that type in your Atmosphere account
  through locallibrary (`atmosphere/put_record`, `Host::put_record` in the SDK). Each record is
  checked strictly against its Lexicon (locallibrary's own, else the one in the plugin's manifest,
  `lexicons`) and shown to you before anything is written; where you can't be asked, it isn't.
  Only plugins built into locallibrary write Local Network's record types.
- **`account login --scope SCOPE`** asks for another permission when signing in, such as
  `repo:NSID?action=create&action=update` for a plugin's record type. A plugin's error and a bug
  report's hint now name the scopes to ask for.
