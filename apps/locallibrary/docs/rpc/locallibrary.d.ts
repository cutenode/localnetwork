// The locallibrary RPC sidecar's types, made from schema.json by `locallibrary rpc --emit-schema`.
// Don't edit: regenerate it.

/** The protocol these types describe (`initialize` answers it). */
export const PROTOCOL = 1;

/** The signed-in Atmosphere account. */
export type AccountInfo = {
  access_expires_at: string | null;
  /** Where it signed in: the PDS, or an entryway such as `https://bsky.social`. */
  auth_server: string;
  client: Client;
  did: string;
  /** As the account's server reports it (`None` when the handle doesn't check out). */
  handle: string | null;
  /** The account's data server (PDS). */
  pds: string;
  /** What was granted. */
  scopes: string[];
  /** The latest a session can last on Bluesky's servers; sign in again before then. */
  sign_in_again_by: string;
  signed_in_at: string;
  storage: Storage;
};

export type AccountLoginCancel = {
  id: string;
};

export type AccountLoginFinish = {
  id: string;
  /** How long to wait for the browser (default 300). */
  timeout_secs?: number | null;
};

export type AccountLoginPaste = {
  id: string;
  /** The address the browser ended up on after approving. */
  url: string;
};

export type AccountLoginRequest = {
  /**
   * The URL of another client metadata document. Default: `$LOCALLIBRARY_OAUTH_CLIENT_ID`,
   * else locallibrary's own (falling back to the development client while it isn't online).
   */
  client_id?: string | null;
  /** Sign in as the `http://localhost` development client. */
  dev_client?: boolean;
  /**
   * A handle (`alice.bsky.social`), a DID, or the account's server (`https://bsky.social`).
   * Default: the handle last signed in with.
   */
  handle?: string | null;
  /**
   * Also allow publishing: writing the My Settings, profile and set records to the account
   * ([`atmosphere::PUBLISH_SCOPES`]), and nothing else.
   */
  publish?: boolean;
  /** Scopes to ask for (default: `atproto`, which identifies you and nothing more). */
  scopes?: string[];
  /**
   * Keep the session in a file only you can read instead of the system keychain. Default:
   * `$LOCALLIBRARY_SESSION_STORE=file`.
   */
  session_file?: boolean | null;
};

export type AccountStatus = {
  /** The signed-in account (`None`: not signed in). */
  account: AccountInfo | null;
  /** The handle last signed in with, to offer next time. */
  last_handle: string | null;
};

/**
 * Something a front end can offer to resolve an error: call a method, or send the request that
 * failed again with some parameters added (`method` absent), or again with administrator rights.
 */
export type Action = {
  /** Run it with administrator rights (restart the sidecar elevated first). */
  elevate?: boolean;
  /** What a button offering it says. */
  label: string;
  /**
   * The RPC method to call (`pull`, `sync.plan`). Absent: the request that failed, again,
   * with `params` added to its own.
   */
  method?: string | null;
  /**
   * The parameters: the method's, or the ones to add to the request that failed. `null`
   * means a value the user has to give (a name, a path).
   */
  params: { [key: string]: unknown };
};

export type AdoptAs = {
  drive: string;
  /** The registered drive whose identity it takes. */
  known: string;
};

/** What giving a drive back a known drive's identity would do. */
export type AdoptAsPlan = {
  drive: string;
  /** The registered drive whose identity it takes. */
  known: DriveRecord;
  root: string;
};

export type AdoptDrive = {
  drive: string;
  name?: string | null;
};

/** What adopting a drive would do: give it an identity (`.locallibrary/state.json`), or a name. */
export type AdoptPlan = {
  drive: string;
  /** Its identity, if it has one already. */
  id: string | null;
  /** The name it will have. */
  name: string;
  root: string;
};

export type AffectedPlaylist = {
  entries_removed: number;
  path: string;
};

/** Which way a sync between two diverged drives can go. */
export type AfterSet = {
  /** The drive whose changes are all player-made: it is overwritten, after being saved. */
  back_from_set: DriveChanges;
  /** The last state both held. */
  common: SnapshotInfo;
  /** Whether `left` (as given) is the drive back from the set. */
  left_is_back_from_set: boolean;
  /** The other drive (a new export, an edit...). */
  newer: DriveChanges;
};

export type Alternate = {
  file_path: string;
  name: string;
  /** The same recording (a duplicate or re-encode) rather than one sharing some material. */
  same_audio: boolean;
  track_id: number;
};

/** A track and every file of its name found for it, none picked. */
export type Ambiguous = {
  candidates: RelocateCandidate[];
  track: TrackRef;
};

export type AnalysisBenchRequest = {
  /** The datasets' audio to analyse. Without it, the drive's tracks are. */
  audio?: string | null;
  /**
   * A drive (or library) whose stored keys and tempos are scored too, and whose tracks are
   * analysed when no `audio` folder is given.
   */
  drive?: string | null;
  /**
   * Key annotations: GiantSteps Key's `annotations/key` folder (`<id>.LOFI.key` files), or the
   * GiantSteps MTG Key dataset's `annotations.txt` (its tracks with one key at confidence 2).
   */
  keys?: string | null;
  /**
   * Tempo annotations: GiantSteps Tempo's `annotations/tempo` or `annotations_v2/tempo` folder
   * (`<id>.LOFI.bpm` files).
   */
  tempos?: string | null;
};

/**
 * Tracks whose stored tempo or key the estimates question, and tracks whose level may be a
 * problem.
 */
export type AnalysisReport = {
  /** Tracks with an analysis (cached or new). */
  analyzed: number;
  /** Analysed just now (the rest came from the cache). */
  decoded: number;
  /** Tracks whose audio couldn't be analysed. */
  failed: TrackFailure[];
  key: KeyFinding[];
  loudness: LoudnessFinding[];
  /** The median integrated loudness of the analysed tracks (LUFS). */
  median_lufs: number | null;
  tempo: TempoFinding[];
};

export type AnalysisRequest = {
  drive: string;
  encoding?: WaveformEncoding;
  track: number;
  waveform?: WaveformChoice;
};

/**
 * What a track's analysis files hold, counted rather than listed (`track.analysis` and
 * `cues.list` have the full data).
 */
export type AnalysisSummary = {
  /** Whether the drive has the track's analysis files. */
  analysed: boolean;
  bars: number;
  beats: number;
  hot_cues: number;
  /** Hot cues and memory cues that are loops. */
  loops: number;
  memory_cues: number;
  mood: Mood | null;
  phrases: PhraseSummary[];
  /** The beat grid's slowest and fastest tempo. */
  tempo_range: BpmRange | null;
  /** Whether there's a scrolling (detail) waveform. */
  waveform: boolean;
};

/**
 * [`TrackAnalysis`] with only the waveforms asked for, in the encoding asked for. With the
 * defaults it's the same as [`TrackAnalysis`].
 */
export type AnalysisView = {
  beats: Beat[];
  cues: Cue[];
  detail: WaveformData | null;
  preview: WaveformData | null;
  structure: SongStructure | null;
};

/** A track's place in a set. */
export type Appearance = {
  date: string;
  /** From 1. */
  position: number;
  /** The set's id. */
  set: string;
  /** Seconds from the start of the set, when known. */
  start_seconds?: number | null;
  title?: string | null;
  /** The track as the set has it: `Artist - Title`. */
  track: string;
  venue?: string | null;
};

/** `*.apply` by the id of the plan `*.plan` returned: applies exactly that plan, once. */
export type ApplyPlan = {
  plan_id: string;
};

export type ApplyReport = {
  changes: SettingChange[];
  dry_run: boolean;
  files_created: SettingsFile[];
  files_written: SettingsFile[];
};

export type ApplySettings = {
  /** Write all four files even when the profile doesn't mention their settings. */
  create_missing?: boolean;
  drive: string;
  dry_run?: boolean;
};

export type ArcSection = {
  /**
   * Shares of sets whose closing quarter is at least 2 BPM faster, within 2 BPM, or at least
   * 2 BPM slower than their opening quarter.
   */
  builds: number;
  closing_bpm: number | null;
  /** BPM relative to each set's median, across ten stretches of a set, averaged over sets. */
  curve: (number | null)[];
  /** Median BPM over the first and last quarter of a set, averaged over sets. */
  opening_bpm: number | null;
  /** Sets long enough to have a shape (6+ tracks with a BPM). */
  sets: number;
  steady: number;
  winds_down: number;
};

export type ArcShape = "builds" | "steady" | "windsDown" | "varies";

/** One history list: the tracks a player played during one session, in order. */
export type ArchivedHistory = {
  /**
   * The database it was read from: `device_library` (`export.pdb`) or `onelibrary` (the first
   * one, when both hold it).
   */
  database: string;
  /** The earliest snapshot holding it: it was recorded before then. */
  first_seen: Sighting;
  /** Entries whose track isn't in the library any more. */
  missing: number;
  name: string;
  /** Folders and name (OneLibrary can keep history lists in folders). */
  path: string;
  /**
   * It was recorded after this: when the snapshot before `first_seen` of the same drive was
   * taken (which didn't hold it). `None` when that drive wasn't pulled before.
   */
  recorded_after: string | null;
  /** How many snapshots hold it. */
  snapshots: number;
  tracks: Track[];
};

/** A track's artwork file, for a webview to load directly. */
export type ArtworkFile = {
  mime: string;
  /** Absolute, with symlinks resolved, and inside the drive. */
  path: string;
};

export type ArtworkImage = {
  base64: string;
  mime: string;
};

export type ArtworkReport = {
  artwork_rows_added: number;
  backup: string | null;
  files_written: string[];
  fingerprint: Hash | null;
  tracks_updated: number;
};

/** One track (`track`) or several (`tracks`). */
export type ArtworkRequest = {
  drive: string;
  track: number;
} | {
  drive: string;
  tracks: number[];
};

/** What setting artwork on tracks would do. */
export type ArtworkSetPlan = {
  drive: string;
  height: number;
  image: string;
  root: string;
  tracks: TrackRef[];
  /** The image's size, before it's scaled for the players. */
  width: number;
};

/** One audio format a model plays, at these sample rates (Hz) and bit depths. */
export type Audio = {
  codec: Codec;
  depths: number[];
  inferred: boolean;
  rates: number[];
};

/** What a track's rows say about its audio file, for a track pointed at another encoding. */
export type AudioFile = {
  bit_depth: number;
  /** kbps. */
  bitrate: number;
  sample_rate: number;
  size: number;
};

/** A track whose audio file has a problem. */
export type AudioFinding = {
  problem: AudioProblem;
  track: TrackRef;
};

/** Audio files with one kind of problem. */
export type AudioIssues = {
  drive: string;
  files: AudioFinding[];
  /** `truncated`, `unreadable`, `undecodable` or `length_differs` (see [`probe::AudioProblem`]). */
  kind: string;
  level: Level;
  /** The `audio` item's message. */
  message: string;
};

/** What's wrong with an audio file. */
export type AudioProblem =
  /** It doesn't open as audio, or there's no decoder for it. */
  | {
    error: string;
    kind: "unreadable";
  }
  /**
   * Its audio stops before the length the database (or its own header) gives: a copy cut
   * short, most likely.
   */
  | {
    ends_secs: number;
    expected_secs: number;
    kind: "truncated";
  }
  /** Its last seconds don't decode. */
  | {
    error: string;
    kind: "undecodable";
  }
  /** Its header gives a different length from the database's (a replaced file?). */
  | {
    expected_secs: number;
    file_secs: number;
    kind: "length_differs";
  };

export type AudioReport = {
  /** Audio files opened. */
  checked: number;
  problems: AudioFinding[];
};

export type AutoArtworkItem = {
  /** `embedded` or the image file's key. */
  source: string;
  track: TrackRef;
};

export type AutoArtworkPlan = {
  found: AutoArtworkItem[];
  not_found: TrackRef[];
};

export type BackupInfo = {
  /**
   * What its change left in each file putting it back replaces or removes, by content (`null`:
   * absent). Missing when the change didn't finish, or an earlier version made the backup.
   */
  after?: { [key: string]: Hash | null } | null;
  /** How many of `files` are audio (removed tracks' files, which may be their only copy). */
  audio: number;
  created: string;
  files: string[];
  /** Each of `files` as it was backed up, by content. */
  hashes?: { [key: string]: Hash };
  id: string;
  reason: string;
  /**
   * Its place among the drive's backups, counting up (0: made by a version that didn't number
   * them).
   */
  seq: number;
  /**
   * When an undo took it: the backup that undo put back. Putting this one back redoes what the
   * undo took back; `undo` passes it by.
   */
  undoes?: string | null;
};

/** One setting in the settings profile's base, before and after importing. */
export type BaseChange = {
  /** `None`: not set. */
  from: string | null;
  key: string;
  to: string | null;
};

/** What the numbers describe. */
export type Basis =
  /** Tracks as played in the DJ's sets (a track played in three sets counts three times). */
  | "plays"
  /**
   * No sets declared (or the store left out): tracks as played in the sessions players
   * recorded with `min_set_tracks` or more tracks, which can be practice as well as gigs.
   */
  | "sessions"
  /**
   * Nothing played to count (no sets in the range, or no sets declared and no sessions): the
   * collection, each track once.
   */
  | "collection";

export type Beat = {
  /** Position in the bar, 1-4. */
  beat_number: number;
  bpm: number;
  time_ms: number;
};

/** Each analyzer's scores. */
export type BenchReport = {
  /** Tempo Accuracy2 (percent) by analyzer. */
  accuracy2: { [key: string]: number };
  /** Audio files that couldn't be analysed: `[name, error]`. */
  failed: [string, string][];
  /** Key reports by analyzer (`rekordbox`, `stratum-dsp`): `score` is MIREX's weighted score. */
  keys: { [key: string]: KeyOutcomeReport };
  /** Tempo reports by analyzer: `score` is Accuracy1. */
  tempos: { [key: string]: TempoOutcomeReport };
  /**
   * Annotated tracks the analysis had no audio for: not on the drive, missing from it, or not in
   * the audio folder.
   */
  unmatched: number;
};

export type BpmRange = [low: number, high: number];

export type BrokenTrack = {
  /** The database listing the track: `device_library` (`export.pdb`) or `onelibrary`. */
  database: string;
  detail: string | null;
  /** The file concerned, as a drive key. */
  path: string;
  problem: DoctorProblem;
  track: TrackRef;
};

/** `$/cancel`: stop a running request at its next safe point. */
export type CancelRequest = {
  /** The request's id (a notification, which has none, can't be cancelled). */
  id?: RequestId | null;
};

/** Whether there was something to cancel. */
export type Cancelled = {
  cancelled: boolean;
};

/** Sessions that look like one set. */
export type Candidate = {
  /** The evidence, in words. */
  reasons: string[];
  /**
   * How much it looks like a set, 0 to 100: up to 40 for its tracks, 20 for a drive taken to
   * sets, 30 for a recording that has its tracks, 10 for several drives.
   */
  score: number;
  /** Their ids, newest first. */
  sessions: string[];
  /** Different tracks the sessions logged. */
  tracks: number;
  /**
   * When it was played: the imported logs' times when there are some, else the time every
   * session's window shares.
   */
  window: Window;
};

export type CheckEntry = {
  at: string;
  ok: boolean;
  summary: string;
};

export type CheckItem = {
  /**
   * What was checked: `export`, `same_library`, `databases`, `files`, `audio`, `loudness`,
   * `large_files`, `interrupted`, `history`, `file_system`, `players` or `formats`.
   */
  check: string;
  /** The drive it's about. */
  drive: string;
  level: Level;
  message: string;
};

export type CheckReport = {
  /**
   * With [`CheckOptions::audio`]: audio files with a problem, grouped as the `audio` items
   * describe them.
   */
  audio: AudioIssues[];
  drives: string[];
  items: CheckItem[];
  /** Tracks whose level may be a problem, as the `loudness` items describe them. */
  loudness: DriveLoudness[];
  /** The player models each drive was checked against, and where they came from. */
  players: DrivePlayers[];
  problems: number;
  /** Tracks a player won't load, grouped as the `formats` items describe them. */
  unloadable: Unloadable[];
  warnings: number;
};

export type CheckRequest = {
  /**
   * Also open every audio file and decode its last seconds, to find files that are cut short
   * or don't decode. It reads a little of every file, so it takes a while on a big drive.
   */
  audio?: boolean;
  /**
   * The drives going to the set (a library's name: its mounted clones, but archive and retired
   * ones); the first is the reference the others must match.
   */
  drives: string[];
  /**
   * Player models to check the drives against (`CDJ-2000NXS2`, `CDJ-3000X`...). Without any,
   * each drive is checked against the models remembered for its library, if any.
   */
  players?: string[];
};

export type Choice = {
  id: string;
  label: string;
};

export type ClassCounts = {
  analysis: number;
  artwork: number;
  audio: number;
  database: number;
  other: number;
};

export type CleanPlan = {
  /** Total size of `files`. */
  bytes: number;
  drive: string;
  /** Files to delete (audio, analysis, artwork), as drive keys. */
  files: string[];
  fits: boolean;
  free_space: number | null;
  /** Artist/album/genre/label/key/artwork rows left unused by the removal. */
  lookup_rows: number;
  /** Space needed for the backup of the database and the deleted files. */
  needed_bytes: number;
  playlists: AffectedPlaylist[];
  root: string;
  tracks: TrackRef[];
  warnings: string[];
};

export type CleanReport = {
  backup: string | null;
  files_deleted: number;
  fingerprint: Hash | null;
  freed_bytes: number;
  tracks_removed: number;
};

export type ClearEdits = {
  /** Only entries recorded before this time. */
  before?: string | null;
  drive: string;
};

/** The recorded edits clearing the journal would forget. */
export type ClearEditsPlan = {
  drive: string;
  entries: JournalEntry[];
  root: string;
};

export type ClearPlayers = {
  /** The library (name or id) whose models to forget; without one, the default. */
  library?: string | null;
};

export type ClearedEdits = {
  removed: number;
};

/** How locallibrary introduces itself to the account's server. */
export type Client =
  /**
   * The `http://localhost` development client: nothing to host, but servers show it as a
   * local app and ask for approval at every sign-in.
   */
  | {
    kind: "loopback";
  }
  /** A client metadata document at this HTTPS URL (see [`client_metadata`]). */
  | {
    client_id: string;
    kind: "hosted";
  };

export type ClientMetadataRequest = {
  /**
   * Where the document will be hosted (an https:// URL; default: locallibrary's own,
   * [`atmosphere::CLIENT_ID`]).
   */
  client_id?: string;
  /** Scopes to allow besides `atproto` and the publish scopes, which every document allows. */
  scopes?: string[];
};

/**
 * A mounted clone of the library a plan writes, which applying the plan copies the drive it wrote
 * onto unless it's left alone (see `Planned::reaching_clones`).
 */
export type CloneTarget = {
  /**
   * Why it isn't copied onto (it's newer, diverged, or back from a set), also among the
   * plan's warnings.
   */
  left_alone?: string | null;
  name: string;
  root: string;
};

/** An audio format players list. */
export type Codec = "mp3" | "aac" | "wav" | "aiff" | "flac" | "alac";

/** What collecting recordings would copy (and, with `delete`, delete from the drive). */
export type CollectPlan = {
  bytes: number;
  /** Each is deleted from the drive once its copy is verified. */
  delete: boolean;
  drive: string;
  recordings: Recording[];
  root: string;
  to: string;
};

export type CollectRecordings = {
  /** Delete each recording from the drive once its copy is verified. */
  delete?: boolean;
  drive: string;
  /** Only these recordings (paths as [`list`] gives them) [default: all]. */
  only?: string[];
  /** Folder to copy into; a subfolder named after the drive is created. */
  to: string;
  tracklist?: Tracklist;
};

export type CollectReport = {
  bytes: number;
  collected: Collected[];
  failed: Failure[];
};

export type Collected = {
  copied_to: string;
  deleted: boolean;
  recording: string;
  tracklist: string | null;
};

/** Which `master.db` to read, and how. */
export type CollectionDiffRequest = {
  /** Default: `$LOCALLIBRARY_MASTERDB`, else rekordbox's own (see [`masterdb::default_path`]). */
  db?: string | null;
  drive: string;
  /**
   * SQLCipher passphrase. Default: `$LOCALLIBRARY_MASTERDB_KEY`, else the one every rekordbox
   * 6/7 installation uses.
   */
  key?: string | null;
  /** Only this playlist, or the playlists in this folder (and no track-level changes). */
  playlist?: string | null;
};

/** What `master.db` holds, in numbers. */
export type CollectionInfo = {
  /** Size of `master.db`, and of its WAL when there is one. */
  bytes: number;
  db_version: string | null;
  history_lists: number;
  hot_cues: number;
  loops: number;
  memory_cues: number;
  path: string;
  /** Paths of intelligent playlists (not evaluated, so listed empty). */
  smart_playlists: string[];
  summary: LibrarySummary;
  tag_categories: number;
  tagged_tracks: number;
  tags: number;
  wal_bytes: number | null;
};

/** Which `master.db` to read, and how. */
export type CollectionRef = {
  /** Default: `$LOCALLIBRARY_MASTERDB`, else rekordbox's own (see [`masterdb::default_path`]). */
  db?: string | null;
  /**
   * SQLCipher passphrase. Default: `$LOCALLIBRARY_MASTERDB_KEY`, else the one every rekordbox
   * 6/7 installation uses.
   */
  key?: string | null;
};

export type CollectionSection = {
  coloured_share: number | null;
  /**
   * Extra files of a recording already counted (the most any one library has): copies such
   * as `Track-1.mp3` left by repeated exports. `locallibrary doctor` lists them.
   */
  duplicate_files: number;
  folders: number;
  /** Tracks played in at least one set. */
  played_share: number | null;
  playlists: number;
  rated_share: number | null;
  tracks: number;
};

/**
 * Everything `collection.library` returns: counts, the library (tracks, playlists, history), cues
 * by track and My Tags.
 */
export type CollectionView = {
  cues: Record<string, never>;
  info: CollectionInfo;
  library: Library;
  tags: TagCategory[];
};

export type ComparePair = {
  left: string;
  right: string;
};

export type Comparison = {
  confidence: Confidence;
  /** From the older side to the newer one (left -> right when undetermined). */
  diff: LibraryDiff;
  evidence: Evidence[];
  left: SideSummary;
  /** Shared tracks / tracks in the smaller library. */
  overlap: number;
  relation: Relation;
  right: SideSummary;
  suggestion: Suggestion;
};

export type Compatibility = {
  /** Caveats worth knowing. */
  notes: string[];
  summary: string;
  /** Equipment that can't use it (when only some can). */
  unsupported: string[];
  verdict: Verdict;
};

export type Confidence = "low" | "medium" | "high";

/** What putting a contact on a drive would write. */
export type ContactPlan = {
  drive: string;
  file: string;
  /** The playlist made, with `playlist`. */
  playlist: string | null;
  /** The drive has one already, which is replaced. */
  replaces: boolean;
  root: string;
};

export type ContactReport = {
  file: string;
  playlist: string | null;
};

export type ConvertPlan = {
  /**
   * The drive already has that database, and it keeps what it has. The counts below are then
   * what it gains: tracks it lacks, playlists it lacks or that gain entries, folders made for
   * them, and history lists it lacks.
   */
  adds: boolean;
  /** Artwork images copied under the other database's file names. */
  artwork: number;
  drive: string;
  fits: boolean;
  folders: number;
  free_space: number | null;
  history_playlists: number;
  /** Space needed: the new database, copied artwork, and the backup of the database files. */
  needed_bytes: number;
  playlists: number;
  /**
   * The drive already has that database, and it's rebuilt, dropping what only it has (the
   * warnings name it).
   */
  replaces: boolean;
  root: string;
  /**
   * Tracks whose My Tags come along from the other database's tags (that gain some, when
   * adding).
   */
  tagged_tracks: number;
  to: Target;
  tracks: number;
  warnings: string[];
};

/** What a conversion made (added, when the drive had the database: see [`ConvertPlan::adds`]). */
export type ConvertReport = {
  artwork: number;
  backup: string | null;
  /** OneLibrary cue rows made from analysis files. */
  cues: number;
  fingerprint: Hash | null;
  history_playlists: number;
  playlists: number;
  /** Tracks with My Tags in the converted database's tags (that gained some, when adding). */
  tagged_tracks: number;
  tracks: number;
};

export type ConvertRequest = {
  drive: string;
  /**
   * Rebuild the target database if the drive already has one, dropping what only it has.
   * Otherwise it keeps what it has and gains what it lacks.
   */
  replace?: boolean;
  to: Target;
};

export type CopyReason = "new" | "changed";

/** A copy of a store: kept in the copy as `copy.json`, and in the store's `copies.json`. */
export type CopyRecord = {
  bytes: number;
  /**
   * A hash of what the copy held besides stored files (snapshots, libraries, labels, the
   * store's other records) when the latest copy finished: one that changed since was written
   * to.
   */
  contents?: Hash | null;
  /** When the latest copy finished; `None` while one runs, or after one was stopped. */
  finished: string | null;
  from: string;
  /** When a copy last finished with every snapshot: the copy holds the store as it was then. */
  last_complete: string | null;
  objects: number;
  path: string;
  problems?: string[];
  snapshots: number;
  /** When the latest copy started. */
  started: string;
  /** The store copied: its id, and where it was. */
  store: string;
};

export type CopyReport = {
  bytes_copied: number;
  /** Whether the copy now holds every snapshot. */
  complete: boolean;
  dry_run: boolean;
  objects_copied: number;
  /** Stored files the copy had already. */
  objects_present: number;
  /** With `verify`: files the copy had that had changed, copied again. */
  objects_repaired: number;
  path: string;
  /** Snapshots left out because their files are damaged or missing in the store. */
  problems: string[];
  removed_objects: number;
  /** With `mirror`: what was removed from the copy. */
  removed_snapshots: number;
  /** Snapshots the copy holds now. */
  snapshots: number;
  snapshots_copied: number;
};

/** A copy the store made, and how it stands now. */
export type CopyStatus = {
  /** Whether the copy is there now (its disk mounted, its share reachable). */
  available: boolean;
  bytes: number;
  /**
   * A hash of what the copy held besides stored files (snapshots, libraries, labels, the
   * store's other records) when the latest copy finished: one that changed since was written
   * to.
   */
  contents?: Hash | null;
  /** When the latest copy finished; `None` while one runs, or after one was stopped. */
  finished: string | null;
  from: string;
  /** When a copy last finished with every snapshot: the copy holds the store as it was then. */
  last_complete: string | null;
  objects: number;
  path: string;
  problems?: string[];
  snapshots: number;
  /** When the latest copy started. */
  started: string;
  /** The store copied: its id, and where it was. */
  store: string;
  /** Whether it holds every library's latest snapshot, when available. */
  up_to_date: boolean | null;
};

/** A count: exact, or (rounded) a round number it's at least. */
export type Count = {
  count: number;
} | {
  atLeast: number;
};

export type CoverSource = {
  kind: "embedded";
  path: string;
} | {
  kind: "file";
  path: string;
};

/** A category or tag an edit creates. */
export type Created = {
  database: Source;
  /** `Category` or `Category/Tag`. */
  path: string;
};

export type Cue = {
  color: number[] | null;
  /**
   * A memory cue's or loop's colour: a row of rekordbox's colour table (1 pink ... 8 purple,
   * as [`crate::model::TrackColor`]), 0 for none. Only the extended list has it.
   */
  color_id: number;
  comment: string;
  /** End of the loop, for loops. */
  end_ms: number | null;
  /** `None` for memory cues; `Some(0)` = hot cue A, `Some(1)` = B, ... */
  hot_cue: number | null;
  /**
   * A loop's length in beats as rekordbox stored it, numerator and denominator (4/1, 1/2),
   * when it was set on the grid. Only the extended list has it.
   */
  loop_beats: LoopBeats | null;
  start_ms: number;
};

/**
 * A cue edit as the drive's journal records it (see [`crate::edit::journal`]): a copy's source
 * by its audio file (a device path), like the tracks edited, so a replay on a later export, where
 * ids differ, finds them.
 */
export type CueChange = {
  set: SetCue;
} | {
  update: UpdateCue;
} | {
  delete: CueRef;
} | {
  copy: {
    from: string;
  };
} | {
  carry: {
    from: string;
    shift_ms: number;
  };
};

/** A cue as shown to people. */
export type CueInfo = {
  active_loop: boolean;
  /** The beat of the grid the cue sits on (counting from 1), if it's on one. */
  beat: number | null;
  /** `#rrggbb` for a hot cue with a palette colour, a colour name (`red`) for a memory cue. */
  color: string | null;
  /** Where the loop ends, for loops. */
  end_ms: number | null;
  /** `A`-`H`; `None` for memory cues. */
  hot_cue: string | null;
  /** A quantised loop's length in beats: `"8"`, `"1/2"`. */
  loop_beats: string | null;
  name: string;
  start_ms: number;
};

export type CueMark = {
  /** `#rrggbb`. */
  color: string | null;
  /** The hot cue's letter and name: `A`, `B · Drop` (the HTML report gives only the letter). */
  label: string;
  /** Seconds into the track. */
  t: number;
};

/**
 * What to do to the selected tracks' cues, tagged by `op`:
 * `{"op": "set", "hot_cue": "B", "at": {"ms": 62500}, "loop_beats": 8, "name": "Drop", "color": "red"}`,
 * `{"op": "update", "cue": {"hot": "B"}, "name": "Drop"}`, `{"op": "delete", "cue": "all"}`,
 * `{"op": "copy", "from": {"ids": [12]}}`.
 */
export type CueOp =
  | (SetCue & {
    op: "set";
  })
  | (UpdateCue & {
    op: "update";
  })
  | {
    cue: CueRef;
    op: "delete";
  }
  /** Replace the selected tracks' cues with a copy of one other track's. */
  | {
    from: TrackSelector;
    op: "copy";
  };

export type CuePlan = {
  drive: string;
  fits: boolean;
  free_space: number | null;
  /** Space needed for the backup and the rewritten files. */
  needed_bytes: number;
  root: string;
  /** Tracks whose cues change. */
  tracks: TrackCueChange[];
  warnings: string[];
};

/** A position in a track. */
export type CuePosition =
  /** Milliseconds from the start. */
  | {
    ms: number;
  }
  /** A beat of the track's beat grid, counting from 1. */
  | {
    beat: number;
  };

/** Which existing cues an edit is about. */
export type CueRef =
  /** `{"hot": "B"}` */
  | {
    hot: HotCue;
  }
  /** `{"memory": {"ms": 62500}}`: the memory cue nearest the position (within 0.1 s). */
  | {
    memory: CuePosition;
  }
  /** Every cue. */
  | "all";

export type CueReport = {
  backup: string | null;
  files_written: string[];
  fingerprint: Hash | null;
  onelibrary: CueRowChanges | null;
  tracks_changed: number;
};

/** What [`OneLibrary::replace_cues`] did. */
export type CueRowChanges = {
  added: number;
  /** Hot cue bank list entries that pointed at removed cues. */
  bank_list_entries_removed: number;
  removed: number;
  updated: number;
};

/** The name and colour code in a OneLibrary database. */
export type DbIdentity = {
  /** rekordbox's background colour code (0: none). */
  colour: number | null;
  name: string | null;
};

export type DecadeCount = [decade: number, count: number];

export type DedupeRequest = {
  /**
   * Also find copies by their sound: the same recording in another file or encoding (see
   * [`crate::doctor`]).
   */
  by_sound?: boolean;
  drive: string;
  /** Also find copies by audio content (see [`crate::doctor`]). */
  hash_audio?: boolean;
  /**
   * Also replace tracks that only share artist, title and duration (they may be different
   * edits of a recording).
   */
  metadata?: boolean;
};

export type DeleteHistoryRequest = {
  drive: string;
  /** Delete lists the store hasn't kept (`locallibrary pull` keeps them). */
  force?: boolean;
  /** History lists by name; all of them when empty. */
  names?: string[];
};

/** `rpc.describe`. */
export type DescribeRequest = {
  /** One method (or an alias of one); default every method. */
  method?: string | null;
};

/** Quick, hash-free description of a drive, for listings and UIs. */
export type DeviceInfo = {
  /** How `export.pdb` and OneLibrary differ, on a drive that has both (empty when they agree). */
  drift: LibraryDrift | null;
  exported_at: string | null;
  free_space: number | null;
  /** `export.pdb` present (the Device Library every player reads, except the newest). */
  has_device_library: boolean;
  /** Either database below. */
  has_export: boolean;
  /**
   * OneLibrary `exportLibrary.db` present (the only database CDJ-3000X, OPUS-QUAD, XDJ-AZ...
   * read).
   */
  has_library_plus: boolean;
  id: string | null;
  last_known: LineageEntry | null;
  library_id: string | null;
  lineage_len: number;
  name: string | null;
  pdb: PdbHeader | null;
  pending: PendingOp | null;
  /** rekordbox's automatic playlist sync to this drive, per database. */
  rekordbox_sync: RekordboxSync[];
  root: string;
  settings_files: string[];
  summary: LibrarySummary | null;
  total_space: number | null;
  volume_name: string;
};

export type DiffEntry = {
  position: number;
  track: TrackRef;
};

export type DiffRequest = {
  /**
   * A library (its newest drive plugged in, else its latest snapshot), a drive, or
   * `snapshot:<id prefix>`.
   */
  left: string;
  /** Only this playlist, or the playlists in this folder. */
  playlist?: string | null;
  right: string;
};

export type Direction =
  /** Copy from whichever side is newer (refuses when that can't be determined). */
  | "auto"
  /** Copy left -> right regardless. */
  | "left_to_right"
  /** Copy right -> left regardless. */
  | "right_to_left";

export type Disk = {
  /** "USB", "SD"... */
  bus: string | null;
  /** Whole-disk device path. */
  device: string;
  /** OS identifier: `disk4` (macOS), `/dev/sdb` (Linux), `PhysicalDrive2` (Windows). */
  id: string;
  internal: boolean;
  model: string;
  read_only: boolean;
  /** External, removable or hot-pluggable. */
  removable: boolean;
  /** `None` when the disk has no partition table. */
  scheme: Scheme | null;
  sector_size: number;
  serial?: string | null;
  size: number;
  /** Holds the running operating system. */
  system: boolean;
  /** A disk image, loop device or virtual disk. */
  virtual_disk: boolean;
  volumes: Volume[];
};

/** A disk, how players would get on with it as it is, and whether it can be formatted. */
export type DiskView = {
  compatibility: Compatibility;
  disk: Disk;
  identity: string;
  /** Why it can't be formatted (`None`: it can). */
  problem: string | null;
};

export type DismissSessions = {
  /** Ids (any unique prefix of 4 or more characters) or list names. */
  sessions: string[];
  /** Take the dismissal back. */
  undo?: boolean;
};

export type DocArc = {
  builds: number;
  shape: ArcShape;
  steady: number;
  windsDown: number;
};

export type DocCollection = {
  playlists: Count;
  tracks: Count;
};

export type DocEnergy = {
  high: number;
  low: number;
  mid: number;
};

export type DocEra = {
  medianYear?: number | null;
  recent?: number | null;
};

export type DocKeys = {
  minor?: number | null;
  top: string[];
};

export type DocMixing = {
  genreChanges?: number | null;
  harmonic?: number | null;
  /** Transitions that change tempo by 3% or less. */
  smoothTempo?: number | null;
};

export type DocPrep = {
  hotCues: number;
  loops: number;
  memoryCues: number;
};

/** A count: exact, or (rounded) a round number it's at least. */
export type DocSets = {
  medianTracks: number;
  /** Year of the oldest set. */
  since?: number | null;
} & ({
  count: number;
} | {
  atLeast: number;
});

export type DocShare = {
  name: string;
  share: number;
};

export type DocTempo = {
  high: number;
  low: number;
  median: number;
};

export type DocTrack = {
  artist: string;
  title: string;
};

export type DoctorProblem =
  /** The audio file isn't on the drive. */
  | "missing_audio"
  /** The database's path for it leaves the drive. */
  | "unsafe_path"
  /**
   * The audio file is smaller than the database recorded, and its audio stops before its
   * length: a copy cut short.
   */
  | "truncated"
  /** The analysis file (`.DAT`: beat grid, cues, waveform) isn't on the drive. */
  | "missing_analysis"
  /** The artwork image isn't on the drive. */
  | "missing_artwork";

export type DoctorReport = {
  /**
   * What an analysis of the audio says about stored tempos, keys and levels (with
   * [`DoctorOptions::analyze`]). Opinions to look into, not problems.
   */
  analysis: AnalysisReport | null;
  /** Audio files cut short or that don't decode (with [`DoctorOptions::audio`]). */
  audio: AudioReport | null;
  /** How players get on with the drive's disk, when it is a mounted volume's root. */
  compatibility: Compatibility | null;
  /** How `export.pdb` and OneLibrary differ (`None` also on a drive with only one of them). */
  drift: LibraryDrift | null;
  drive: string;
  /** Each section is `None` when it wasn't run (see [`DoctorOptions::only`]). */
  duplicates: DuplicateGroup[] | null;
  /** The volume's file system as the OS names it. */
  file_system: string | null;
  large_files: LargeFile[] | null;
  missing: BrokenTrack[] | null;
  /** `None` also when OneLibrary can't be read (then what it references is unknown). */
  orphans: OrphanFile[] | null;
  root: string;
  tracks: number;
  /** Tracks in no playlist. */
  unfiled: TrackRef[] | null;
  warnings: string[];
};

export type DoctorRequest = {
  /** Also run [`Section::Analysis`]. */
  analyze?: boolean;
  /** Also run [`Section::Audio`]. */
  audio?: boolean;
  /**
   * Also find the same recording in other files and encodings (a FLAC and an MP3 of it) by its
   * sound: every track is fingerprinted (once: the store keeps the prints) and tracks of about
   * the same length are compared. Decoding a library's audio the first time takes a while.
   */
  by_sound?: boolean;
  drive: string;
  /**
   * Also find duplicates by audio content. Only files whose size matches another track's are
   * hashed, through the drive's hash cache.
   */
  hash_audio?: boolean;
  /** Sections to run (all but [`Section::Audio`] when empty). */
  only?: DoctorSection[];
  /**
   * Duplicates only by file, audio, sound or Sync Manager copy: leave out tracks that only share
   * artist, title and length (they may be different edits), also from groups the others join.
   */
  sure_only?: boolean;
};

export type DoctorSection =
  | ("duplicates" | "missing" | "orphans" | "unfiled" | "drift" | "large_files" | "compatibility")
  /**
   * Audio files cut short or that don't decode. Only when asked for (`audio`, or `only`
   * naming it): it reads every file's last seconds.
   */
  | "audio"
  /**
   * Stored tempos and keys an analysis of the audio disagrees with, and tracks too loud,
   * clipped or far from the library's level. Only when asked for (`analyze`, or `only` naming
   * it): it decodes every track not analysed before.
   */
  | "analysis";

/** How a drive's current settings deviate from what the profile wants. */
export type Drift = {
  actual: SettingValue | null;
  expected: string;
  key: string;
  /** Whether the expectation comes from a per-drive override. */
  overridden: boolean;
};

export type DriveBackups = {
  /** Newest first. */
  backups: UndoableBackup[];
  drive: string;
  root: string;
};

/** What changed on a drive (see [`WatchEvent::Changed`]). */
export type DriveChange =
  /** Its database files: an export, an edit, a sync, or a player recording history. */
  | "databases"
  /** Its `.locallibrary` state: its identity, name, library or lineage. */
  | "state";

/** What changed on one drive since the last state both drives held. */
export type DriveChanges = {
  /**
   * Analysis files rewritten for tracks that were already there (memory cues saved on a
   * player).
   */
  cue_files: number;
  drive: string;
  /** History lists added (players add one per session). */
  history_lists: string[];
  /** Changes players don't make: new or removed tracks, edited playlists, new audio... */
  other: string[];
  /** Playlists a CDJ-3000X makes from its Tag List. */
  tag_lists: string[];
};

/**
 * [`Collection::diff_drive`]: `diff.added_*` is what the drive lacks, `diff.removed_*` what only
 * the drive has, and changed tracks are details the collection has since changed (or the drive
 * was edited).
 */
export type DriveDiff = {
  diff: PlaylistDiff;
  /** Found by ISRC, or artist, title and length. */
  matched_by_details: number;
  /** Collection tracks found on the drive by their file. */
  matched_by_file: number;
  /**
   * Collection tracks only a loose match (title alone, or similar words) finds on the drive;
   * counted as missing from it.
   */
  uncertain: TrackRef[];
};

export type DriveFit = {
  capacity: number;
  fits: boolean;
  mounted: boolean;
  name: string;
  /** The library plus the players' headroom. */
  needed: number;
};

export type DriveHistory = {
  drive: string;
  lists: HistoryList[];
  warnings: string[];
};

/** A drive's logbook, kept in the store's registry. */
export type DriveLog = {
  /** Bytes written to it by sync, push and repair. */
  bytes_written: number;
  clean_ejects: number;
  first_seen?: string | null;
  last_fs_check?: CheckEntry | null;
  last_scrub?: CheckEntry | null;
  /** Newest last, at most 50 (the oldest are dropped). */
  problems?: FleetProblem[];
  /** No longer in use: hidden from listings, refused as a sync target. */
  retired?: string | null;
  /** Operations that wrote to it. */
  writes: number;
};

/** One drive's tracks whose level may be a problem. */
export type DriveLoudness = {
  drive: string;
  tracks: LoudnessFinding[];
};

export type DriveOverrides = {
  /** Drive name when the override was last edited (for display only). */
  name: string;
  values: { [key: string]: string };
};

export type DrivePlayers = {
  drive: string;
  from: PlayersFrom;
  /** The library they're remembered for (with `from: library`). */
  library?: string | null;
  models: string[];
};

export type DriveRecord = {
  /** Size of its volume when last seen. */
  capacity?: number | null;
  /** The disk it was last seen on: hints for spotting copies and drives erased elsewhere. */
  hardware?: Hardware | null;
  id: string;
  last_fingerprint: Hash | null;
  last_mount: string;
  last_seen: string;
  library_id: string | null;
  log: DriveLog;
  name: string;
  /** What the drive is for (see [`crate::fleet`]). */
  role?: Role | null;
};

export type DriveRef = {
  drive: string;
};

/**
 * A drive group: the mounted drives that share a library and can be synced together (not
 * archive or retired), plus the library's primary if it's mounted.
 */
export type DriveSet = {
  /** Mounted members left out: archive or retired. */
  excluded: string[];
  library: string | null;
  library_id: string | null;
  members: string[];
  primary: string | null;
};

export type DriveSettingsPlan = {
  changes: ApplyReport;
  drive: string;
  root: string;
};

/** One drive's part of a settings write: its report, or why it failed (the others carry on). */
export type DriveSettingsResult = {
  drive: string;
  error: TargetError | null;
  report: ApplyReport | null;
  root: string;
};

export type DriveSettingsView = {
  current: SettingsSnapshot;
  drift: Drift[];
  drive: string;
  id: string | null;
};

/** One drive's standing relative to its library's latest snapshot. */
export type DriveStatus = {
  /**
   * Back from a set: the drive is newer than the head only by what players add (history
   * lists, TAG LIST playlists), with this many history lists new or longer. Pulling saves them.
   */
  back_from_set: number | null;
  drive: DeviceInfo;
  error: string | null;
  head: SnapshotInfo | null;
  library: string | null;
  /**
   * The library head (left) vs. the drive (right): `identical`, `left_newer` (the drive is
   * behind: push), `right_newer` (the drive has newer changes: pull them), `diverged`... `None`
   * if not linked to a library.
   */
  relation: Relation | null;
  /** Settings that differ from the profile. */
  settings_drift: number;
};

/** Mounted drives sharing one identity. */
export type Duplicate = {
  id: string;
  name: string;
  /** The one on the disk the identity was given on, when the hardware says. */
  original: string | null;
  roots: string[];
};

export type DuplicateGroup = {
  /**
   * Id of the suggested keeper: the best audio (lossless over lossy, then the higher bitrate,
   * sample rate and bit depth), then the most cues, playlists and history lists, then the
   * earliest date added.
   */
  keep: number;
  /** What put the keeper ahead of the next copy (`lossless over lossy`, `320 kbps over 192`). */
  keep_reason: string;
  reasons: DuplicateReason[];
  /** Suggested keeper first. */
  tracks: DuplicateTrack[];
};

/** Why tracks are considered the same recording. */
export type DuplicateReason =
  /** Several database rows for one audio file. */
  | "same_file"
  /** Different files with identical audio (with `hash_audio`). */
  | "same_audio"
  /**
   * The same recording in another file or encoding (with `by_sound`): their fingerprints line
   * up over most of both, and their lengths agree (an extended mix and its radio edit share
   * most of their audio, but not their length).
   */
  | "same_recording"
  /** rekordbox Sync Manager copies: `Name-1.mp3` or `Name (1).mp3` next to `Name.mp3`, same size. */
  | "sync_manager_copy"
  /** Same artist, title and duration. */
  | "same_metadata";

export type DuplicateTrack = {
  bit_depth: number;
  bitrate: number;
  /** Hot cues, memory cues and loops in its analysis files. */
  cues: number;
  date_added: string;
  file_size: number;
  /** History lists (what players recorded) the track is in. */
  history: number;
  /** FLAC, WAV, AIFF or Apple Lossless. */
  lossless: boolean;
  play_count: number;
  /** Playlists the track is in. */
  playlists: number;
  sample_rate: number;
  track: TrackRef;
};

/**
 * One change to a drive's library. Playlists are given as paths (`Folder/Name`) or unique
 * names; positions in a playlist count from 1.
 */
export type Edit =
  /**
   * Create a playlist (or folder) at `path`, creating missing folders above it. Nothing
   * happens if it exists already.
   */
  | {
    folder?: boolean;
    op: "create_playlist";
    path: string;
  }
  | {
    name: string;
    op: "rename_playlist";
    playlist: string;
  }
  /**
   * Move a playlist or folder into folder `into` (the top level when absent or empty), at
   * `position` among its new siblings (the end when absent).
   */
  | {
    into?: string | null;
    op: "move_playlist";
    playlist: string;
    position?: number | null;
  }
  /**
   * Delete a playlist, or a folder with everything in it. Tracks stay in the library unless
   * `with_tracks`: then tracks no playlist outside it (and no history list) uses go too, with
   * their files, as when removing a guest's crate after a back-to-back set.
   */
  | {
    op: "delete_playlist";
    playlist: string;
    with_tracks?: boolean;
  }
  /**
   * Add tracks at `position` (the end when absent). Tracks already in the playlist are left
   * out unless `allow_repeats`.
   */
  | {
    allow_repeats?: boolean;
    op: "add_tracks";
    playlist: string;
    position?: number | null;
    /** Order the added tracks (see [`query::parse_sort`]). */
    sort?: string | null;
    tracks: TrackSelector;
  }
  /** Remove every entry of the selected tracks and/or the entries at `positions`. */
  | {
    op: "remove_tracks";
    playlist: string;
    positions?: number[];
    tracks?: TrackSelector | null;
  }
  /** Move the entries at `positions` (kept in their order) so the first lands at `to`. */
  | {
    op: "move_tracks";
    playlist: string;
    positions: number[];
    to: number;
  }
  /**
   * Make a playlist hold exactly the selected tracks (a query or filter may select none),
   * optionally sorted and limited. With `create`, the playlist at exactly that path, made if
   * missing. Rule-based playlists use this.
   */
  | {
    create?: boolean;
    limit?: number | null;
    op: "set_tracks";
    playlist: string;
    sort?: string | null;
    tracks: TrackSelector;
  }
  /**
   * Make a playlist's entries exactly these audio files (device paths) in this order, repeats
   * and all; none empties it. With `create`, the playlist at exactly that path, made (with
   * missing folders above it) if missing. Imported tracklists are made this way. The journal
   * records these edits this way: filling a playlist (`SetTracks`), ordering it for mixing
   * (`PlanSet`), and removing or moving entries whose files have other entries in it too.
   */
  | {
    create?: boolean;
    op: "set_entries";
    paths: string[];
    playlist: string;
  }
  /** Reorder a playlist (see [`query::parse_sort`]). */
  | {
    by: string;
    op: "sort_playlist";
    playlist: string;
  }
  /**
   * Drop repeated entries (the first stays); with `recordings`, also other copies of the same
   * recording (artist, title and duration).
   */
  | {
    op: "dedupe";
    playlist: string;
    recordings?: boolean;
  }
  /**
   * Order a playlist for mixing (see [`harmony::order_set`], with how sure
   * each key is from the analyses doctor has cached, see [`crate::sound::key_confidences`]): in
   * place, or into a new playlist at `into`.
   */
  | {
    into?: string | null;
    op: "plan_set";
    playlist: string;
    start?: TrackSelector | null;
  }
  | {
    fields: TrackFields;
    op: "edit_tracks";
    tracks: TrackSelector;
  }
  /**
   * Delete tracks with their playlist and history entries, and (unless `keep_files`) their
   * audio, analysis and artwork files when no other track uses them.
   */
  | {
    keep_files?: boolean;
    op: "delete_tracks";
    tracks: TrackSelector;
  }
  /**
   * Replace one copy of a recording with another: playlist and history entries (and My Tags)
   * move to `keep`, then `duplicate` is deleted unless `keep_duplicate`.
   */
  | {
    duplicate: TrackSelector;
    keep: TrackSelector;
    keep_duplicate?: boolean;
    op: "replace_track";
  }
  /**
   * Point one track at another audio file on the drive (a device path), as when its file was
   * moved or renamed: in both databases and in the analysis files that record it. With `file`,
   * the file is another encoding of the recording (a lossless copy of an MP3): the rows take
   * its format, size, sample rate, bit depth and bit rate, and with `shift_ms` its cues and
   * beat grid move by that much, for audio that starts at another point.
   */
  | {
    file?: AudioFile | null;
    op: "set_path";
    path: string;
    shift_ms?: number;
    track: TrackSelector;
  }
  /** Delete history lists (what players recorded) by name, or all of them. */
  | {
    all?: boolean;
    names?: string[];
    op: "delete_history";
  }
  /**
   * Delete history lists, each the one with exactly its name and entries. The journal records
   * deleting history lists this way, so a replay leaves lists players recorded since, under a
   * reused name too.
   */
  | {
    lists: HistoryListName[];
    op: "delete_history_lists";
  }
  /**
   * A My Tag edit (see [`crate::tags`]), its tracks named by file. `tags` edits are recorded
   * this way; only a replay takes them.
   */
  | (TagChange & {
    op: "my_tag";
  })
  /**
   * A cue edit (see [`crate::cue_edit`]) on the tracks of these audio files (device paths).
   * `cues` edits are recorded this way; only a replay takes them.
   */
  | {
    change: CueChange;
    op: "cues";
    tracks: string[];
  };

/**
 * Which tracks an operation targets: the tracks that fit every field given, so a playlist and a
 * filter together pick the playlist's tracks the filter matches. At least one field must be set,
 * and a blank query, filter, album, playlist or history list is refused rather than taken to
 * mean every track.
 */
export type EditCues = {
  /** Every track whose album has this name. */
  album?: string | null;
  drive: string;
  edit: CueOp;
  /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
  filter?: string | null;
  /** The tracks of a history list (what a player recorded), by name. */
  history?: string | null;
  ids?: number[];
  /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
  paths?: string[];
  playlist?: string | null;
  /** Matches "artist - title" or path substrings; must match at least one track. */
  query?: string | null;
};

/**
 * One change to a drive's library. Playlists are given as paths (`Folder/Name`) or unique
 * names; positions in a playlist count from 1.
 */
export type EditOutput =
  /**
   * Create a playlist (or folder) at `path`, creating missing folders above it. Nothing
   * happens if it exists already.
   */
  | {
    folder: boolean;
    op: "create_playlist";
    path: string;
  }
  | {
    name: string;
    op: "rename_playlist";
    playlist: string;
  }
  /**
   * Move a playlist or folder into folder `into` (the top level when absent or empty), at
   * `position` among its new siblings (the end when absent).
   */
  | {
    into: string | null;
    op: "move_playlist";
    playlist: string;
    position: number | null;
  }
  /**
   * Delete a playlist, or a folder with everything in it. Tracks stay in the library unless
   * `with_tracks`: then tracks no playlist outside it (and no history list) uses go too, with
   * their files, as when removing a guest's crate after a back-to-back set.
   */
  | {
    op: "delete_playlist";
    playlist: string;
    with_tracks?: boolean;
  }
  /**
   * Add tracks at `position` (the end when absent). Tracks already in the playlist are left
   * out unless `allow_repeats`.
   */
  | {
    allow_repeats: boolean;
    op: "add_tracks";
    playlist: string;
    position: number | null;
    /** Order the added tracks (see [`query::parse_sort`]). */
    sort: string | null;
    tracks: TrackSelector;
  }
  /** Remove every entry of the selected tracks and/or the entries at `positions`. */
  | {
    op: "remove_tracks";
    playlist: string;
    positions: number[];
    tracks: TrackSelector | null;
  }
  /** Move the entries at `positions` (kept in their order) so the first lands at `to`. */
  | {
    op: "move_tracks";
    playlist: string;
    positions: number[];
    to: number;
  }
  /**
   * Make a playlist hold exactly the selected tracks (a query or filter may select none),
   * optionally sorted and limited. With `create`, the playlist at exactly that path, made if
   * missing. Rule-based playlists use this.
   */
  | {
    create: boolean;
    limit: number | null;
    op: "set_tracks";
    playlist: string;
    sort: string | null;
    tracks: TrackSelector;
  }
  /**
   * Make a playlist's entries exactly these audio files (device paths) in this order, repeats
   * and all; none empties it. With `create`, the playlist at exactly that path, made (with
   * missing folders above it) if missing. Imported tracklists are made this way. The journal
   * records these edits this way: filling a playlist (`SetTracks`), ordering it for mixing
   * (`PlanSet`), and removing or moving entries whose files have other entries in it too.
   */
  | {
    create?: boolean;
    op: "set_entries";
    paths: string[];
    playlist: string;
  }
  /** Reorder a playlist (see [`query::parse_sort`]). */
  | {
    by: string;
    op: "sort_playlist";
    playlist: string;
  }
  /**
   * Drop repeated entries (the first stays); with `recordings`, also other copies of the same
   * recording (artist, title and duration).
   */
  | {
    op: "dedupe";
    playlist: string;
    recordings: boolean;
  }
  /**
   * Order a playlist for mixing (see [`harmony::order_set`], with how sure
   * each key is from the analyses doctor has cached, see [`crate::sound::key_confidences`]): in
   * place, or into a new playlist at `into`.
   */
  | {
    into: string | null;
    op: "plan_set";
    playlist: string;
    start: TrackSelector | null;
  }
  | {
    fields: TrackFields;
    op: "edit_tracks";
    tracks: TrackSelector;
  }
  /**
   * Delete tracks with their playlist and history entries, and (unless `keep_files`) their
   * audio, analysis and artwork files when no other track uses them.
   */
  | {
    keep_files: boolean;
    op: "delete_tracks";
    tracks: TrackSelector;
  }
  /**
   * Replace one copy of a recording with another: playlist and history entries (and My Tags)
   * move to `keep`, then `duplicate` is deleted unless `keep_duplicate`.
   */
  | {
    duplicate: TrackSelector;
    keep: TrackSelector;
    keep_duplicate: boolean;
    op: "replace_track";
  }
  /**
   * Point one track at another audio file on the drive (a device path), as when its file was
   * moved or renamed: in both databases and in the analysis files that record it. With `file`,
   * the file is another encoding of the recording (a lossless copy of an MP3): the rows take
   * its format, size, sample rate, bit depth and bit rate, and with `shift_ms` its cues and
   * beat grid move by that much, for audio that starts at another point.
   */
  | {
    file?: AudioFile | null;
    op: "set_path";
    path: string;
    shift_ms?: number;
    track: TrackSelector;
  }
  /** Delete history lists (what players recorded) by name, or all of them. */
  | {
    all: boolean;
    names: string[];
    op: "delete_history";
  }
  /**
   * Delete history lists, each the one with exactly its name and entries. The journal records
   * deleting history lists this way, so a replay leaves lists players recorded since, under a
   * reused name too.
   */
  | {
    lists: HistoryListName[];
    op: "delete_history_lists";
  }
  /**
   * A My Tag edit (see [`crate::tags`]), its tracks named by file. `tags` edits are recorded
   * this way; only a replay takes them.
   */
  | (TagChange & {
    op: "my_tag";
  })
  /**
   * A cue edit (see [`crate::cue_edit`]) on the tracks of these audio files (device paths).
   * `cues` edits are recorded this way; only a replay takes them.
   */
  | {
    change: CueChange;
    op: "cues";
    tracks: string[];
  };

export type EditPlan = {
  bytes: number;
  /** One line per edit. */
  changes: string[];
  /** `export.pdb` and/or `exportLibrary.db`. */
  databases: string[];
  drive: string;
  /** Files to delete (removed tracks' audio, analysis and artwork), as drive keys. */
  files: string[];
  fits: boolean;
  free_space: number | null;
  history_removed: number;
  needed_bytes: number;
  /**
   * OneLibrary, beside `export.pdb`, changes. The counts above are `export.pdb`'s, so they
   * leave out edits only OneLibrary lacks, as after rekordbox exported over it alone.
   */
  onelibrary_changes: boolean;
  playlists_changed: PlaylistChange[];
  playlists_created: string[];
  playlists_deleted: string[];
  /** The edits as recorded in the drive's journal. */
  recorded: EditOutput[];
  root: string;
  tracks_changed: number;
  tracks_removed: TrackRef[];
  warnings: string[];
};

export type EditReport = {
  backup: string | null;
  changes: number;
  databases: string[];
  files_deleted: number;
  fingerprint: Hash | null;
  freed_bytes: number;
  warnings: string[];
};

export type EditRequest = {
  drive: string;
  edits: Edit[];
  /**
   * Skip edits that no longer apply (with a warning) instead of failing: for replaying
   * recorded edits after a new export.
   */
  lenient?: boolean;
  /** Don't record the edits in the drive's journal. */
  no_journal?: boolean;
};

/**
 * Changes to a set. Tracklist positions start at 1 and all name the tracklist as it was before
 * the edit: a track moved up lands just before the track that was at `to`, one moved down just
 * after it; an insert lands just before the track that was at `at` (one past the last appends).
 * Added sessions' tracks go at the end (or, when the set has a recording, are only checked
 * against it). Attaching a recording rebuilds the tracklist from it, so it can't come with
 * changes by position.
 */
export type EditSet = {
  /** Sessions whose tracks to add. */
  add_sessions?: string[];
  city?: string | null;
  /** ISO 3166 code, `GB`. */
  country?: string | null;
  /** The day it was played, `2026-10-03`. */
  date?: string | null;
  /**
   * The drive inserted tracks are looked up on and a recording is matched against (default:
   * the only mounted rekordbox drive).
   */
  drive?: string | null;
  /** Positions to drop. */
  drop?: number[];
  /** The `at://` URI of the event's `community.lexicon.calendar.event` record. */
  event?: string | null;
  insert?: Insert[];
  /** `club`, `festival`, `radio`, `livestream`, `mix`, or another word. */
  kind?: string | null;
  /** Where people can listen to it (`https://...`). */
  link?: string | null;
  moves?: Move[];
  /** Positions to name again. */
  named?: number[];
  /** Private notes, never published. */
  notes?: string | null;
  /** A recording to attach (identified against `drive`'s library), or a kept report. */
  recording?: string | null;
  report?: string | null;
  /** The set: `last`, an id prefix of 4 or more, a title, or a day. */
  set: string;
  title?: string | null;
  /** Positions to publish without naming the track. */
  unnamed?: number[];
  venue?: string | null;
  /**
   * Who they played with: names, or `did:...` for an Atmosphere account. Replaces the list;
   * empty clears it.
   */
  with?: string[] | null;
};

export type EjectDrives = {
  /** Drives, or a library's name for its mounted clones (but archive and retired ones). */
  drives: string[];
};

export type EjectPlan = {
  drives: EjectTarget[];
};

export type EjectReport = {
  /** What was run. */
  commands: string[];
  name: string;
  /** Whether the disk was also powered off (otherwise only unmounted). */
  powered_off: boolean;
  root: string;
};

/** A drive ejecting would unmount and power off. */
export type EjectTarget = {
  /** The other volumes mounted from its disk: ejecting unmounts them too. */
  also_unmounts: string[];
  /** The commands that eject it. */
  commands: string[];
  drive: string;
  root: string;
};

export type EmbedArtwork = {
  files: string[];
  image: string;
};

/**
 * Copies of the audio files one embed rewrote, as they were, kept in the store
 * (`embed-backups/<id>/`) so [`restore_embedded`] can put them back.
 */
export type EmbedBackup = {
  created: string;
  files: EmbeddedFile[];
  id: string;
  /** The image that went in. */
  image: string;
};

/** What embedding art in files would rewrite. */
export type EmbedPlan = {
  /** Kept in the store before they're rewritten, so `art unembed` can put them back. */
  backup_bytes: number;
  files: EmbedTarget[];
  image: string;
  /** Files left alone, and why (on a rekordbox export, missing...). */
  skipped: Skip[];
};

/** What embedding art into files did. */
export type EmbedReport = {
  /** The copies of the files as they were (see [`restore_embedded`]). */
  backup: string | null;
  embedded: string[];
  failed: Failure[];
};

/** A file art would be embedded in. */
export type EmbedTarget = {
  path: string;
  /** It has a front cover already, which is replaced. */
  replaces_art: boolean;
  size: number;
};

export type EmbeddedFile = {
  mtime: number;
  /** Its modification time before (put back with it). */
  mtime_before: number;
  /** The audio file. */
  path: string;
  /** Its copy as it was, in the backup's folder. */
  saved: string;
  /** The file once the art went in: it's only put back while it's still that. */
  size: number;
};

export type EnergySection = {
  /** Average mood (high 1, mid 0.5, low 0) across ten stretches of a set. */
  curve: (number | null)[];
  high: number;
  low: number;
  mid: number;
  /** Plays (or tracks) whose phrase analysis was read. */
  tracks: number;
};

/** Where a set's track came from. */
export type EntryOrigin =
  /** A session logged it. */
  | "session"
  /** It was heard in the recording. */
  | "recording"
  /** The DJ added it. */
  | "added";

export type EraSection = {
  /** Oldest first. */
  decades: DecadeCount[];
  median_year: number | null;
  newest: number | null;
  oldest: number | null;
  /** Released the year it was played, or the year before. */
  recent_share: number | null;
  /** Plays (or tracks) with a release year. */
  tracks: number;
};

/** A volume that formatting destroys. */
export type ErasedVolume = {
  /** locallibrary's name for the drive, if it was adopted. */
  drive_name: string | null;
  /** Whether that library's current state is in the store (`None`: no library, or no store). */
  in_store: boolean | null;
  /** Retired in the store. */
  retired: boolean;
  /**
   * Its role in the store. An archive or retired drive is only erased when writing to one is
   * allowed (see [`crate::fleet::check_target`]).
   */
  role: Role | null;
  /** Tracks in the rekordbox library on it, if any. */
  tracks: number | null;
  volume: Volume;
};

/** An error response's `error.data`: what a front end can act on without reading the message. */
export type ErrorData = {
  /**
   * What a front end can offer: call `method` with `params` (the request that failed, with
   * some parameters added, is one), with administrator rights when `elevate` is set.
   */
  actions: Action[];
  /**
   * The values it's about: a `path` (for `invalid_params`, where in the request), the space
   * `needed` and `available`, the `drive`, `left` and `right`...
   */
  fields: { [key: string]: unknown };
  /** Stable: `not_found`, `conflict`, `stale_plan`, `invalid_params`... (see [`crate::Error::code`]). */
  kind: string;
  /** Which decision a `conflict` needs, or what else would resolve the error. */
  reason: Reason | null;
};

export type Event =
  /** A phase begins. `items`/`bytes` are totals when known up front (0 otherwise). */
  | {
    bytes: number;
    event: "phase_start";
    items: number;
    phase: Phase;
  }
  /** Work on one item (usually a file) begins. */
  | {
    bytes: number;
    event: "item";
    path: string;
    phase: Phase;
  }
  /** `bytes` more bytes were processed in the current phase. */
  | {
    bytes: number;
    event: "bytes";
    phase: Phase;
  }
  | {
    event: "phase_end";
    phase: Phase;
  }
  | {
    event: "warning";
    message: string;
  };

export type Evidence = {
  detail: string;
  favors: Favors;
  signal: string;
};

/** Whether a model reads exFAT. */
export type Exfat =
  | ("no" | "yes")
  /** With this firmware version or later. */
  | {
    firmware: string;
  };

/** A hot cue (A-H), seconds into the track. */
export type ExportCue = {
  at: number;
  color?: string | null;
  label: string;
};

export type ExportFormat = "m3u8" | "csv" | "txt";

export type ExportMix = {
  drive?: string | null;
  /** Audio file or Audacity project (`.aup3`/`.aup4`). */
  mix: string;
  /** Also write the document to this file. */
  output?: string | null;
  /** Replace a file already at `output` (refused otherwise). */
  replace?: boolean;
  /** Playback speeds to search, as a fraction: 0.16 is ±16%. */
  tempo_range?: number | null;
};

/** A phrase from rekordbox's analysis (Intro, Up, Down, Chorus, Verse, Bridge, Outro). */
export type ExportPhrase = {
  end: number;
  kind: string;
  start: number;
};

export type ExportPlay = {
  /** 0-1. */
  confidence: number;
  end: number;
  /** Loops and cue jumps. */
  jumps: number;
  /** Camelot notation (`8A`). */
  key?: string | null;
  /** `None` when the speed is too close to 1.0 to tell. */
  keyLock: boolean | null;
  playedBpm?: number | null;
  playedKey?: string | null;
  /** How clearly the track is heard, every `presenceStep` seconds from `start`, 0-100. */
  presence: number[];
  presenceStep: number;
  semitones: number;
  /** Seconds into the mix where the track is first and last heard. */
  start: number;
  /** Playback speed: 1.03 is +3%. */
  tempo: number;
  track: ExportTrack;
  trackEnd: number;
  /** Seconds into the track at those points. */
  trackStart: number;
};

export type ExportPlaylist = {
  /** Also look for `history` among the lists archived in the store. */
  archived?: boolean;
  drive: string;
  /** Default: from `output`'s extension, else M3U8. */
  format?: ExportFormat | null;
  /** A history list (name or path) instead. */
  history?: string | null;
  /** The file to write; with `split`, the folder to write into. */
  output: string;
  paths?: PathStyle;
  /**
   * Playlist or folder (path like `Folder/Name`, or a unique name). The whole library when
   * neither this nor `history` is given.
   */
  playlist?: string | null;
  /** Replace files already there (refused otherwise). */
  replace?: boolean;
  /** For a folder: one file per playlist below `output`, instead of one file with every track. */
  split?: boolean;
};

export type ExportReport = {
  files: ExportedFile[];
  format: ExportFormat;
};

export type ExportSpan = {
  end: number;
  start: number;
};

/** A track: "ID" for its artist and title (and no more that names it) when the DJ hides it. */
export type ExportTrack = {
  album?: string | null;
  artist?: string | null;
  /** JPEG `data:` URL, 96 px square. */
  artwork?: string | null;
  /** As recorded. */
  bpm?: number | null;
  cues?: ExportCue[];
  durationSecs: number;
  genre?: string | null;
  /** As the library stores it. */
  key?: string | null;
  label?: string | null;
  /** rekordbox's overview waveform: heights 0-255 and `#rrggbb` colours, one per column. */
  overview?: Overview | null;
  phrases?: ExportPhrase[];
  remixer?: string | null;
  title: string;
  year?: number | null;
};

export type ExportTransition = {
  at: number;
  /** Indexes into `plays`. */
  from: number;
  /** `same key`, `±1`, `relative`, `energy boost` or `clash`. */
  harmony?: string | null;
  /** Seconds both are heard together (negative: a gap). */
  overlap: number;
  /** Played tempo change, percent. */
  tempoChange?: number | null;
  to: number;
};

/** The mix's loudness per column, 0-255, in three bands. */
export type ExportWaveform = {
  /** Above 2 kHz. */
  high: number[];
  /** Below 200 Hz. */
  low: number[];
  /** 200 Hz to 2 kHz. */
  mid: number[];
  secondsPerColumn: number;
};

export type ExportXml = {
  drive: string;
  location_root?: string | null;
  /** Only these playlists and folders, and their tracks. */
  only?: string[];
  output: string;
  /** Replace a file already at `output` (refused otherwise). */
  replace?: boolean;
  skip_analysis?: boolean;
};

export type ExportedFile = {
  /** The playlist (or history list, or library) it holds. */
  name: string;
  path: string;
  tracks: number;
};

export type ExportedMix = {
  artist?: string | null;
  /** The cover embedded in the recording: a JPEG `data:` URL, 320 px square. */
  cover?: string | null;
  date?: string | null;
  durationSecs: number;
  genre?: string | null;
  /** The player that recorded it, when the file says. */
  recorder?: string | null;
  title: string;
};

export type Failure = [path: string, error: string];

export type Favors = "left" | "right" | "neither";

/** A record read from a repository. */
export type Fetched = {
  /** The record's content hash (servers always send it). */
  cid: string | null;
  uri: string;
  value: unknown;
};

export type FieldChange = {
  field: string;
  new: string;
  old: string;
};

/** A file undoing changes. */
export type FileChange = {
  bytes: number;
  /**
   * `put back` (it's gone), `replaced` (it differs) or `removed` (a database the backup didn't
   * have).
   */
  change: string;
  /** The file, as a drive key. */
  path: string;
};

/** What a managed file is, which decides how it is synced. */
export type FileClass =
  /** `PIONEER/rekordbox/*`: export.pdb, exportExt.pdb, exportLibrary.db and its WAL. */
  | "database"
  /** `PIONEER/USBANLZ/**`: beat grids, cues, waveforms. */
  | "analysis"
  /** `PIONEER/Artwork/**`: cover art thumbnails. */
  | "artwork"
  /** `Contents/**` and any other file referenced by the database. */
  | "audio"
  /**
   * `PIONEER/*SETTING*.DAT`: player/mixer "My Settings". Managed by the settings profile,
   * never by library sync, so per-drive deviations survive.
   */
  | "settings"
  /**
   * `PIONEER/DeviceLibBackup/**`: rekordbox's per-device identity. Never copied between
   * drives, otherwise rekordbox would mistake one drive for another.
   */
  | "device_identity"
  /**
   * `exportLibrary.db-shm`: SQLite's shared-memory index, rebuilt from the WAL when the
   * database is opened. Never copied or fingerprinted (opening the database rewrites it);
   * deleted whenever the database or its WAL is replaced.
   */
  | "transient"
  /**
   * `PIONEER/extracted/**`: per-drive data rekordbox writes when it first sets a drive up
   * (`gcred.dat`, which looks like an account credential). Never copied between drives or into
   * the store, never deleted.
   */
  | "private"
  /**
   * What players keep for themselves on a drive (`PIONEER/LOG`, `CDJ`, `MPJ` and `TRASHBOX`,
   * and the track filter `PIONEER/rekordbox/RBFLTR.DAT`) and rekordbox's records of the
   * playlists it syncs to the drive (`playlists3*.sync`). It belongs to the drive, like
   * rekordbox's device identity: never copied, deleted or counted in the library's fingerprint.
   */
  | "drive_state"
  /** Anything else under `PIONEER/` (e.g. `djprofile.nxs`). */
  | "other";

export type FileProblem = {
  actual?: Hash | null;
  class: FileClass;
  /** Why it couldn't be read. */
  error?: string | null;
  expected?: Hash | null;
  /**
   * The modification time the good copy had (restored on repair, so syncs and caches see the
   * file they knew).
   */
  mtime?: number | null;
  path: string;
  reference?: ReferenceKind | null;
  /** `None`: no known-good copy is available. */
  repair?: RepairSource | null;
  size: number;
};

export type FileProblemOutput = {
  actual?: Hash | null;
  class: FileClass;
  /** Why it couldn't be read. */
  error?: string | null;
  expected?: Hash | null;
  /**
   * The modification time the good copy had (restored on repair, so syncs and caches see the
   * file they knew).
   */
  mtime?: number | null;
  path: string;
  reference?: ReferenceKind | null;
  /** `None`: no known-good copy is available. */
  repair: RepairSource | null;
  size: number;
};

/** File systems a drive can be formatted with. */
export type Filesystem = "fat32" | "exfat" | "hfsplus";

/** A device's update rules. */
export type FirmwareModel = {
  filesystems: FsKind[];
  model: string;
  /** Which socket to use, where the guide says. */
  port: string | null;
  /** The update file's name starts with this (any case) and ends in `.UPD`. */
  prefix: string;
  /** Only one update file may be on the drive. */
  single_file: boolean;
};

export type FirmwarePlan = {
  drive: string;
  /** Update files already in the root; removed with `replace`. */
  existing: string[];
  file: string;
  file_name: string;
  model: FirmwareModel | null;
  notes: string[];
  /**
   * What stops the device from taking the update. Applying refuses while there are any
   * (unless forced).
   */
  problems: string[];
  root: string;
  size: number;
};

/** The update files removing them would delete from a drive's root. */
export type FirmwareRemovePlan = {
  bytes: number;
  drive: string;
  files: string[];
  root: string;
};

export type FirmwareReport = {
  removed: string[];
  written: string;
};

export type FirmwareRequest = {
  drive: string;
  file: string;
  /** Copy it even if a check fails. */
  force?: boolean;
  /** Remove other update files from the drive's root. */
  replace?: boolean;
};

/** A mounted drive's status, with what the registry knows about it. */
export type FleetDrive = {
  /**
   * Back from a set: the drive is newer than the head only by what players add (history
   * lists, TAG LIST playlists), with this many history lists new or longer. Pulling saves them.
   */
  back_from_set: number | null;
  drive: DeviceInfo;
  error: string | null;
  /**
   * Recorded edits a rekordbox export replaced (see [`super::exported_over_edits`]): 0, or how
   * many `locallibrary refresh` (or `edits replay`) makes again.
   */
  exported_over_edits: number;
  head: SnapshotInfo | null;
  library: string | null;
  log: DriveLog | null;
  /** Less free space than players need for history and cues. */
  low_space: boolean;
  /** Write-protected: players can't save history or cues to it. */
  read_only: boolean;
  /**
   * The library head (left) vs. the drive (right): `identical`, `left_newer` (the drive is
   * behind: push), `right_newer` (the drive has newer changes: pull them), `diverged`... `None`
   * if not linked to a library.
   */
  relation: Relation | null;
  role: Role | null;
  /** Other mounted drives claiming the same identity (copies of this one, or this a copy). */
  same_identity_as?: string[];
  /** Settings that differ from the profile. */
  settings_drift: number;
};

/** A library and its clones, the drives holding it: those plugged in, and those that aren't. */
export type FleetLibrary = {
  /** Its latest snapshot. */
  head: SnapshotInfo | null;
  id: string;
  /**
   * Whether its clones plugged in hold the same library: `None` with fewer than two, or when
   * one couldn't be compared with the store.
   */
  identical: boolean | null;
  /**
   * Where its clones that are plugged in are mounted (each is in `mounted`), by role: the
   * primary first.
   */
  mounted: string[];
  name: string;
  /** Its registered clones that aren't plugged in (retired ones with `retired`). */
  offline: KnownDrive[];
  /**
   * Whether a clone plugged in holds changes the store doesn't have (it's newer than the latest
   * snapshot, or diverged from it).
   */
  unsaved: boolean;
};

export type FleetProblem = {
  at: string;
  detail: string;
  /** `damaged_files`, `unreadable_files`, `missing_files`, `repair_failed`, `fs_errors`. */
  kind: string;
};

export type FleetRequest = {
  /** Include drives that aren't mounted. */
  all?: boolean;
  /** Include retired drives. */
  retired?: boolean;
};

export type FleetStatus = {
  /** Mounted drives sharing one identity (copies of each other). */
  duplicates: Duplicate[];
  /**
   * The libraries of the mounted drives (with `all`, of every registered drive too), by name:
   * each with its clones. Drives of no library in the store are in none.
   */
  libraries: FleetLibrary[];
  mounted: FleetDrive[];
  /** Registered drives that aren't mounted (with `all`). */
  offline: KnownDrive[];
};

export type Forecast = {
  /** When the drive runs out of space at that rate (counting the players' headroom). */
  full_around: string | null;
  library: string;
  /** Library size in the newest snapshot. */
  library_bytes: number;
  /** Average growth, bytes per 30 days (negative when it shrinks). */
  per_month: number;
  snapshots: number;
  span_days: number;
};

export type FormatApply = {
  /** Test the disk's real capacity first (see [`capacity`]), and stop if it is fake. */
  check_capacity?: boolean;
  /** Bytes per cluster. Default: [`Filesystem::default_cluster_size`]. */
  cluster_size?: number | null;
  drive: string;
  /** Default: FAT32, which every player reads. */
  filesystem?: Filesystem | null;
  /** `identity` from the plan the user confirmed: the format is refused if the disk changed. */
  identity: string;
  /** Volume label. Default: the drive's locallibrary name, else its current label. */
  label?: string | null;
};

export type FormatPlan = {
  cluster_size: number;
  /** How players will get on with the result. */
  compatibility: Compatibility;
  disk: Disk;
  erases: ErasedVolume[];
  filesystem: Filesystem;
  /** Pass this to [`apply`] to confirm the disk. */
  identity: string;
  /** The locallibrary name and id the drive keeps (only when the OS mounts it afterwards). */
  keeps_name: string | null;
  label: string;
  /** Size of the new partition. */
  partition_size: number;
  scheme: Scheme;
  /** What will run. */
  steps: string[];
  warnings: string[];
};

export type FormatReport = {
  capacity: number | null;
  /** Cluster size of the mounted volume, as the OS reports it. */
  cluster_size: number | null;
  disk: string;
  /** The locallibrary name the drive kept. */
  drive_name: string | null;
  filesystem: Filesystem;
  label: string;
  /** Where the new volume is mounted (`None` if the OS didn't mount it). */
  mount: string | null;
};

export type FormatRequest = {
  /** Test the disk's real capacity first (see [`capacity`]), and stop if it is fake. */
  check_capacity?: boolean;
  /** Bytes per cluster. Default: [`Filesystem::default_cluster_size`]. */
  cluster_size?: number | null;
  /**
   * A mounted drive (path, or an adopted drive's name or id) or a disk (`disk4`, `/dev/sdb`,
   * `PhysicalDrive2`).
   */
  drive: string;
  /** Default: FAT32, which every player reads. */
  filesystem?: Filesystem | null;
  /** Volume label. Default: the drive's locallibrary name, else its current label. */
  label?: string | null;
};

export type FsCheckReport = {
  command: string;
  drive: string;
  filesystem: string | null;
  /** Whether the file system is consistent (after the repair, when repairing). */
  ok: boolean;
  /** The tool's report, last lines. */
  output: string[];
  repaired: boolean;
  root: string;
  /** The volume's device (`disk4s1`, `/dev/sdb1`, `E:`). */
  volume: string | null;
  /** What to be wary of: the check hasn't run on this platform before. */
  warnings: string[];
};

export type FsCheckRequest = {
  drive: string;
  repair?: boolean;
};

/** A file system found on a volume. */
export type FsKind = "fat12" | "fat16" | "fat32" | "exfat" | "hfsplus" | "apfs" | "ntfs" | "other";

/** Every session, newest first, and what couldn't be read. */
export type Gathered = {
  sessions: Session[];
  warnings: string[];
};

export type GcReport = {
  dry_run: boolean;
  freed_bytes: number;
  removed_objects: number;
  removed_snapshots: number;
};

/** `store.gc` with a request (rather than a `plan_id`). */
export type GcRequest = {
  /** Say what would go, without deleting it. */
  dry_run?: boolean;
};

export type Generator = {
  name: string;
  version: string;
};

/** The disk a drive was seen on. */
export type Hardware = {
  model: string;
  serial?: string | null;
  /** Whole-disk size in bytes. */
  size: number;
  /** The file system's identifier. FAT and exFAT give every format a new one. */
  volume_uuid?: string | null;
};

export type HarmonicMatch = {
  /** Camelot code, e.g. `8A`. */
  key: string;
  /** `same key`, `±1`, `relative` or `energy boost`. */
  relation: string;
  /** 0.02 is 2% apart (half and double time allowed). */
  tempo_gap: number;
  track: Track;
};

/** A BLAKE3 hash, as 64 lowercase hex characters. */
export type Hash = string;

export type HistoryArchive = {
  /** Oldest first. */
  lists: ArchivedHistory[];
  /** Snapshots read. */
  snapshots: number;
  /** Snapshots that couldn't be read. */
  warnings: string[];
};

export type HistoryArchiveRequest = {
  /** Only the library this drive is linked to (every library when it isn't linked). */
  drive?: string | null;
  /** Only this library (name or id). */
  library?: string | null;
};

/** One history list: the tracks a player played during one session, in order. */
export type HistoryList = {
  /**
   * The database it was read from: `device_library` (`export.pdb`) or `onelibrary` (the first
   * one, when both hold it).
   */
  database: string;
  /** Entries whose track isn't in the library any more. */
  missing: number;
  name: string;
  /** Folders and name (OneLibrary can keep history lists in folders). */
  path: string;
  tracks: Track[];
};

/** A history list as the journal names it. */
export type HistoryListName = {
  name: string;
  /** Its entries' audio files (device paths), in order. */
  tracks: string[];
};

export type HistoryRequest = {
  library: string;
  limit?: number | null;
};

/** A hot cue slot, A-H (written as its letter). */
export type HotCue = string;

/** A mix's report with what a front end needs to show it and follow along. */
export type IdentifiedMix = {
  analysed_at: string;
  /**
   * Audio a browser can play for the mix: the recording, or for an Audacity project an export
   * next to it (the file is there; `null` when there's none).
   */
  audio: string | null;
  /** Fraction of the mix where a track was identified. */
  coverage: number;
  drive: string;
  /** The drive's identity, to find it again by (absent for a drive locallibrary never adopted). */
  drive_id?: string | null;
  elapsed_secs: number;
  /**
   * Each play's overview waveform, hot cues and phrases from the drive, in the order of
   * `plays` (asked for with `extras`; empty when the drive isn't mounted).
   */
  extras?: PlayExtras[];
  library: IndexReport;
  mix: MixInfo;
  plays: Play[];
  /** The id the report is kept under in the store's reports (`mix.reports.get`), when saved. */
  saved?: string | null;
  tempo_range: number;
  transitions: Transition[];
  unidentified: Span[];
  waveform: MixWaveform;
};

/**
 * `mix.identify`'s request ([`super::mix_identify_cached`]): the mix, and what to answer with
 * its report.
 */
export type IdentifyMixRequest = {
  drive?: string | null;
  /**
   * Also each play's overview waveform, hot cues and phrases (`extras`), for following
   * along without asking for every track's analysis.
   */
  extras?: boolean;
  /** Audio file or Audacity project (`.aup3`/`.aup4`). */
  mix: string;
  /**
   * Keep the report in the store's reports folder (`saved` is its id; see
   * [`mix_reports_list`]).
   */
  save?: boolean;
  /** Playback speeds to search, as a fraction: 0.16 is ±16%. */
  tempo_range?: number | null;
};

/** One track as an outside list describes it. */
export type ImportEntry = {
  album: string | null;
  /** Empty when the list names none. */
  artist: string;
  duration_secs: number | null;
  isrc: string | null;
  /** 1-based line of the file where the entry starts. */
  line: number;
  /** Mix / version name, when the list has a column for it ("Original Mix"). */
  mix: string | null;
  /**
   * The file as the list names it, with `file://` URLs decoded (`/Users/me/Music/x.mp3`,
   * `C:\Music\x.mp3`, `../x.mp3`).
   */
  path: string | null;
  /** The text it came from (the line, CSV record or XML tag), for messages. */
  raw: string;
  title: string;
};

/** What a tracklist file is. */
export type ImportFormat =
  /** M3U / M3U8 playlists, and plain lists of file paths. */
  | "m3u"
  /** A table with a header row (Exportify, TuneMyMusic, Soundiiz, Beatport-style...). */
  | "csv"
  /** rekordbox's "Export a playlist to a file" text. */
  | "rekordbox_text"
  /** rekordbox XML (`DJ_PLAYLISTS`). */
  | "rekordbox_xml"
  /** One track per line. */
  | "text";

export type ImportPreview = {
  /** `None` when no entry was found. */
  plan: EditPlan | null;
  report: Report;
};

export type ImportPublishedSettings = {
  /** Whose My Settings: a handle or DID. Default: the signed-in account's. */
  from?: string | null;
  /**
   * Their data server's URL (https, or http on this machine), instead of finding it from the
   * handle or DID.
   */
  pds?: string | null;
  /**
   * Replace the settings profile's whole base with theirs, so settings they didn't publish
   * become unset. Default: add theirs to the base, over the values it has for them.
   */
  replace?: boolean;
};

export type ImportRequest = {
  /** Add to the playlist instead of replacing its contents. */
  append?: boolean;
  /** Decisions for ambiguous entries: entry index (in the report) -> track id. */
  choices?: Record<string, never>;
  drive: string;
  /**
   * A tracklist: M3U, CSV (Spotify, TIDAL, Beatport... exports), rekordbox text or XML, or
   * plain "Artist - Title" lines.
   */
  file: string;
  /** The playlist to fill (created if needed). */
  into: string;
  /** For rekordbox XML: which of its playlists, by path (the whole collection otherwise). */
  playlist?: string | null;
};

export type ImportSessionLog = {
  /**
   * The drive whose library the log's tracks are looked up in. Default: the only mounted
   * rekordbox drive (with none mounted, the log is kept as it is).
   */
  drive?: string | null;
  /** A Beat Link Trigger Playlist Writer CSV. */
  file: string;
  /** Default: the file's name. */
  name?: string | null;
};

export type ImportSettings = {
  /** Drive to import from; `None` imports rekordbox's own "My Settings" on this computer. */
  drive?: string | null;
};

/**
 * What importing a drive's settings, or rekordbox's, would make of the profile's base. (For
 * the My Settings an account published, see [`SettingsImportPlan`].)
 */
export type ImportSettingsPlan = {
  changes: BaseChange[];
  /** The drive, or `rekordbox on this computer`. */
  from: string;
  /** The values read. */
  values: number;
};

export type ImportSummary = {
  ambiguous: number;
  entries: number;
  found: number;
  missing: number;
};

/** A play log imported from another tool, with real play times. */
export type ImportedSession = {
  /** The file it was imported from. */
  file: string;
  /** Like a history list's fingerprint: the same log imported twice is one session. */
  id: string;
  imported_at: string;
  name: string;
  plays: TimedPlay[];
  /** `beat_link_trigger`. */
  tool: string;
};

export type IndexMixLibrary = {
  /** Default: the only mounted rekordbox drive. */
  drive?: string | null;
};

export type IndexReport = {
  /** Audio files that couldn't be decoded. */
  failed: TrackFailure[];
  /** Fingerprinted just now (the rest came from the cache). */
  fingerprinted: number;
  hashes: number;
  /** Tracks in the index. */
  indexed: number;
  /** Tracks over 20 minutes ([`LONGEST_TRACK_SECS`]), left out: recorded sets, not tracks. */
  long: TrackRef[];
  /** Audio files missing from the drive. */
  missing: TrackRef[];
  /** Tracks in the library. */
  tracks: number;
};

/** What `initialize` answers. */
export type Initialized = {
  /**
   * Features a client can check for: `progress`, `progress.scope`, `cancel`, `jobs.list`,
   * `shutdown.cancel`, `events`, `plan_id`, `describe`, `error.detail`, and `account` when the
   * sidecar was built with the Atmosphere account (the `account.*` and `*.publish.*` methods,
   * and `settings.import_published.*`).
   */
  capabilities: string[];
  /** Every method's name, aliases included (`rpc.describe` says more about each). */
  methods: string[];
  /** Changes only when a change would break clients written for the previous one. */
  protocol: number;
  /** The sidecar's version. */
  version: string;
};

export type Insert = {
  at: number;
  /**
   * A search for one of the drive's tracks (`Artist - Title`, part of it, or a path), else
   * the track as typed: `Artist - Title`.
   */
  track: string;
};

/** What an export writes, and what it leaves out. */
export type InteropPlan = {
  beat_grids: number;
  /** Bytes of audio copied (Serato with `write_tags`). */
  copy_bytes: number;
  /** Hot cues, memory cues and loops carried across. */
  cues: number;
  files: PlannedFile[];
  folders: number;
  /** What the format can't hold, and what to do with the files. */
  notes: string[];
  output: string;
  playlists: number;
  software: Software;
  tracks: number;
  /** Problems with this library: missing audio, cues the format has no room for... */
  warnings: string[];
};

export type InteropRequest = {
  drive: string;
  /**
   * The folder that will hold the drive's `Contents/` when the other software reads the files.
   * Default: the drive's mount point.
   */
  location_root?: string | null;
  /**
   * Only these playlists and folders (paths like `Folder/Name`, or unique names) and their
   * tracks. Everything when empty.
   */
  only?: string[];
  /** The folder to write into (never on the drive). */
  output: string;
  /**
   * Engine DJ: track paths relative to where the library is written (`relative`, the
   * default), or to the drive's root (`drive`, for an `Engine Library` folder copied there).
   */
  paths?: PathStyle;
  /** Replace files the output folder already holds. */
  replace?: boolean;
  /** Leave out beat grids and cues. */
  skip_analysis?: boolean;
  software: Software;
  /**
   * Serato: copy the audio into the output folder and write cue points into the copies' tags
   * (the drive's files are never changed); the crates list the copies.
   */
  write_tags?: boolean;
};

/** A drive a job is about. */
export type JobDrive = {
  name: string;
  root: string;
};

/** A request starting or finishing. */
export type JobEvent = {
  event: "job.started";
  id: RequestId | null;
  method: string;
  started: string;
} | {
  event: "job.finished";
  id: RequestId | null;
  /** The error's `kind`, when it failed. */
  kind?: string | null;
  method: string;
  ok: boolean;
};

/** A running request, as `jobs.list` lists it. */
export type JobInfo = {
  /** `null` for a notification (and for `watch`). */
  id: RequestId | null;
  method: string;
  /** Waiting for a worker. */
  queued?: boolean;
  started: string;
};

/** A job's plan: its steps, in the order they run (see the module docs). */
export type JobPlan = {
  /** Every step, including the ones that won't run (`runs: false`, with the reason). */
  steps: JobStep[];
  /** The drives it's about. */
  targets: JobDrive[];
};

/** What applying a job's plan did. */
export type JobReport = {
  /** The check's report, when it ran. */
  check?: CheckReport | null;
  /** The check found problems, so the job stopped before ejecting the drives. */
  problems: boolean;
  /** The steps that ran, in order (a step that failed part of the way is here with what it did). */
  steps: StepReport[];
  /** The step that failed, which stopped the job. The steps before it stay done. */
  stopped?: StepError | null;
};

/** One step of a job's plan. */
export type JobStep = {
  /** It deletes or overwrites something. */
  destructive: boolean;
  /** The plans of the operations it runs, as their `*.plan` methods return them. */
  plans?: unknown[];
  reason?: string | null;
  /** Whether it runs when the plan is applied; when it doesn't, `reason` says why. */
  runs: boolean;
  step: StepKind;
  /**
   * What it will change, in a line each; the pull step also names the drives the store holds
   * already, which it leaves alone. A step after one that writes to the same drive is planned
   * on the drive as it is now, and planned again when it runs.
   */
  summary: string[];
  /** What it does, in a few words. */
  title: string;
  warnings: string[];
};

export type JournalEntry = {
  at: string;
  /** The backup made before them, which `restore` undoes them with. */
  backup?: string | null;
  /** What the edits did, as described when they were applied. */
  changes: string[];
  edits: EditOutput[];
};

/** Why a snapshot stays. */
export type KeepReason =
  | ("label" | "last" | "daily" | "weekly")
  /** The library's latest. */
  | "latest"
  /** A drive was last seen holding it. */
  | "drive"
  /** The newest snapshot holding a history list. */
  | "history"
  /** Its databases couldn't be read, so what history lists it holds isn't known. */
  | "unreadable";

export type KeptSnapshot = {
  /** With `drive`: the drives last seen holding it. */
  drives?: string[];
  /** With `history`: the history lists no newer kept snapshot holds. */
  history?: string[];
  reasons: KeepReason[];
  snapshot: SnapshotInfo;
};

export type KeyCount = [key: string, count: number];

/** A track whose stored key the estimate doesn't agree with. */
export type KeyFinding = {
  /** 0 to 1. */
  confidence: number;
  /** The analyzer's key, as a Camelot code (`8A`). */
  estimate: string;
  /** The estimate against the stored key: `fifth`, `relative`, `parallel` or `other`. */
  relation: KeyOutcome;
  /** The library's key, as it stores it. */
  stored: string;
  track: TrackRef;
};

/**
 * MIREX's key categories, from the [Audio Key Detection task](https://music-ir.org/mirex/wiki/2021:Audio_Key_Detection).
 * A fifth counts either way round, as MIREX has scored it since 2017 (mir_eval's
 * `allow_descending_fifths`, [key.py](https://github.com/mir-evaluation/mir_eval/blob/main/mir_eval/key.py)).
 */
export type KeyOutcome =
  | ("correct" | "other")
  /** A fifth above or below in the same mode: a neighbour on the Camelot wheel. */
  | "fifth"
  /** Major for its relative minor or back: same Camelot number. */
  | "relative"
  /** Major for minor on the same tonic. */
  | "parallel"
  /** The analyzer gave no key. */
  | "missing";

/** How one analyzer did. */
export type KeyOutcomeReport = {
  /** Tracks with each outcome, every outcome listed. */
  counts: { [key: string]: number };
  /** The tracks it didn't get fully right, in the order they came, with what it did instead. */
  misses: [string, KeyOutcome][];
  /** The mean credit, 0 to 1: MIREX's weighted key score, or tempo Accuracy1. */
  score: number;
  tracks: number;
};

export type KeysSection = {
  minor_share: number | null;
  top: Share[];
  tracks: number;
  /** Every Camelot key in wheel order (1A, 1B, 2A ... 12B), with its count. */
  wheel: KeyCount[];
};

export type Kind =
  /** A multi player (CDJ, XDJ-1000...). */
  | "player"
  /** Players and a mixer in one unit (XDJ-XZ, OPUS-QUAD...). */
  | "all_in_one"
  /** A mixer that reads USB drives for settings and firmware only. */
  | "mixer";

/** A drive from the registry. */
export type KnownDrive = {
  capacity: number | null;
  id: string;
  last_mount: string;
  last_seen: string;
  library: string | null;
  log: DriveLog;
  /** Where it is mounted now, if it is. */
  mounted: string | null;
  name: string;
  role: Role | null;
  standing: Standing;
};

/** A label from a label track (DJs sometimes mark where tracks change). */
export type Label = {
  end: number;
  start: number;
  title: string;
};

export type LabelRequest = {
  label: string;
  /** Take the label off instead. */
  remove?: boolean;
  /** The snapshot to label: an id prefix or another label. Not needed with `remove`. */
  snapshot?: string | null;
};

export type LargeFile = {
  path: string;
  size: number;
  /** The track using it, if any. */
  track: TrackRef | null;
};

export type Level = "ok" | "warning" | "problem";

/** A rekordbox library as seen on a device export. */
export type Library = {
  /**
   * The history playlists themselves (and, in OneLibrary, their folders), shaped like
   * playlists.
   */
  history: Playlist[];
  /** Number of history playlists (players record what was played). */
  history_playlists: number;
  playlists: Playlist[];
  tracks: Track[];
};

/**
 * Differences between two exports *of the same library*. Tracks are matched by file path,
 * which rekordbox keeps stable across exports; playlists by their place in the folder tree.
 */
export type LibraryDiff = {
  added_playlists: string[];
  added_tracks: TrackRef[];
  /** Playlists whose contents or order differ. */
  changed_playlists: string[];
  changed_tracks: TrackChange[];
  removed_playlists: string[];
  removed_tracks: TrackRef[];
};

/**
 * Differences between a drive's two databases. rekordbox exports the same collection to both,
 * but they can drift apart (rekordbox applies some edits to only one of them), and then players
 * that read OneLibrary (CDJ-3000X, OPUS-QUAD...) show a different library from the others.
 */
export type LibraryDrift = {
  /** Tracks in both (same file) whose details differ. */
  changed_tracks: TrackChange[];
  /** OneLibrary couldn't be read, so nothing could be compared. */
  error: string | null;
  /** Tracks only `export.pdb` lists: OneLibrary players don't see them. */
  only_in_device_library: TrackRef[];
  /** Tracks only OneLibrary lists: older players don't see them. */
  only_in_onelibrary: TrackRef[];
  /** Playlists (and folders, ending in `/`) in only one database, or with different contents. */
  playlists: string[];
};

/** A library in the store, for listings. */
export type LibraryListing = {
  created: string;
  /** Drives linked to it, by name. */
  drives: string[];
  /** The latest snapshot. */
  head: SnapshotInfo | null;
  id: string;
  name: string;
  /** Its snapshots, back from the latest through first parents. */
  snapshots: number;
  updated: string;
};

export type LibraryPage = {
  /** Its tracks are the page asked for (every track when unpaged). */
  library: Library;
  /** Set when paged. */
  page?: Page | null;
  summary: LibrarySummary;
};

export type LibraryPlayers = {
  id: string;
  models: string[];
  /** `None` when the library is no longer in the store. */
  name: string | null;
};

export type LibraryPrune = {
  /** Newest first. */
  kept: KeptSnapshot[];
  library: string;
  /** Newest first. */
  removed: SnapshotInfo[];
};

export type LibraryRecord = {
  created: string;
  head: Hash | null;
  id: string;
  name: string;
  updated: string;
};

/** `drive.library`, optionally paged: without `offset` or `limit` every track comes, as before. */
export type LibraryRequest = {
  drive: string;
  limit?: number | null;
  offset?: number | null;
};

export type LibrarySummary = {
  artists: number;
  audio_bytes: number;
  folders: number;
  playlists: number;
  total_duration_secs: number;
  tracks: number;
  tracks_without_artwork: number;
};

export type LineageEntry = {
  at: string;
  fingerprint: Hash;
  op: LineageOp;
  snapshot?: Hash | null;
  /** Drive the content came from (sync/merge). */
  source?: string | null;
  tracks: number;
};

export type LineageOp =
  | ("pull" | "push" | "sync" | "merge" | "artwork" | "restore" | "clean")
  /** locallibrary saw this state (e.g. after a rekordbox export). */
  | "observed"
  /** One database built from the other (see [`crate::convert`]). */
  | "convert"
  /** Playlists, tracks or cues edited in place (see [`crate::edit`]). */
  | "edit"
  /** A subset of another drive's library written to this one (see [`crate::pack`]). */
  | "pack"
  /** Damaged files rewritten from a known-good copy (see [`crate::scrub`]). */
  | "repair"
  /** An operation from a newer version of locallibrary. */
  | "other";

/**
 * Which tracks an operation targets: the tracks that fit every field given, so a playlist and a
 * filter together pick the playlist's tracks the filter matches. At least one field must be set,
 * and a blank query, filter, album, playlist or history list is refused rather than taken to
 * mean every track.
 */
export type ListCues = {
  /** Every track whose album has this name. */
  album?: string | null;
  drive: string;
  /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
  filter?: string | null;
  /** The tracks of a history list (what a player recorded), by name. */
  history?: string | null;
  ids?: number[];
  /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
  paths?: string[];
  playlist?: string | null;
  /** Matches "artist - title" or path substrings; must match at least one track. */
  query?: string | null;
};

export type ListDrives = {
  /** Include mounted volumes without a rekordbox export (possible sync targets). */
  all?: boolean;
};

/** A sign-in waiting for the browser. */
export type LoginStart = {
  /** The client signing in. */
  client: Client;
  /** Pass to [`finish_login`] / [`finish_login_with_url`]. */
  id: string;
  /** Why it isn't locallibrary's own client, when it fell back to the development one. */
  note: string | null;
  redirect_uri: string;
  /** The host the account signs in at, e.g. `bsky.social`. */
  server: string;
  /** Open this in the browser. */
  url: string;
};

export type LogoutReport = {
  /** The account that was signed in (`None`: nobody was). */
  account: AccountInfo | null;
  /**
   * Why some of the session's secrets are still on this computer (the keychain didn't
   * answer); signing out again tries again. `None`: they're gone.
   */
  left_behind: string | null;
  /**
   * Whether the server revoked the session's tokens. When it couldn't be reached (or offers
   * no revocation), the session is still removed from this computer and ends on its own.
   */
  revoked: boolean;
};

export type LoopBeats = [numerator: number, denominator: number];

export type LooseFile = {
  path: string;
  size: number;
};

/** A track whose level may be a problem. */
export type LoudnessFinding = {
  clipped_runs: number;
  /** LUFS (`None` for silence). */
  integrated_lufs: number | null;
  problems: LoudnessProblem[];
  track: TrackRef;
  true_peak_dbtp: number;
};

/** What may be wrong with a track's level. */
export type LoudnessProblem =
  /** Its true peak is above 0 dBTP: a player's converter or a lossy encode can clip it. */
  | "over_zero"
  /** Runs of samples at full scale: clipped when it was mastered or converted. */
  | "clipped"
  /** Its integrated loudness is more than [`FAR_FROM_MEDIAN_LU`] from the library's median. */
  | "far_from_median";

/** How an entry was found. */
export type MatchKind =
  | "isrc"
  /** Same file (by its trailing path components). */
  | "path"
  /** Same normalised artist and title, durations within 2 seconds. */
  | "artist_title_duration"
  /** Same normalised artist and title. */
  | "artist_title"
  /** The entry names no artist; same normalised title. */
  | "title"
  /** Close enough on fuzzy token similarity, with no close runner-up. */
  | "fuzzy"
  /** Picked from the candidates by hand. */
  | "chosen";

export type MatchMode =
  /** Same path + size, or same artist/title/duration (default). */
  | "metadata"
  /** Same path + size only. */
  | "path";

export type Matched = {
  entry: ImportEntry;
} & (
  | {
    how: MatchKind;
    status: "found";
    track_id: number;
  }
  /**
   * Needs a decision: several tracks fit equally well, or the only candidates are weak.
   * Best first.
   */
  | {
    candidates: number[];
    status: "ambiguous";
  }
  | {
    status: "missing";
  });

export type MatchesRequest = {
  drive: string;
  limit?: number | null;
  track: number;
};

export type MergePlan = {
  artwork_files: number;
  copy_bytes: number;
  fits: boolean;
  free_space: number | null;
  /** History playlists to bring across (with [`MergeOptions::history`]). */
  history: string[];
  /** Source tracks already in the target. */
  matched_tracks: number;
  /** Estimated space needed on the target (cluster-rounded, including the new database). */
  needed_bytes: number;
  /** My Tag categories and tags the target gets for them. */
  new_tags: Created[];
  new_tracks: PlannedTrack[];
  playlists: PlannedPlaylist[];
  source: string;
  /** New tracks that bring My Tags along. */
  tagged_tracks: number;
  target: string;
  warnings: string[];
};

export type MergeReport = {
  artwork_added: number;
  backup: string | null;
  copied_bytes: number;
  fingerprint: Hash | null;
  history_playlists_added: number;
  hot_cue_bank_lists_added: number;
  playlist_entries_added: number;
  playlists_created: number;
  /** New tracks given My Tags (in either database). */
  tagged_tracks: number;
  tracks_added: number;
  tracks_matched: number;
};

export type MergeRequest = {
  /** Folder name for [`PlaylistMode::Folder`] (default: the source drive's name). */
  folder_name?: string | null;
  /**
   * Also bring the source's history playlists (what players recorded during a set) across,
   * except ones the target already has.
   */
  history?: boolean;
  match_mode?: MatchMode;
  /**
   * Only merge these source playlists (paths like `Sets/Warmup` or unique names) and the
   * tracks in them. Empty = everything.
   */
  only_playlists?: string[];
  playlists?: PlaylistMode;
  source: string;
  target: string;
};

/** An entry the source has whose track isn't on the drive any more. */
export type MissingEntry = {
  /** The audio file, as the database lists it. */
  file: string;
  /** The playlist's path. */
  playlist: string;
  /** Its position in the source playlist, from 1. */
  position: number;
  /** `Artist - Title`. */
  track: string;
};

export type MixExport = {
  /** RFC 3339. */
  analysedAt: string;
  /** Fraction of the mix where a track was identified, 0-1. */
  coverage: number;
  format: string;
  generator: Generator;
  mix: ExportedMix;
  plays: ExportPlay[];
  /** Playback speeds searched, as a fraction either way (0.16 is ±16%). */
  tempoRange: number;
  transitions: ExportTransition[];
  /** Stretches where no track was identified. */
  unidentified: ExportSpan[];
  version: number;
  waveform: ExportWaveform;
};

/** What was analysed. */
export type MixInfo = {
  artist: string | null;
  channels: number;
  /** Cover image: embedded in the audio file, else an image next to it named like it. */
  cover: CoverSource | null;
  date: string | null;
  duration_secs: number;
  genre: string | null;
  /** `audio` or `audacity`. */
  kind: string;
  /** Labels from an Audacity project's label tracks. */
  labels: Label[];
  path: string;
  /** The device that recorded it, when the file says (Pioneer DJ's recorders tag their model). */
  recorder: string | null;
  sample_rate: number;
  title: string;
};

/** The mix's loudness for drawing it: per column, 0-255. */
export type MixWaveform = {
  columns_per_sec: number;
  high: number[];
  /** Below 200 Hz, 200 Hz-2 kHz, above 2 kHz. */
  low: number[];
  mid: number[];
  peak: number[];
};

export type MixingSection = {
  adjacent: number;
  clash: number;
  energy_boost: number;
  genre_change_share: number | null;
  /** Transitions where both tracks have a genre. */
  genred: number;
  /** Transitions that only line up at half or double time. */
  half_double: number;
  /** (same + adjacent + relative + energy boost) / keyed. */
  harmonic_share: number | null;
  /** Transitions where both keys are known. */
  keyed: number;
  /**
   * Median tempo change between consecutive tracks, in percent (half and double time count
   * as the same tempo).
   */
  median_step_pct: number | null;
  relative: number;
  same_key: number;
  /** Transitions where both BPMs are known. */
  timed: number;
  /**
   * Pairs of tracks played one after the other within sets (not across a track gone from
   * the library, nor a track played twice in a row).
   */
  transitions: number;
  within_3pct: number | null;
  within_6pct: number | null;
};

export type Model = {
  /** The formats it plays. A format missing from the list doesn't play. */
  audio: Audio[];
  /** The list of formats is itself inferred: a format missing from it probably doesn't play. */
  audio_inferred: boolean;
  exfat: Spec_for_Exfat;
  /** Reads Device Library (`export.pdb`). */
  export_pdb: Spec_for_boolean;
  kind: Kind;
  /** As AlphaTheta writes it: `CDJ-2000NXS2`, `OPUS-QUAD`. */
  name: string;
  /** What else is worth knowing about it. */
  notes: string[];
  /** Reads OneLibrary (`exportLibrary.db`, called Device Library Plus before October 2025). */
  onelibrary: Spec_for_boolean;
  /** Where the values come from. */
  source: string;
};

/** rekordbox's overall classification of a track, which decides how phrases are named. */
export type Mood = "high" | "mid" | "low";

/** A mounted drive, as cheaply as it can be described: no database is read. */
export type MountedDrive = {
  /** Whether it holds a rekordbox export (either database). */
  has_export: boolean;
  /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
  id: string | null;
  /** Its adopted name, else the volume's name. */
  name: string;
  root: string;
};

export type Move = {
  from: number;
  to: number;
};

export type MoveOutput = {
  /** 1-based position on the left. */
  from: number;
  /** 1-based position on the right. */
  to: number;
  track: TrackRef;
};

export type MyTagCategory = {
  name: string;
  /** The databases that have it. */
  sources: Source[];
  tags: TagInfo[];
};

/** A drive's My Tags, from both databases. */
export type MyTags = {
  /** In display order: the first source's order, then anything only the other one has. */
  categories: MyTagCategory[];
  /** Where `exportExt.pdb` and `exportLibrary.db` disagree. */
  differences: string[];
  drive: string;
  /** The databases read. */
  sources: Source[];
  warnings: string[];
};

/** A set's details. Left out, a field stays as it is; empty (`""`), it's cleared. */
export type NewSet = {
  city?: string | null;
  /** ISO 3166 code, `GB`. */
  country?: string | null;
  /** The day it was played, `2026-10-03`. */
  date?: string | null;
  /**
   * The drive whose library a recording is matched against (default: the only mounted
   * rekordbox drive).
   */
  drive?: string | null;
  /** The `at://` URI of the event's `community.lexicon.calendar.event` record. */
  event?: string | null;
  /** `club`, `festival`, `radio`, `livestream`, `mix`, or another word. */
  kind?: string | null;
  /** Where people can listen to it (`https://...`). */
  link?: string | null;
  /** Private notes, never published. */
  notes?: string | null;
  /**
   * A recording of the set: identified against `drive`'s library, its report kept in the
   * store. Its tracklist beats the sessions'.
   */
  recording?: string | null;
  /** Or a report kept in the store already (`mix.reports.list`). */
  report?: string | null;
  /** Sessions to build it from, in play order (ids, or any unique prefix of 4 or more). */
  sessions?: string[];
  title?: string | null;
  venue?: string | null;
  /**
   * Who they played with: names, or `did:...` for an Atmosphere account. Replaces the list;
   * empty clears it.
   */
  with?: string[] | null;
};

/** No parameters (any object is accepted). */
export type NoParams = { [key: string]: unknown };

/**
 * An `event` notification's parameters: drives and the store (after `events.subscribe`),
 * requests starting and finishing, and what `watch.start` does.
 */
export type Notice = WatchEvent | JobEvent | WatchAction;

/** One answer for `One`, a list in the order asked for `Many`. */
export type OneOrMany_for_ArtworkFile = (ArtworkFile | null) | TrackArtwork_for_ArtworkFile[];

/** One answer for `One`, a list in the order asked for `Many`. */
export type OneOrMany_for_ArtworkImage = (ArtworkImage | null) | TrackArtwork_for_ArtworkImage[];

export type OrphanCleanReport = {
  backup: string | null;
  files_deleted: number;
  fingerprint: Hash | null;
  freed_bytes: number;
};

export type OrphanFile = {
  class: FileClass;
  /** Drive key. */
  path: string;
  size: number;
};

export type OrphanPlan = {
  /** Total size of `files`. */
  bytes: number;
  drive: string;
  files: OrphanFile[];
  fits: boolean;
  free_space: number | null;
  /** Space needed for the backup of the database and the deleted files. */
  needed_bytes: number;
  root: string;
  warnings: string[];
};

export type Overview = {
  colors: string[];
  heights: number[];
};

export type PackCopy = {
  path: string;
  size: number;
};

export type PackPlan = {
  copies: PackCopy[];
  copy_bytes: number;
  delete_bytes: number;
  /** Files on the target the packed library doesn't use. */
  deletes: PackCopy[];
  fits: boolean;
  free_space: number | null;
  /** Size of the packed library's files. */
  library_bytes: number;
  /**
   * Space the pack needs on the target: the files it copies, its new databases and the backup
   * of what it replaces.
   */
  needed_bytes: number;
  playlists: string[];
  /** The target holds a library that isn't an earlier pack of this source. */
  replaces_library: boolean;
  source: string;
  target: string;
  target_root: string;
  tracks: number;
  unchanged: number;
  /** What the target holds that only it has: saved in the store before packing. */
  unsaved: Unsaved | null;
  warnings: string[];
};

export type PackReport = {
  /**
   * The backup of what the pack replaced: `locallibrary undo` puts its databases, analysis and
   * artwork back. Tracks the pack removed have no audio in it (their audio is on the source),
   * so taking back a pack that removed tracks needs `--force` (and leaves them without audio)
   * or packing again with the old selection.
   */
  backup: string | null;
  copied: number;
  copied_bytes: number;
  deleted: number;
  deleted_bytes: number;
  fingerprint: Hash | null;
  /** The target's state, saved in the store before packing (see [`PackPlan::unsaved`]). */
  saved: SnapshotInfo | null;
};

export type PackRequest = {
  /** Keep the source's history lists too. */
  history?: boolean;
  /** Leave files on the target that the packed library doesn't use. */
  keep_extra?: boolean;
  /** Write to an archive or retired drive (see [`crate::fleet::check_target`]). */
  overwrite_archive?: boolean;
  /**
   * Playlists or folders (paths) to put on the target. Empty: the selection the target was
   * last packed with.
   */
  playlists?: string[];
  source: string;
  target: string;
};

/** One layer of a [`PackedWaveform`]. */
export type PackedLayer = {
  /** Three bytes a column: its red, green and blue. */
  colors: string;
  /** One byte a column: its height, 0-255 of the waveform's full height. */
  heights: string;
};

/** A [`Waveform`] packed for sending: each layer's heights and colours as bytes in base64. */
export type PackedWaveform = {
  back: PackedLayer;
  /** How many columns. */
  columns: number;
  columns_per_sec: number | null;
  /** Always `base64`; a [`Waveform`] has none. */
  encoding: string;
  /** `None` when no column has a front layer (scrolling waveforms and blue ones). */
  front: PackedLayer | null;
  style: WaveformStyle;
};

export type Page = {
  /** How many tracks the page holds. */
  count: number;
  offset: number;
  /** How many tracks the library has. */
  total: number;
};

/** Someone the DJ played with. */
export type Partner = {
  /** Their Atmosphere account, when known. */
  did?: string | null;
  name: string;
};

/** How file paths are written. */
export type PathStyle =
  /**
   * Relative to the output file's folder (absolute when there is no relative path, e.g. on
   * another Windows drive letter).
   */
  | "relative"
  /** The drive's own path, `/Contents/Artist/Track.mp3`. */
  | "drive"
  /** Absolute on this computer, through the drive's mount point. */
  | "absolute";

/** Header fields useful for telling exports apart. */
export type PdbHeader = {
  num_tables: number;
  page_size: number;
  /** Incremented by rekordbox on every save; higher means a later revision of the *same* file. */
  sequence: number;
};

export type PendingOp = {
  /** The backup it made before changing anything: putting it back takes it back. */
  backup?: string | null;
  description: string;
  op: LineageOp;
  /**
   * The player name and colour the drive had before a sync, push or pack replaced its
   * `exportLibrary.db`, put back once it's done (see [`crate::player::stash`]).
   */
  player?: DbIdentity | null;
  started: string;
};

export type Phase =
  | ("scan" | "hash" | "copy" | "write" | "delete" | "verify" | "format")
  /** Decoding and fingerprinting library tracks. */
  | "fingerprint"
  /** Building the fingerprint index. */
  | "index"
  /** Decoding and fingerprinting a mix. */
  | "listen"
  /** Looking for library tracks in a mix. */
  | "match"
  /** Decoding library tracks to estimate their tempo, key and loudness. */
  | "analyze";

export type Phrase = {
  end_beat: number;
  /** Beat where a fill-in leads into the next phrase. */
  fill_beat: number | null;
  kind: PhraseKind;
  /** 1-based beat numbers into the beat grid; the phrase runs up to (not including) `end_beat`. */
  start_beat: number;
};

export type PhraseKind =
  | ("intro" | "bridge" | "chorus" | "outro")
  /** Build-ups, in high-mood tracks. */
  | "up"
  /** Breakdowns, in high-mood tracks. */
  | "down"
  | {
    verse: number;
  }
  | {
    unknown: number;
  };

export type PhraseSpan = {
  end: number;
  /** `Intro`, `Up`, `Chorus`... */
  kind: string;
  start: number;
};

export type PhraseSummary = {
  bars: number;
  /** `Intro`, `Up`, `Verse 2`... */
  kind: string;
};

/** What every plan says besides its own fields, as `*.plan` returns it (see [`Planned`]). */
export type PlanFields = {
  /**
   * The library's other mounted clones, which applying copies the drive it writes onto, but
   * those `left_alone`.
   */
  clones?: CloneTarget[];
  /**
   * It deletes or overwrites something (files, tracks, a library, a disk): a dialog should
   * say so.
   */
  destructive: boolean;
  /** The drives (and files on this computer) it reads or writes, its clones included. */
  drives: Touched[];
  /** Applying it changes nothing. */
  noop: boolean;
  /**
   * The RPC method's name without `.plan`, except `undo` for `drive.restore.plan`, `eject` for
   * `drive.eject.plan` and `set` for `prep.plan`.
   */
  operation: string;
  /** Names the plan in `*.apply {"plan_id": ...}`. */
  plan_id: string;
  planned_at: string;
  /** What will change, in a few lines, for a confirmation dialog. */
  summary: string[];
  /** Every warning, the plan's own included. */
  warnings: string[];
};

export type PlannedCopy = {
  class: FileClass;
  hash?: Hash | null;
  mtime: number;
  path: string;
  reason: CopyReason;
  size: number;
  /** Size of the file being replaced. */
  target_size?: number | null;
};

export type PlannedDelete = {
  class: FileClass;
  path: string;
  size: number;
};

export type PlannedFile = {
  bytes: number;
  /** A file is already there (it's replaced only with [`InteropOptions::replace`]). */
  exists: boolean;
  path: string;
  /** What it holds. */
  what: string;
};

export type PlannedPlaylist = {
  create: boolean;
  /** Target path of the playlist. */
  path: string;
  tracks_added: number;
};

export type PlannedTrack = {
  /** Target key of the audio file. */
  audio: string;
  name: string;
  renamed: boolean;
  size: number;
  source_id: number;
};

/** Which of two tempos a [`TempoPrior`] finds more likely. */
export type Plausible = "stored" | "estimate";

/** One track heard in the mix. */
export type Play = {
  /** Other library tracks matching the same stretch of the mix. */
  alternates: Alternate[];
  /** 0-1. */
  confidence: number;
  end: number;
  /** Loops and cue jumps inside the play. */
  jumps: number;
  /** The track's key in Camelot notation (the library may use note names or Open Key). */
  key: string | null;
  /** Whether key lock (master tempo) was on: `None` when the speed is too close to 1.0 to tell. */
  key_lock: boolean | null;
  /** The track's BPM at that speed. */
  played_bpm: number | null;
  /** The key heard: the track's key moved by `semitones`, in Camelot notation. */
  played_key: string | null;
  /**
   * How clearly the track is heard, every [`matcher::PRESENCE_STEP`] seconds from `start` (0-1).
   * Fades and EQ swaps show up as slopes.
   */
  presence: number[];
  presence_step: number;
  /** Pitch change from the speed, in semitones (0 with key lock). */
  semitones: number;
  /** Seconds into the mix where the track is first and last heard. */
  start: number;
  /** Playback speed: 1.0 as recorded, 1.03 is +3%. */
  tempo: number;
  track: Track;
  track_end: number;
  /** Seconds into the track at those points. */
  track_start: number;
  votes: number;
};

export type PlayDelta = {
  after: number;
  before: number;
  /** History lists recorded since that the track appears in. */
  history_lists: number;
  track: TrackRef;
};

/**
 * What the drive adds to a play for following along: the track's overview waveform, hot cues
 * and phrases (and, in the HTML report, its cover art), so a front end needn't ask for each
 * track's analysis.
 */
export type PlayExtras = {
  /**
   * The track's cover art, a JPEG `data:` URL (the HTML report only; `art.get` and `art.path`
   * have it otherwise).
   */
  art?: string | null;
  /** Its hot cues, in seconds into the track. */
  cues: CueMark[];
  /** Its phrases, in seconds into the track. */
  phrases: PhraseSpan[];
  /**
   * The track's overview waveform: up to 400 columns of four bytes (height 0-255, then red,
   * green and blue), in base64.
   */
  wave: string | null;
};

export type PlayStats = {
  history_lists: number;
  /** Most played first. */
  most_played: TrackPlays[];
  /** Play count 0 and in no history list. */
  never_played: TrackRef[];
  /** Tracks with a play count or in a history list. */
  played: number;
  /** With a `since` snapshot: what was played between it and this state. */
  since: PlayedSince | null;
  /** What the statistics are about: a drive, or a snapshot. */
  source: string;
  tracks: number;
};

export type PlayStatsRequest = {
  /** How many of the most played tracks to list (default 20). */
  limit?: number | null;
  /**
   * Also report what was played since this snapshot (a label, `snapshot:<id prefix>` or an id
   * prefix), or since this date (`2026`, `2026-09` or `2026-09-30`: since the newest snapshot
   * of the source's library from before it).
   */
  since?: string | null;
  /**
   * A library (its newest drive plugged in, else its latest snapshot), a drive, or
   * `snapshot:<id prefix>`.
   */
  source: string;
};

export type PlayedSince = {
  created: string;
  /** History lists recorded since. */
  new_histories: string[];
  short_id: string;
  snapshot: Hash;
  /** Tracks played since, most first. */
  tracks: PlayDelta[];
};

export type PlayedTrack = {
  artist: string;
  closed: number;
  name: string;
  opened: number;
  plays: number;
  /** Sets it appears in. */
  sets: number;
  title: string;
  /**
   * A declared set marks it unnamed (`sets edit --unnamed`): the public document never names
   * it.
   */
  unnamed?: boolean;
};

export type PlayerIdentity = {
  drive: string;
  /** From OneLibrary (`None`: the drive has no readable OneLibrary database). */
  onelibrary: DbIdentity | null;
  /** rekordbox's own record, if rekordbox set the drive up. */
  rekordbox: RekordboxDevice | null;
};

/** What setting the name and colour players show would change. */
export type PlayerPlan = {
  colour: number | null;
  drive: string;
  name: string | null;
  /** What players show now. */
  now: PlayerIdentity;
  root: string;
};

/** Where a drive's player models came from. */
export type PlayersFrom =
  /** `--players`. */
  | "given"
  /** Remembered for the drive's library. */
  | "library"
  /** Remembered for every library. */
  | "default"
  /** None given or remembered. */
  | "none";

/** What remembering (or forgetting) player models would change. */
export type PlayersPlan = {
  /** The models remembered now. */
  from: string[];
  /**
   * The library they're for; `None` for the default (every library without models of its
   * own).
   */
  library: string | null;
  library_name: string | null;
  /** The models remembered after (empty: forgotten). */
  to: string[];
};

export type Playlist = {
  id: number;
  is_folder: boolean;
  name: string;
  /** `None` for top-level nodes. */
  parent_id: number | null;
  sort_order: number;
  /** Track ids in playlist order (empty for folders). */
  track_ids: number[];
};

/** A playlist whose contents change. */
export type PlaylistChange = {
  added: number;
  after: number;
  before: number;
  path: string;
  removed: number;
  /** Same tracks, different order. */
  reordered: boolean;
};

export type PlaylistChanges = {
  /** Entries only on the right, at their (1-based) position there. */
  added: DiffEntry[];
  moved: MoveOutput[];
  path: string;
  /** Entries only on the left, at their position there. */
  removed: DiffEntry[];
};

export type PlaylistDiff = {
  /** Playlists (and folders, ending in `/`) only on the right. */
  added_playlists: string[];
  /** Tracks (in the collection) only on the right. */
  added_tracks: TrackRef[];
  /** Playlists on both sides whose entries differ. */
  changed_playlists: PlaylistChanges[];
  /** Tracks on both sides whose details differ. */
  changed_tracks: TrackChange[];
  left: string;
  /** Playlists (and folders) only on the left. */
  removed_playlists: string[];
  /** Tracks only on the left. */
  removed_tracks: TrackRef[];
  right: string;
  /** Playlists on both sides with the same entries in the same order. */
  unchanged_playlists: number;
};

export type PlaylistMode =
  /** Recreate the source's playlist tree inside a new folder (default). */
  | "folder"
  /** Merge into playlists with the same folder path, creating missing ones. */
  | "by_name"
  /** Only add tracks. */
  | "none";

/** A playlist or folder in [`PlaylistTree`]. */
export type PlaylistNode = {
  /** A folder's contents, in display order. */
  children?: PlaylistNode[];
  /** The entries' total length. */
  duration_secs: number;
  folder: boolean;
  id: number;
  name: string;
  /** Through its folders, e.g. `Sets/2026/Warmup`. */
  path: string;
  /** Entries in a playlist (a track can be in it twice); 0 for a folder. */
  tracks: number;
};

export type PlaylistRestorePreview = {
  /** Entries put back. */
  entries: number;
  folder: boolean;
  /** Entries left out because their track isn't on the drive any more. */
  missing: MissingEntry[];
  /** The playlist or folder's path, as the source has it (and as it will be on the drive). */
  path: string;
  plan: EditPlan;
  /** Playlists restored: one, or every playlist in the folder. */
  playlists: number;
  source: RestoreSource;
  /** Playlists in the folder on the drive that the source doesn't have: left as they are. */
  untouched: string[];
};

export type PlaylistRestoreRequest = {
  /** Take it from one of the drive's backups instead (see [`super::list_backups`]). */
  backup?: string | null;
  drive: string;
  /**
   * The playlist or folder: its path (`Folder/Name`), or its name when that's unique, as the
   * snapshot or backup has it.
   */
  playlist: string;
  /**
   * The snapshot to take it from: an id prefix or a label [default: the newest snapshot of the
   * drive's library that has it].
   */
  snapshot?: string | null;
};

/** The playlist tree without the tracks: what a sidebar shows. */
export type PlaylistTree = {
  /** The history lists players recorded (in folders, on OneLibrary). */
  history: PlaylistNode[];
  playlists: PlaylistNode[];
};

export type Precision =
  | "exact"
  /** Counts as "at least" round numbers, shares to 5%, BPM to whole numbers. */
  | "rounded";

/** Changes to [`ProfilePrefs`], applied in field order after `reset`. */
export type PrefsUpdate = {
  alias?: { [key: string]: string };
  /** Go back to the default sections. */
  default_sections?: boolean;
  exclude?: ProfileSection[];
  /** The DJ's own headline (empty: back to the generated one). */
  headline?: string | null;
  hide?: string[];
  hide_artist?: string[];
  include?: ProfileSection[];
  pin?: string[];
  precision?: Precision | null;
  /** Start again from the defaults. */
  reset?: boolean;
  /** Publish exactly these sections. */
  sections?: ProfileSection[] | null;
  unalias?: string[];
  unhide?: string[];
  unhide_artist?: string[];
  unpin?: string[];
};

export type PrepSection = {
  hot_cue_share: number;
  /** Hot cues per track, on tracks that have any. */
  hot_cues_per_track: number | null;
  loop_share: number;
  memory_cue_share: number;
  /** Collection tracks whose cues were read. */
  tracks: number;
};

export type ProfileDocument = {
  arc?: DocArc | null;
  /** A track credited to several artists counts for each, so shares can add up to more than 1. */
  artists?: DocShare[] | null;
  /** `plays`, `sessions` or `collection` (see [`Basis`]). */
  basis: Basis;
  collection?: DocCollection | null;
  energy?: DocEnergy | null;
  era?: DocEra | null;
  generatedAt: string;
  genres?: DocShare[] | null;
  headline?: string | null;
  keys?: DocKeys | null;
  labels?: DocShare[] | null;
  mixing?: DocMixing | null;
  precision: Precision;
  prep?: DocPrep | null;
  sets?: DocSets | null;
  signature?: DocTrack[] | null;
  /** Setting key → value, e.g. `{"quantize": "off", "jogMode": "vinyl"}`. */
  style?: { [key: string]: string } | null;
  tags?: string[];
  tempo?: DocTempo | null;
  version: number;
};

export type ProfileOptions = {
  /** Read analysis files for phrase mood and cue statistics (one or two small files per track). */
  analysis: boolean;
  /** The DJ's genre spellings, e.g. `{"dnb": "Drum & Bass"}` (see [`ProfilePrefs`]). */
  genre_aliases: { [key: string]: string };
  /** Without declared sets: sessions with fewer tracks (sound checks, previews) don't count. */
  min_set_tracks: number;
  /**
   * Only sets played on or after this day. Sessions (without declared sets) count from the day
   * a pull first archived them; those still only on a drive are recent, so they always count.
   */
  since: string | null;
  /** How many entries ranked lists keep. */
  top: number;
  /** Only sets played on or before this day (sessions still only on a drive don't count). */
  until: string | null;
};

/** The DJ's choices about their profile, kept in the store. */
export type ProfilePrefs = {
  /** The DJ's genre spellings: genre text as tagged → genre as shown. */
  genre_aliases: { [key: string]: string };
  /** The DJ's own one-line description, instead of the generated one. */
  headline?: string | null;
  /** Tracks never named (unreleased promos, edits, dubplates). They still count in the numbers. */
  hidden: string[];
  /** Artists never named, wherever they're credited. */
  hidden_artists: string[];
  /** Tracks ("Artist - Title") to list first among signature tracks, in this order. */
  pinned: string[];
  precision: Precision;
  /**
   * Sections the document includes (`None`: the defaults, see
   * [`Section::published_by_default`]).
   */
  sections?: ProfileSection[] | null;
};

export type ProfileReport = {
  arc: ArcSection;
  artists: RankedSection;
  basis: Basis;
  collection: CollectionSection;
  /** `None` without analysis files. */
  energy: EnergySection | null;
  era: EraSection;
  generated: string;
  genres: RankedSection;
  keys: KeysSection;
  labels: RankedSection;
  mixing: MixingSection;
  options: ProfileOptions;
  /** `None` without analysis files. */
  prep: PrepSection | null;
  sets: SetsSection;
  signature: SignatureSection;
  sources: SourceInfo[];
  /** `None` when no drive has My Settings files and the store has no settings profile. */
  style: StyleSection | null;
  summary: ProfileSummary;
  tempo: TempoSection;
  warnings: string[];
};

export type ProfileRequest = {
  /**
   * Drives to read. Default: the mounted rekordbox drives in the store's registry, so a drive
   * someone else plugged in doesn't count as the DJ's; every mounted rekordbox drive when the
   * registry is empty.
   */
  drives?: string[];
  /**
   * Only these store libraries (names or ids). Default: the libraries the named drives are
   * linked to, or every library.
   */
  libraries?: string[];
  /** Without declared sets: sessions with fewer tracks don't count (default 5). */
  min_set_tracks?: number | null;
  /** Skip analysis files: no phrase mood or cue statistics, but much faster. */
  no_analysis?: boolean;
  /** Leave the store out (its sets, archived history lists and libraries). */
  no_store?: boolean;
  /**
   * Only sets played from this day: `2026`, `2026-09` or `2026-09-30`. Without declared sets,
   * sessions first archived from it.
   */
  since?: string | null;
  /** Entries in ranked lists (default 10). */
  top?: number | null;
  /** Only sets played up to this day (same forms). */
  until?: string | null;
};

export type ProfileSection = "summary" | "sets" | "tempo" | "arc" | "mixing" | "keys" | "energy" | "genres" | "labels" | "artists" | "era" | "signature" | "prep" | "collection" | "style";

export type ProfileSummary = {
  headline: string;
  tags: Tag[];
};

export type ProfileView = {
  /** What a public profile would show. */
  document: ProfileDocument;
  prefs: ProfilePrefs;
  /** Sections the document includes. */
  published: ProfileSection[];
  /** Everything worked out (stays on this computer). */
  report: ProfileReport;
};

/**
 * What happened, which drive and step it's about, and (for a coalesced `item`) how many files
 * it stands for.
 */
export type ProgressEvent = {
  /** How many files an `item` stands for, when more than one (count `items ?? 1`). */
  items?: number | null;
  step?: Step | null;
  target?: string | null;
} & (
  | Event
  /** A step begins (`step` says which). */
  | StepStarted);

/** A `progress` notification's parameters. */
export type ProgressParams = {
  event: ProgressEvent;
  /** The request it's about. */
  id: RequestId | null;
};

/** [`provision_apply`] with a request (rather than a plan id). */
export type ProvisionApply = {
  /** Test the disk's real capacity first (see [`capacity`]), and stop if it is fake. */
  check_capacity?: boolean;
  /** Bytes per cluster. Default: [`Filesystem::default_cluster_size`]. */
  cluster_size?: number | null;
  eject?: boolean;
  /** Default: FAT32, which every player reads. */
  filesystem?: Filesystem | null;
  /** Erase and format it first. */
  format?: boolean;
  /** The plan's `format.identity`, when formatting: the disk the user confirmed. */
  format_identity?: string | null;
  /** Volume label. Default: the drive's locallibrary name, else its current label. */
  label?: string | null;
  /** The library to push (without `like`) [default: the only library]. */
  library?: string | null;
  /** A drive to copy the library from [default: push the library's latest snapshot]. */
  like?: string | null;
  /** locallibrary name for the new drive. */
  name?: string | null;
  player_colour?: number | null;
  /** Device Name and colour players show. */
  player_name?: string | null;
  role?: string | null;
  /** Write the settings profile [default: yes]. */
  settings?: boolean;
  /** The new drive: a mounted volume, or (with `format`) a disk such as `disk4`. */
  target: string;
  /** Read everything back afterwards [default: yes]. */
  verify?: boolean;
};

export type ProvisionPlan = {
  /** The format's plan, when formatting (its `identity` confirms the disk on apply). */
  format: FormatPlan | null;
  steps: string[];
  warnings: string[];
};

export type ProvisionReport = {
  copy: SyncReport | null;
  drive: DeviceInfo | null;
  eject: EjectReport | null;
  format: FormatReport | null;
  player: PlayerIdentity | null;
  settings: ApplyReport | null;
  /** Where it stopped, when a step failed (the steps before it stay done). */
  stopped: StepError | null;
  verify: ScrubReport | null;
};

export type ProvisionRequest = {
  /** Test the disk's real capacity first (see [`capacity`]), and stop if it is fake. */
  check_capacity?: boolean;
  /** Bytes per cluster. Default: [`Filesystem::default_cluster_size`]. */
  cluster_size?: number | null;
  eject?: boolean;
  /** Default: FAT32, which every player reads. */
  filesystem?: Filesystem | null;
  /** Erase and format it first. */
  format?: boolean;
  /** Volume label. Default: the drive's locallibrary name, else its current label. */
  label?: string | null;
  /** The library to push (without `like`) [default: the only library]. */
  library?: string | null;
  /** A drive to copy the library from [default: push the library's latest snapshot]. */
  like?: string | null;
  /** locallibrary name for the new drive. */
  name?: string | null;
  player_colour?: number | null;
  /** Device Name and colour players show. */
  player_name?: string | null;
  role?: string | null;
  /** Write the settings profile [default: yes]. */
  settings?: boolean;
  /** The new drive: a mounted volume, or (with `format`) a disk such as `disk4`. */
  target: string;
  /** Read everything back afterwards [default: yes]. */
  verify?: boolean;
};

export type PruneOptions = {
  /** Say what would go, and remove nothing (for callers that plan and prune in one go). */
  dry_run?: boolean;
  /** Keep the newest snapshot of each of the last N days that have one. */
  keep_daily?: number | null;
  /** Keep each library's newest N snapshots. */
  keep_last?: number | null;
  /** Keep the newest snapshot of each of the last N weeks that have one. */
  keep_weekly?: number | null;
  /** Only this library (name or id) [default: every library]. */
  library?: string | null;
};

export type PruneReport = {
  dry_run: boolean;
  freed_bytes: number;
  libraries: LibraryPrune[];
  /**
   * Stored files removed: no snapshot left uses them. With `dry_run`, the ones that would
   * be.
   */
  removed_objects: number;
  removed_snapshots: number;
  warnings: string[];
};

/** A record ready to publish, and what's published in its place now. */
export type PublishPlan = {
  /** The account it goes to. `None`: nobody's signed in, so it can only be looked at. */
  account: AccountInfo | null;
  /** Whether publishing changes anything (time stamps aside). */
  changed: boolean;
  collection: string;
  /** What's published there now (`None`: nothing yet). */
  current: Fetched | null;
  /**
   * The scope the session lacks to write the record: sign in again allowing publishing
   * (`account.login.begin` with `publish: true`) before applying.
   */
  missing_scope: string | null;
  /** What the DJ should know before publishing, in words: guesses made, things left out. */
  notes: string[];
  /** The record exactly as it will be written. */
  record: unknown;
  rkey: string;
  /** `at://did/collection/rkey` (`None` while signed out). */
  uri: string | null;
};

/** A record published. */
export type Published = {
  /** Its content hash. */
  cid: string | null;
  record: unknown;
  uri: string;
  /** False when what's published was already the same, so nothing was written. */
  written: boolean;
};

/** The settings a published settings record holds, as a settings profile's base values. */
export type PublishedSettings = {
  /**
   * `group.field = value` entries this version of locallibrary doesn't know (newer firmware,
   * or a newer locallibrary), left out.
   */
  skipped: string[];
  updated_at: string | null;
  values: { [key: string]: string };
};

/** What saving a drive in the store (`pull`) would do. */
export type PullPlan = {
  drive: string;
  /** The library's latest snapshot. */
  head: SnapshotInfo | null;
  /** The library it's saved into. */
  library: string;
  /** The store has no such library yet (or no store at all): pulling starts it. */
  new_library: boolean;
  /**
   * The library's latest snapshot (left) against the drive (right): `right_newer` means the
   * drive holds changes the store lacks, `identical` that it's saved already. `None` when the
   * library has no snapshot yet.
   */
  relation: Relation | null;
  root: string;
};

export type PullReport = {
  imported_bytes: number;
  imported_files: number;
  library: string;
  library_id: string;
  reused_files: number;
  snapshot: SnapshotInfo | null;
  up_to_date: boolean;
};

export type PullRequest = {
  drive: string;
  force?: boolean;
  /**
   * Label the snapshot (see [`Store::add_label`]); when the drive is already saved, the label
   * goes on the snapshot it matches.
   */
  label?: string | null;
  /** The library to pull into [default: the drive's, else a new one named after the drive]. */
  library?: string | null;
  message?: string | null;
  /**
   * Start a new library when no library has the name `library` gives. Without it, an unknown
   * name is refused, so a typo doesn't start a new library.
   */
  new_library?: boolean;
};

export type PushPreview = {
  plan: SyncPlan;
  snapshot: SnapshotInfo;
};

export type PushRequest = {
  delete_first?: boolean;
  drive: string;
  force?: boolean;
  keep_extra?: boolean;
  library?: string | null;
  snapshot?: string | null;
  verify?: boolean;
};

export type RankedSection = {
  distinct: number;
  /** Plays (or tracks) with a value. */
  known: number;
  top: Share[];
};

/**
 * Why an error needs a decision, or what would resolve it: a stable code a front end can switch
 * on (`data.reason` over RPC), next to the error's [`Error::code`].
 */
export type Reason =
  /** Which of two drives is newer can't be told with confidence: choose a direction. */
  | "uncertain"
  /**
   * Both drives changed since they were last in sync: merge them, or choose a direction (the
   * other side's changes are lost).
   */
  | "diverged"
  /** The drives hold different libraries: merge them, or choose a direction to overwrite one. */
  | "unrelated"
  /**
   * A drive came back from a set with changes players made: sync it as a pair first, which
   * keeps them in the store.
   */
  | "back_from_set"
  /**
   * What would be overwritten (a library state, history lists) isn't saved in the store:
   * pull the drive first, or force it.
   */
  | "not_saved"
  /**
   * The drive holds an older state of the library than the store: push to update it, or
   * force the pull to record it as the newest.
   */
  | "older_than_store"
  /**
   * The drive's library has diverged from the store's: force the pull, pull into another
   * library, or merge.
   */
  | "diverged_from_store"
  /** The drive is retired (`fields.drive`): bring it back, or force the write. */
  | "retired"
  /** The drive is an archive drive, kept as it is: force the write. */
  | "archive"
  /**
   * Something by that name, or at that path, is there already (`fields.name` or
   * `fields.path`).
   */
  | "exists"
  /** Files of the drive's own are where the library would go (`fields.paths`): move them. */
  | "in_the_way"
  /** A backup lists audio that's no longer on the drive (`fields.missing`): force it. */
  | "missing_audio"
  /**
   * The drive changed since the change a backup takes back (`fields.changed`), or the backup
   * doesn't record what its change left, so that can't be told (`fields.changed` empty):
   * putting it back would take that back too. Force it.
   */
  | "changed_since"
  /** The drive already has its own identity: re-identify it to replace it. */
  | "has_identity"
  /** Another mounted drive holds that identity. */
  | "identity_in_use"
  /** An interrupted operation must be finished first. */
  | "interrupted"
  /** The copy holds a different store. */
  | "different_store"
  /**
   * The copy has snapshots this store doesn't, or changed since it was last copied to: force
   * it to replace them.
   */
  | "copy_ahead"
  /** The label is on another snapshot: take it off first. */
  | "label_taken"
  /** The snapshot is a library's latest or labelled, and stays. */
  | "snapshot_kept"
  /** The update file has problems (`fields.problems`): force it. */
  | "firmware_problems"
  /** The drive is fake: it holds less than it claims. */
  | "fake_capacity"
  /** What it works on changed while it worked: run it again. */
  | "changed"
  /** Replaying recorded edits stopped part way: undo takes back what it made. */
  | "replay_stopped"
  /** Files didn't read back correctly. */
  | "verify_failed"
  /** Several libraries could be meant: name one (`fields.libraries`). */
  | "choose_library"
  /** Several drives could be meant: name one (`fields.drives`). */
  | "choose_drive"
  /** No library has that name (`fields.library`): start one, or name another. */
  | "unknown_library"
  /** It needs the store, and there's none (or it isn't given). */
  | "no_store"
  /** A parameter it needs is missing (`fields.param`). */
  | "missing_param"
  /** The drive has no OneLibrary database (`exportLibrary.db`): convert to add one. */
  | "no_one_library"
  /** The drive has no rekordbox export. */
  | "no_export"
  /**
   * rekordbox's collection can't be decrypted with the key it has (`fields.env` names the
   * variable that gives another).
   */
  | "wrong_key"
  /** No setting has that key. */
  | "unknown_setting"
  /** The settings profile is empty: import or set some first. */
  | "empty_profile"
  /** It needs administrator rights: run it again elevated. */
  | "needs_admin"
  /** Not signed in to an Atmosphere account. */
  | "not_signed_in"
  /** The session ended or was revoked: sign in again. */
  | "session_ended"
  /** The session may not publish: sign in again allowing it. */
  | "missing_scope"
  /** The system keychain isn't available: keep the session in a file instead. */
  | "no_keychain";

/** What an unadopted (or re-identified) drive matches. */
export type Recognition = {
  /** A registered drive last seen holding exactly this state. */
  drive_by_content: RegisteredRef | null;
  /** A registered drive last seen on this disk (same serial number). */
  drive_by_hardware: RegisteredRef | null;
  /** Its library state is a stored snapshot. */
  snapshot: SnapshotMatch | null;
};

export type Recording = {
  /**
   * Length of the audio, from the WAV header (or the file size when recording was cut off
   * before the header was finalised).
   */
  duration_secs: number | null;
  format: WavFormat | null;
  /** When the file was last written: the end of the recording. */
  modified: string;
  /** Relative to the drive root. */
  path: string;
  size: number;
  started: string | null;
};

/** Where a file's expected content came from. */
export type ReferenceKind =
  /** A snapshot of exactly this library state. */
  | "snapshot"
  /** The drive's hash cache. */
  | "cache"
  /** An older snapshot holding the same version of the file. */
  | "history";

/** After rekordbox exported to a drive: put back what the export replaced, then save it. */
export type RefreshRequest = {
  /** The drive rekordbox exported to [default: see [`refresh_drive`]]. */
  drive?: string | null;
  /** Steps to leave out (`clean`, `replay`, `smart`, `player`, `settings`, `pull`, `sync`). */
  skip?: StepKind[];
  /** Then copy the drive onto the other mounted drives of its library, when it's the newest. */
  sync?: boolean;
};

export type RegisteredRef = {
  id: string;
  last_seen: string;
  name: string;
};

export type Reidentify = {
  drive: string;
  /**
   * Give it a new identity though nothing shows it's a copy (no other drive mounted has its
   * identity, and it's on the disk recorded with it).
   */
  force?: boolean;
  name?: string | null;
};

/** What giving a copied drive an identity of its own would do. */
export type ReidentifyPlan = {
  drive: string;
  /** The identity it shares with the drive it was copied from, which keeps it. */
  id: string;
  /** The name it gets. */
  name: string;
  root: string;
};

/** rekordbox's record of the device. */
export type RekordboxDevice = {
  background_color: string;
  background_color_libplus: string;
  backup_date: string;
  backup_pc_name: string;
  device_filesystem: string;
  device_id: string;
  device_name: string;
};

/**
 * rekordbox's automatic playlist sync to a drive (`playlists3.sync` / `playlists3Plus.sync`):
 * which playlists of its collection it keeps up to date there. Playlists locallibrary adds
 * aren't in it.
 */
export type RekordboxSync = {
  automatic: boolean;
  /** rekordbox collection the playlists come from (OneLibrary's `masterDbId`). */
  collection: number | null;
  /** `device_library` (`export.pdb`) or `onelibrary`. */
  library: string;
  /** Playlists and folders rekordbox tracks. */
  playlists: number;
};

export type Relation =
  | ("left_newer" | "right_newer")
  /** Same content. */
  | "identical"
  /** Same library, but both sides changed independently. */
  | "diverged"
  /** Different libraries (little overlap): merge rather than sync. */
  | "unrelated"
  /** Same library, no way to tell which is newer. */
  | "undetermined";

/** A file a missing track could be. */
export type RelocateCandidate = {
  /** Seconds, when its headers say. */
  duration_secs: number | null;
  on_drive: boolean;
  /** A key on the drive, or a path on this computer. */
  path: string;
  size: number;
};

export type RelocatePlan = {
  ambiguous: Ambiguous[];
  /** Tracks whose file is copied to the path they have. */
  copied: Relocation[];
  /** Audio copied onto the drive. */
  copy_bytes: number;
  drive: string;
  fits: boolean;
  free_space: number | null;
  /** Space needed for that, the backup and the new databases. */
  needed_bytes: number;
  /** Missing tracks with no file of their name. */
  not_found: TrackRef[];
  /** Tracks pointed at their file where it is on the drive. */
  repointed: Relocation[];
  root: string;
  /** Tracks whose files of their name are none of them as long as the track: none picked. */
  unfit: Ambiguous[];
  warnings: string[];
};

export type RelocateRequest = {
  drive: string;
  /**
   * Folders to look in besides the drive (on this computer or another drive), with
   * everything below them.
   */
  folders?: string[];
};

export type Relocation = {
  file: RelocateCandidate;
  track: TrackRef;
};

/** The player models `check` uses when none are given. */
export type RememberedPlayers = {
  /** For drives of a library without models of its own. */
  default: string[];
  libraries: LibraryPlayers[];
};

export type RemoveSmartPlaylist = {
  path: string;
};

export type RenameDrive = {
  drive: string;
  /** Also change the volume label. */
  label?: boolean;
  name: string;
};

export type RenameLibrary = {
  /** The library's name (any case) or id. */
  library: string;
  name: string;
};

/** What renaming a drive would do. */
export type RenamePlan = {
  drive: string;
  from: string;
  /** The volume label it gets, with `label`. */
  label: string | null;
  root: string;
  to: string;
};

export type RenameReport = {
  id: string;
  /** The volume label, when it was changed too. */
  label: string | null;
  name: string;
  old_name: string;
};

export type RepairReport = {
  bytes: number;
  drive: string;
  /** Files that still differ after being rewritten: the flash itself is failing. */
  failed: Failure[];
  repaired: string[];
  /** Files that were fine by the time of the repair, or had no good copy. */
  skipped: Skip[];
};

export type RepairRequest = {
  against?: string[];
  drive: string;
  /** The problems a [`drive_verify`] found (each is checked again before anything is written). */
  problems: FileProblem[];
};

/** Where a damaged or missing file can be rewritten from. */
export type RepairSource = {
  from: "store";
} | {
  from: "drive";
  name: string;
  root: string;
};

/** Everything an import preview shows: the format read, the counts, and every entry's result. */
export type Report = {
  entries: Matched[];
  format: ImportFormat;
  summary: ImportSummary;
};

export type ReportHtml = {
  /** The report's id (`mix.reports.list`). */
  id: string;
  /** Also answer the page itself (otherwise just where it is). */
  inline?: boolean;
};

/** Where a report's interactive page was written. */
export type ReportPage = {
  bytes: number;
  /** The page, when asked for with `inline`. */
  html?: string | null;
  id: string;
  /** `<id>.html` in the store's reports folder: open it in a browser or a webview. */
  path: string;
};

export type ReportRef = {
  /**
   * Also each play's waveform, hot cues and phrases, from the drive the mix was matched
   * against when it's mounted.
   */
  extras?: boolean;
  /** The report's id (`mix.reports.list`). */
  id: string;
};

/**
 * A JSON-RPC request's id: a string or a number. Only describes ids in the schema: they're
 * passed through as they came.
 */
export type RequestId = number | string;

/** An account found from its handle or DID. */
export type Resolved = {
  did: string;
  /** When it was found by handle. */
  handle: string | null;
  /** Its data server. */
  pds: string;
};

export type RestoreBackup = {
  backup?: string | null;
  drive: string;
  /** Restore even if the backup references files that are gone. */
  force?: boolean;
};

export type RestorePreview = {
  backup: BackupInfo;
  /** From the drive as it is to the drive restored. */
  diff: PlaylistDiff;
  /**
   * Tracks in the backup whose audio is neither on the drive nor in the backup: restoring
   * refuses them unless forced.
   */
  missing_audio: string[];
};

/** Where the restored playlist comes from. */
export type RestoreSource = {
  kind: "snapshot";
  library: string | null;
  snapshot: SnapshotInfo;
} | {
  backup: BackupInfo;
  kind: "backup";
};

export type RetireDrive = {
  drive: string;
  undo?: boolean;
};

/** What a drive is for. */
export type Role =
  | "backup"
  /** Where rekordbox exports go: the source for the rest of its group. */
  | "primary"
  /** Taken out to play sets (`gig`, its old name, is read too). */
  | "set"
  /** Kept as it is: never overwritten by a sync unless forced. */
  | "archive";

/** A report kept in the store's `reports` folder. */
export type SavedReport = {
  analysed_at: string | null;
  artist: string | null;
  /** Fraction of the mix identified. */
  coverage: number | null;
  /** The drive whose library it was matched against. */
  drive: string | null;
  duration_secs: number | null;
  /** The portable export (`<id>.mix.json`), for Local Network. */
  export: string | null;
  /** The interactive page (`<id>.html`). */
  html: string | null;
  /**
   * Names it in `mix.reports.get` and `mix.reports.html`: its files' name, the title's slug
   * and a hash of the recording's path (see [`id`]).
   */
  id: string;
  /** The recording it was made from. */
  mix: string | null;
  /** When one of its files last changed. */
  modified: string;
  /** The report as data (`<id>.json`): `mix.reports.get` reads it. */
  report: string | null;
  /** The mix's title (from its file name when only the HTML page is there). */
  title: string;
  /** Tracks found. */
  tracks: number | null;
};

export type Scheme =
  | "other"
  /** Master Boot Record ("FDisk", "dos"). */
  | "mbr"
  /** GUID partition table. */
  | "gpt"
  /** Apple partition map. */
  | "apm";

export type ScrubReport = {
  bytes: number;
  damaged: FileProblemOutput[];
  drive: string;
  files: number;
  missing: FileProblemOutput[];
  playlists: string[];
  root: string;
  /** The snapshot of exactly this state, when there is one. */
  snapshot: Hash | null;
  /** Tracks whose audio, analysis or artwork is affected. */
  tracks: TrackRef[];
  unreadable: FileProblemOutput[];
  /**
   * Files read without error but with no reference (changed since locallibrary last saw
   * them, or never seen).
   */
  unverified: number;
  /** Files compared with a reference and found intact. */
  verified: number;
};

/**
 * A session: one run of play logging. A history list a player recorded, or a log imported from
 * another tool ([`ImportedSession`]).
 */
export type Session = {
  /** The DJ said it wasn't a set. */
  dismissed: boolean;
  /** The mounted drives holding it now. */
  drives: string[];
  /** The drive whose snapshot first held it. */
  first_seen_on?: string | null;
  /**
   * The list's fingerprint ([`crate::plays::HistoryList::fingerprint`]): the same wherever
   * it's read. Commands take any unique prefix of 4 or more characters.
   */
  id: string;
  /** Entries whose track isn't in the library any more. */
  missing: number;
  name: string;
  /** Folders and name. */
  path: string;
  /** Sets built from it, by id. */
  sets: string[];
  /**
   * `device_library` (`export.pdb`), `onelibrary`, or the tool a log was imported from
   * (`beat_link_trigger`).
   */
  source: string;
  /** In play order; `start_seconds` is set for timed logs. */
  tracks: SetEntry[];
  window: Window;
};

export type SessionsRequest = {
  /**
   * Only this drive's sessions and those of its library in the store. Default: every mounted
   * rekordbox drive's and the whole store's.
   */
  drive?: string | null;
  /** Only the sessions in no set that weren't dismissed (`sessions.dismiss`). */
  unassigned?: boolean;
};

/** A performance: what the DJ played, where and when. */
export type Set = {
  city?: string | null;
  /** ISO 3166 country code, `GB`. */
  country?: string | null;
  created_at: string;
  /** The day it was played (local). */
  date: string;
  /** The event it was part of: the `at://` URI of a `community.lexicon.calendar.event` record. */
  event?: string | null;
  /**
   * A TID ([`new_id`]): the record key when it's published, and how commands name it (any
   * unique prefix of 4 or more characters).
   */
  id: string;
  /**
   * `club`, `festival`, `radio`, `livestream`, `mix` (the Lexicon's `knownValues`) or the DJ's
   * own word.
   */
  kind?: string | null;
  /** Where people can listen to it (Mixcloud, SoundCloud, YouTube...). */
  link?: string | null;
  /** The DJ's own notes. Never published. */
  notes?: string | null;
  /** The recording on this computer the tracklist was found in. */
  recording?: SetRecording | null;
  /** The sessions it was built from, by id ([`Session::id`]). */
  sessions?: string[];
  title?: string | null;
  /** In play order. */
  tracks: SetEntry[];
  updated_at: string;
  venue?: string | null;
  /** Who they played with (back to back). */
  with?: Partner[];
};

/**
 * Which tracks an operation targets: the tracks that fit every field given, so a playlist and a
 * filter together pick the playlist's tracks the filter matches. At least one field must be set,
 * and a blank query, filter, album, playlist or history list is refused rather than taken to
 * mean every track.
 */
export type SetArtwork = {
  /** Every track whose album has this name. */
  album?: string | null;
  drive: string;
  /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
  filter?: string | null;
  /** The tracks of a history list (what a player recorded), by name. */
  history?: string | null;
  ids?: number[];
  /** Image file, or an audio file whose embedded art to use. */
  image: string;
  /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
  paths?: string[];
  playlist?: string | null;
  /** Matches "artist - title" or path substrings; must match at least one track. */
  query?: string | null;
};

/**
 * A set as it was saved, and what's worth telling the DJ about it (tentative matches, tracks
 * logged but not heard in the recording, a day to check, how inserted tracks were found).
 */
export type SetChange = {
  notes: string[];
  set: Set;
};

export type SetContact = {
  contact: string;
  drive: string;
  /** Also add a top-level playlist named after it (shown first on the player). */
  playlist?: boolean;
};

/**
 * Add a cue, or replace the hot cue in its slot (or the memory cue at its position). A replaced
 * cue keeps its name and colour unless new ones are given.
 */
export type SetCue = {
  at: CuePosition;
  /** See [`set_color`]. */
  color?: string | null;
  /** The hot cue slot; a memory cue when absent. */
  hot_cue?: HotCue | null;
  /** Make it a loop this many beats long (a whole number, or 0.5, 0.25...). */
  loop_beats?: number | null;
  name?: string | null;
  /** Move a time to the nearest beat of the grid. */
  snap?: boolean;
};

/**
 * One track of a set, kept as it was when it went in, so the set outlives library changes and
 * reads without the drive.
 */
export type SetEntry = {
  album?: string;
  artist: string;
  /** 0 when unknown. */
  bpm: number;
  duration_secs: number;
  /** The audio file's path in the library (`/Contents/...`), to find the track again. */
  file?: string | null;
  genre?: string;
  isrc?: string;
  /** As the library has it (note name, Camelot or Open Key). */
  key?: string;
  label?: string;
  origin: EntryOrigin;
  /** The session that logged it. */
  session?: string | null;
  /** Seconds from the start of the set (from a recording or a timed log). */
  start_seconds?: number | null;
  title: string;
  /** Published without naming it: only its place in the set. */
  unnamed?: boolean;
  year: number;
};

export type SetExport = {
  /** For `m3u8`: the drive the tracks are on (default: the only mounted rekordbox drive). */
  drive?: string | null;
  format: SetFormat;
  /**
   * Write it to this file (otherwise it's answered as `text`). An M3U8's paths are relative
   * to it.
   */
  output?: string | null;
  /** Replace a file already at `output` (refused otherwise). */
  replace?: boolean;
  set: string;
};

export type SetExportReport = {
  format: SetFormat;
  path?: string | null;
  /** The document, when it wasn't written to a file. */
  text?: string | null;
  /** Tracks written. */
  tracks: number;
  /** Rules of the format the set breaks, and tracks left out. */
  warnings: string[];
};

/** How a set is written out. */
export type SetFormat =
  /** `01. Artist - Title`. */
  | "text"
  /** Mixcloud's tracklist paste: `Artist - Title - 00:04:12`, with its rules checked. */
  | "mixcloud"
  /** A CUE sheet for the recording. */
  | "cue"
  /** Chapters for a YouTube description: `04:12 Artist - Title`. */
  | "youtube"
  /**
   * For 1001Tracklists: `[0:04:12] Artist - Title`. Unconfirmed: their site documents no
   * paste format, so this is the cue-time style tracklists are usually written in.
   */
  | "1001"
  /** `#,Start,Artist,Title,Album,Label,Genre,BPM,Key,ISRC,Year,Unnamed`. */
  | "csv"
  /** A playlist of the tracks on a drive. */
  | "m3u8";

export type SetInfo = {
  closing_bpm: number | null;
  /**
   * The day the set was played. A session has no such day: this is the day a pull first
   * archived it (it was played then or before), and none while it's only on a drive.
   */
  date: string | null;
  /** Share of transitions between keyed tracks that are harmonic. */
  harmonic_share: number | null;
  /**
   * The set's id, or the session's (its history list's fingerprint, see
   * [`crate::sets::Session::id`]).
   */
  id: string;
  /** The set's title, else its venue; the session's history list (folders and name). */
  name: string;
  opening_bpm: number | null;
  /**
   * Where: the set's venue and city; the drive a session was read from, or whose snapshot
   * archived it.
   */
  place: string;
  tracks: number;
};

/** Getting drives ready for a set. */
export type SetJobRequest = {
  /** Also decode the end of every audio file (slow). */
  audio?: boolean;
  /**
   * The drives you're taking, or a library's name for its mounted clones [default: the mounted
   * drives of one library, see [`drive_group`]].
   */
  drives?: string[];
  /** Eject the drives once everything passed [default: yes]. */
  eject?: boolean;
  /** The library whose mounted drives to take, when `drives` is empty. */
  library?: string | null;
  /** Player models to check against [default: the ones remembered, see `players set`]. */
  players?: string[];
};

export type SetPlayerIdentity = {
  /** rekordbox's background colour code, 0 (none) to 8. */
  colour?: number | null;
  drive: string;
  name?: string | null;
};

export type SetPlayers = {
  /** The library (name or id) they're for; without one, every library without models of its own. */
  library?: string | null;
  /** Model names (`CDJ-3000X`, `cdj-2000nxs2`...); stored as the table spells them. */
  models: string[];
};

export type SetPlaylist = {
  drive: string;
  /** The playlist's path (default: `Sets/<date> <title or venue>`). */
  name?: string | null;
  set: string;
};

export type SetPublishRequest = {
  /**
   * Positions (from 1) of tracks to publish without naming them. Tracks the set marks unnamed
   * and tracks the profile hides (`profile hide`) are never named either.
   */
  hide?: number[];
  /** The set: its id (or a prefix of 4 or more characters), title or day, or `last`. */
  set: string;
};

/** A recording of a set. */
export type SetRecording = {
  path: string;
  /** The mix report kept in the store for it (`mix.reports.get`). */
  report?: string | null;
};

export type SetRef = {
  /** The set: `last`, an id prefix of 4 or more, a title, or a day. */
  set?: string | null;
  /** Without `set`: the latest set at the venue whose name contains this (ignoring case). */
  venue?: string | null;
};

export type SetRole = {
  drive: string;
  /** `primary`, `backup`, `set` (or `gig`, its old name) or `archive`; `None` clears it. */
  role?: string | null;
};

export type SetSettings = {
  /** Make these per-drive overrides instead of base values. */
  drive?: string | null;
  /** Setting key -> value id (see [`settings::SCHEMA`]). */
  values: { [key: string]: string };
};

export type SetSmartPlaylist = {
  /**
   * Only for these libraries, or the libraries of these drives (names, ids or mount paths);
   * every drive when empty.
   */
  drives?: string[];
  filter: string;
  limit?: number | null;
  path: string;
  sort?: string | null;
};

export type SetsQuery = {
  /** Only sets of this kind (ignoring case). */
  kind?: string | null;
  /** Only sets played on or after this day (`2026`, `2026-09` or `2026-09-30`). */
  since?: string | null;
  /** Only sets played on or before this day (`2026` is the whole year). */
  until?: string | null;
  /** Only sets at a venue whose name contains this (ignoring case). */
  venue?: string | null;
};

/**
 * The sets the numbers come from: the DJ's, or the sessions standing in for them (see
 * [`ProfileReport::basis`]).
 */
export type SetsSection = {
  count: number;
  distinct_tracks: number;
  /** Newest first (sessions only on a drive, undated, first). */
  list: SetInfo[];
  longest: number;
  median_tracks: number;
  newest: string | null;
  /** Sessions only on a drive, which the store hasn't archived yet. */
  not_archived: number;
  /** The days of the oldest and newest sets ([`SetInfo::date`]). */
  oldest: string | null;
  /** Track appearances across every set. */
  plays: number;
  /** Sessions left out for having fewer than `min_set_tracks` tracks. */
  too_short: number;
};

export type SettingCategory = "player" | "display" | "mixer";

export type SettingChange = {
  file: SettingsFile;
  /** `None` when the file did not exist yet. */
  from: SettingValue | null;
  key: string;
  to: string;
};

export type SettingDef = {
  category: SettingCategory;
  choices: Choice[];
  default: string;
  file: SettingsFile;
  key: string;
  label: string;
};

export type SettingValue =
  | string
  /** A byte value not in the schema (newer firmware?). Preserved untouched. */
  | number;

/** What writing the profile would change on each drive. */
export type SettingsApplyPlan = {
  targets: DriveSettingsPlan[];
};

/** Writing the settings profile to several drives. */
export type SettingsApplyRequest = {
  /** Write all four files even when the profile doesn't mention their settings. */
  create_missing?: boolean;
  /**
   * Drives, or libraries: each of a library's mounted clones (settings are each drive's own) but
   * archive and retired ones [default: every mounted rekordbox drive but archive and retired
   * ones].
   */
  drives?: string[];
};

export type SettingsFile = "my_setting" | "my_setting2" | "djm_my_setting" | "dev_setting";

/** Someone's published My Settings, and how importing them changes the settings profile. */
export type SettingsImportPlan = {
  /** Base values that change. */
  changes: BaseChange[];
  cid: string | null;
  from: Resolved;
  published: PublishedSettings;
  /**
   * Whether importing replaces the whole base, so settings they didn't publish become unset.
   * Otherwise their settings are added to the base, over the values it has for them.
   */
  replace: boolean;
  uri: string;
};

export type SettingsProfile = {
  /** Applied to every drive. */
  base: { [key: string]: string };
  /** Per-drive deviations, keyed by drive id. */
  drives: { [key: string]: DriveOverrides };
};

export type SettingsSnapshot = {
  missing_files: SettingsFile[];
  problems: string[];
  /**
   * Bytes outside every known setting that differ from what rekordbox 6 writes: settings newer
   * rekordbox versions added (rekordbox 7.2 writes six). Preserved by `apply`, but not managed
   * by profiles until their meaning is known.
   */
  undecoded: UndecodedByte[];
  values: { [key: string]: SettingValue };
};

export type Share = {
  count: number;
  name: string;
  /** Of `known`. */
  share: number;
};

/** `shutdown`. */
export type Shutdown = {
  /** Cancel running requests instead of waiting for them. */
  cancel?: boolean;
};

export type SideSummary = {
  changed_since_last_sync: boolean;
  exported_at: string | null;
  fingerprint: Hash | null;
  id: string | null;
  interrupted_operation: boolean;
  label: string;
  last_synced: string | null;
  library_id: string | null;
  playlists: number;
  sequence: number | null;
  tracks: number;
};

/** Where an archived list was first seen. */
export type Sighting = {
  created: string;
  /** The drive the snapshot was pulled from. */
  drive: string | null;
  library: string;
  short_id: string;
  snapshot: Hash;
};

export type SignatureSection = {
  closers: PlayedTrack[];
  /** Opened (or closed) two or more sets. */
  openers: PlayedTrack[];
  /** Every track played, most sets first. */
  played: PlayedTrack[];
  /** Played in two or more sets, most sets first. */
  tracks: PlayedTrack[];
  /** The same move from one track into another, in two or more sets. */
  transitions: SignatureTransition[];
};

export type SignatureTransition = {
  from: string;
  sets: number;
  to: string;
};

export type Skip = [path: string, reason: string];

export type SmartPlaylist = {
  /**
   * The libraries it's limited to, by a library's id or one of its drives' (any clone of that
   * drive's library takes it, as clones are kept identical); every drive when empty.
   */
  drives?: string[];
  /** See [`crate::query`]. */
  filter: string;
  limit?: number | null;
  /** Where the playlist goes on a drive, e.g. `Smart/Peak time`. */
  path: string;
  sort?: string | null;
};

export type SmartPlaylists = {
  playlists: SmartPlaylist[];
};

/** Snapshot without its file list, for listings. */
export type SnapshotInfo = {
  /**
   * Off the line back from the library's latest: saved beside it (a drive back from a set, see
   * [`crate::sync::pull_beside`]). False from [`Snapshot::info`]; [`crate::api::history`]
   * fills it in.
   */
  beside?: boolean;
  bytes: number;
  created: string;
  files: number;
  fingerprint: Hash;
  id: Hash;
  /**
   * Its labels (see [`Store::add_label`]). Empty from [`Snapshot::info`]; the store's
   * listings fill them in.
   */
  labels?: string[];
  library_id: string;
  message: string | null;
  parents: Hash[];
  short_id: string;
  source: SnapshotSource | null;
  summary: LibrarySummary;
};

/**
 * A name given to a snapshot (`pull --label "before NYE"`). Labels are unique in a store (any
 * case), stand in for the snapshot's id wherever one is asked for, and keep the snapshot from
 * being pruned or collected.
 */
export type SnapshotLabel = {
  created: string;
  name: string;
  snapshot: Hash;
};

export type SnapshotMatch = {
  library: string;
  snapshot: SnapshotInfo;
};

export type SnapshotSource = {
  drive_id: string | null;
  drive_name: string;
};

/** The DJ software to write for. */
export type Software =
  /** Native Instruments Traktor Pro: `collection.nml`. */
  | "traktor"
  /** Serato DJ: `_Serato_/Subcrates/*.crate`. */
  | "serato"
  /** Engine DJ: `Engine Library/Database2/m.db`. */
  | "engine";

/** rekordbox's phrase analysis (`PSSI`, in `.EXT`). */
export type SongStructure = {
  mood: Mood;
  phrases: Phrase[];
};

/** One of the two databases that hold My Tags. */
export type Source =
  /** `exportExt.pdb`, for players that read `export.pdb`. */
  | "export_ext"
  /** OneLibrary's `exportLibrary.db`. */
  | "one_library";

export type SourceInfo = {
  /** History lists (sessions) read. */
  history_lists: number;
  /** `drive` or `store`. */
  kind: string;
  name: string;
  tracks: number;
};

export type SpaceCategory = {
  bytes: number;
  files: number;
  kind: SpaceKind;
};

export type SpaceKind =
  | ("audio" | "analysis" | "artwork" | "databases" | "other")
  /** Player and mixer My Settings. */
  | "settings"
  /** Everything else rekordbox keeps under `PIONEER/`. */
  | "rekordbox_other"
  /** `.locallibrary/backups`: undo copies taken before merges, cleans and edits. */
  | "locallibrary_backups"
  /** The rest of `.locallibrary/` (identity, hash cache). */
  | "locallibrary"
  /** WAV files recorded on the player (`PIONEER REC`, `ALPHATHETA REC`). */
  | "recordings"
  /** `._*`, `.DS_Store`, `.Spotlight-V100`, `.fseventsd`, `Thumbs.db`... */
  | "os_clutter"
  /** `.Trashes`, `$RECYCLE.BIN`: deleted files still using space. */
  | "trash"
  /** Audio files outside the library. */
  | "loose_audio";

export type SpaceReport = {
  /** Largest first. */
  categories: SpaceCategory[];
  cluster_size: number;
  drive: string;
  /** Whether this drive's library fits on the other drives of its library. */
  fits: DriveFit[];
  forecast: Forecast | null;
  free: number | null;
  /** Largest first, at most 50. */
  loose_audio: LooseFile[];
  root: string;
  total: number | null;
  /** Files that couldn't be measured (no permission). */
  unreadable: number;
};

/** Stretch of the mix with no track found. */
export type Span = {
  end: number;
  start: number;
};

/** A value, and whether it's inferred rather than documented. */
export type Spec_for_Exfat = {
  inferred: boolean;
  value: Exfat;
};

/** A value, and whether it's inferred rather than documented. */
export type Spec_for_boolean = {
  inferred: boolean;
  value: boolean;
};

/** Where a drive stood against its library's latest snapshot when it was last seen. */
export type Standing =
  /** It held the latest snapshot. */
  | {
    standing: "up_to_date";
  }
  /** It held an older snapshot: `snapshots` newer ones have been saved since. */
  | {
    held: string;
    snapshots: number;
    standing: "behind";
  }
  /** It held a state the store never saved (a later export or edit that wasn't pulled). */
  | {
    standing: "unsaved";
  }
  /** Not linked to a library, or the library has no snapshots. */
  | {
    standing: "unknown";
  };

/** `watch.start`'s parameters (see [`super::WatchOptions`]). */
export type StartWatch = {
  /** How often drives are polled, in milliseconds (at least 100). */
  interval_ms?: number | null;
  /** Pull drives that hold changes the store lacks. */
  pull?: boolean;
  /** How long a drive must go unchanged before it's looked at, in milliseconds. */
  settle_ms?: number | null;
  /** Then sync the rest of the drive's group from it (`sync_set`, the old name, works too). */
  sync_group?: boolean;
};

/** Step `index` of `total`, counting from 1. `id` names it (`format`, `library`, `verify`, ...). */
export type Step = {
  id: string;
  index: number;
  total: number;
};

export type StepError = {
  kind: string;
  message: string;
  step: string;
};

/** A step of a job. */
export type StepKind =
  /** Remove rekordbox's demo samples (`clean`). */
  | "clean"
  /** Make the recorded edits again (`edits replay`). */
  | "replay"
  /** Fill the rule-based playlists (`smart apply`). */
  | "smart"
  /** Put back the Device Name and colour players show (`drive player`). */
  | "player"
  /** Write My Settings (`settings apply`). */
  | "settings"
  /** Save drives in the store (`pull`). */
  | "pull"
  /** Copy the newest library onto the other drives (`sync`). */
  | "sync"
  /** Check the drives for the players (`check`). */
  | "check"
  /** Eject the drives (`eject`). */
  | "eject";

/** What one step did. */
export type StepReport = {
  /** What it did, in a line each. */
  done: string[];
  /** Each operation's report, as its `*.apply` method returns it. */
  reports: unknown[];
  step: StepKind;
  /** Warnings that came up as it ran (the plan's own aside). */
  warnings: string[];
};

/** `{"event": "step"}`. */
export type StepStarted = {
  event: StepTag;
};

export type StepTag = "step";

/** `watch.stop`. */
export type StopWatch = {
  /** Stop what it's doing too, instead of letting it finish. */
  cancel?: boolean;
};

/** Where the session's secrets are kept. */
export type Storage =
  /** macOS Keychain, Windows Credential Manager, or the Secret Service on Linux. */
  | "keychain"
  /**
   * `atmosphere-session.json` in the store folder, readable only by you: for computers without
   * a keychain (a Linux server without a desktop session).
   */
  | "file";

export type StoreCopyRequest = {
  /** Say what would be copied, and copy nothing. */
  dry_run?: boolean;
  /**
   * Copy over a copy that was written to since it was last copied to (with `--store`), or that
   * has snapshots the store doesn't.
   */
  force?: boolean;
  /**
   * Also remove from the copy what the store no longer has (pruned or collected snapshots,
   * and files nothing in the store uses).
   */
  mirror?: boolean;
  /** Where the copy goes: a folder on a NAS, another disk, a cloud-synced folder. */
  path: string;
  /** Re-hash the files the copy has already, copying again any that changed. */
  verify?: boolean;
};

export type StoreInfo = {
  /** When this store is a copy of another (`store copy`): when it was made. */
  copy_of?: CopyRecord | null;
  /** Size of the copies `art embed` keeps of the files it rewrote, for `art unembed`. */
  embed_backup_bytes: number;
  libraries: LibraryRecord[];
  root: string;
  stats: StoreStats;
};

export type StoreStats = {
  drives: number;
  libraries: number;
  object_bytes: number;
  objects: number;
  snapshots: number;
};

export type StyleSection = {
  /** `settings profile` (the store's) or the drive the files were read from. */
  source: string;
  values: StyleValue[];
};

export type StyleValue = {
  key: string;
  label: string;
  value: string;
  value_label: string;
};

/** `events.subscribe`. */
export type Subscribe = {
  /**
   * How often drives and the store are looked at, in milliseconds (at least 100; default
   * 1000).
   */
  interval_ms?: number | null;
  /** Event kinds (`drive.attached`) or groups (`drive`, `store`, `job`, `watch`); empty: all. */
  kinds?: string[];
};

/** What `events.subscribe` answers: from here on, events say what changes. */
export type Subscribed = {
  /** The drives mounted now. */
  drives: MountedDrive[];
  kinds: string[];
};

export type SuggestSets = {
  /**
   * Only this drive's sessions and those of its library in the store. Default: every mounted
   * rekordbox drive's and the whole store's.
   */
  drive?: string | null;
};

/** What to do about it. */
export type Suggestion =
  | ("nothing" | "update_right" | "update_left" | "merge")
  /** Diverged: pick a side explicitly, or merge. */
  | "choose_or_merge";

/** The sets the sessions suggest, and the sessions they're made of. */
export type Suggestions = {
  /** Newest first. */
  candidates: Candidate[];
  /** The candidates' sessions, newest first. */
  sessions: Session[];
};

export type SyncManyPreview = {
  source: string;
  source_name: string;
  /** One entry per other drive; `plan` is `None` for drives already identical to the source. */
  targets: SyncPreview[];
};

/** Sync any number of drives: the single newest one is copied onto all the others. */
export type SyncManyRequest = {
  allow_low_confidence?: boolean;
  delete_first?: boolean;
  /** Drives, or a library's name for its mounted clones (but archive and retired ones). */
  drives: string[];
  keep_extra?: boolean;
  overwrite_archive?: boolean;
  verify?: boolean;
};

export type SyncPlan = {
  /** Allocation unit of the target filesystem, used to estimate space. */
  cluster_size: number;
  copies: PlannedCopy[];
  copies_by_class: ClassCounts;
  copy_bytes: number;
  delete_bytes: number;
  deletes: PlannedDelete[];
  deletes_by_class: ClassCounts;
  /** Whether the copies fit in the free space (deletes happen last). */
  fits: boolean;
  free_space: number | null;
  /** Source drive name, or `snapshot <id>`. */
  source: string;
  target: string;
  target_name: string;
  unchanged: number;
  warnings: string[];
};

export type SyncPreview = {
  /**
   * The drives diverged, but the target's changes are all player-made (it came back from a
   * set): it is saved in the store before being overwritten (see [`crate::after_set`]).
   */
  after_set?: AfterSet | null;
  comparison: Comparison;
  plan: SyncPlan | null;
  /** `None` when nothing needs to happen. */
  source: string | null;
  target: string | null;
};

export type SyncReport = {
  copied: number;
  copied_bytes: number;
  deleted: number;
  deleted_bytes: number;
  fingerprint: Hash | null;
  unchanged: number;
};

export type SyncRequest = {
  /** Accept a low-confidence "newer" verdict in auto mode. */
  allow_low_confidence?: boolean;
  delete_first?: boolean;
  direction?: Direction;
  keep_extra?: boolean;
  /**
   * With an explicit direction: the target came back from a set, so save it in the store
   * first (as an automatic sync does; see [`SyncPreview::after_set`]).
   */
  keep_player_changes?: boolean;
  left: string;
  /** Write to an archive or retired drive (see [`crate::fleet::Role`]). */
  overwrite_archive?: boolean;
  right: string;
  verify?: boolean;
};

export type SyncResult = {
  report: SyncReport | null;
  /**
   * Snapshots taken along the way (after a set: the state the drive came back with, then the
   * result).
   */
  saved?: SnapshotInfo[];
  source: string | null;
  target: string | null;
};

/**
 * Sync a drive group: every mounted member of a library's group (not archive or retired drives)
 * from the newest of them, which should be the primary.
 */
export type SyncSetRequest = {
  allow_low_confidence?: boolean;
  delete_first?: boolean;
  keep_extra?: boolean;
  /**
   * The library whose group to sync [default: the only group with two or more drives mounted;
   * with several, refused as `choose_library`, listing them].
   */
  library?: string | null;
  verify?: boolean;
};

/** A trait the numbers support, with the evidence. */
export type Tag = {
  id: string;
  label: string;
  why: string;
};

export type TagCategory = {
  name: string;
  tags: TagEntry[];
};

/**
 * A My Tag edit as the drive's journal records it (see [`crate::edit::journal`]): tracks by
 * their audio files (device paths), so a replay on a later export, where ids differ, finds them.
 */
export type TagChange = {
  action: "add";
  tag: string;
  tracks: string[];
} | {
  action: "remove";
  tag: string;
  tracks: string[];
} | {
  action: "rename";
  tag: string;
  to: string;
} | {
  action: "delete";
  tag: string;
};

export type TagEntry = {
  name: string;
  track_ids: number[];
};

export type TagInfo = {
  name: string;
  /** `Category/Tag`. */
  path: string;
  sources: Source[];
  /**
   * Tracks carrying it in either database, with [`Device::library`]'s ids (or, for a track
   * only OneLibrary lists, OneLibrary's).
   */
  tracks: TrackRef[];
};

export type TagPlan = {
  /** `add`, `remove`, `rename` or `delete`. */
  action: string;
  created: Created[];
  /** The databases the edit changes. */
  databases: Source[];
  drive: string;
  root: string;
  /** The category or tag edited (as the drive spells it, if it has it). */
  tag: string;
  /** The new name, for a rename. */
  to: string | null;
  /** Tracks that gain (`add`) or lose (`remove`, `delete`) the tag. */
  tracks: TrackRef[];
  /** Selected tracks that already have (`add`) or don't have (`remove`) the tag. */
  unchanged: number;
  warnings: string[];
};

export type TagReport = {
  backup: string | null;
  /** The databases written. */
  databases: Source[];
  fingerprint: Hash | null;
  tracks_changed: number;
};

/** A My Tag edit: `{"drive": "Home", "action": "add", "tag": "Genre/Deep House", "query": "..."}`. */
export type TagsEdit = {
  drive: string;
} & (
  /** Give the selected tracks a tag (`Category/Tag`), creating it where it's missing. */
  | {
    action: "add";
    /** Every track whose album has this name. */
    album?: string | null;
    /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
    filter?: string | null;
    /** The tracks of a history list (what a player recorded), by name. */
    history?: string | null;
    ids?: number[];
    /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
    paths?: string[];
    playlist?: string | null;
    /** Matches "artist - title" or path substrings; must match at least one track. */
    query?: string | null;
    tag: string;
  }
  /** Take a tag off the selected tracks. */
  | {
    action: "remove";
    /** Every track whose album has this name. */
    album?: string | null;
    /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
    filter?: string | null;
    /** The tracks of a history list (what a player recorded), by name. */
    history?: string | null;
    ids?: number[];
    /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
    paths?: string[];
    playlist?: string | null;
    /** Matches "artist - title" or path substrings; must match at least one track. */
    query?: string | null;
    tag: string;
  }
  /** Rename a tag (`Category/Tag`) or a category (`Category`). */
  | {
    action: "rename";
    tag: string;
    to: string;
  }
  /** Delete a tag, taking it off every track. */
  | {
    action: "delete";
    tag: string;
  });

/** Which database to build. */
export type Target =
  /** `exportLibrary.db`, from `export.pdb`. */
  | "one_library"
  /** `export.pdb`, from `exportLibrary.db`. */
  | "device_library";

export type TargetError = {
  kind: string;
  message: string;
};

/** One target of a [`sync_many_apply`]. */
export type TargetResult = {
  /** Why this target failed (the others carry on). */
  error: TargetError | null;
  name: string;
  report: SyncReport | null;
  target: string;
};

export type TempoBin = [bpm: number, count: number];

/** A track whose stored tempo the estimate doesn't agree with. */
export type TempoFinding = {
  /** 0 to 1. */
  confidence: number;
  /** The analyzer's BPM. */
  estimate: number;
  /** Which of the two the library's other tempos make more likely, if either. */
  likelier: Plausible | null;
  /**
   * The stored tempo against the estimate: `half`, `double`, `two_thirds`, `three_halves` or
   * `disagrees`.
   */
  relation: TempoRelation;
  /** The library's BPM. */
  stored: number;
  track: TrackRef;
};

/**
 * A tempo estimate against the annotation, within [`TOLERANCE`] (4%) of the annotation times the
 * factor (Gouyon et al. 2006). Accuracy1 counts [`TempoOutcome::Correct`]; Accuracy2 also counts
 * the double, half, triple and third tempos.
 */
export type TempoOutcome =
  | ("correct" | "other")
  /** The estimate is double the annotated tempo. */
  | "double"
  /** The estimate is half the annotated tempo. */
  | "half"
  /** The estimate is three times the annotated tempo. */
  | "triple"
  /** The estimate is a third of the annotated tempo. */
  | "third"
  /** The analyzer gave no tempo. */
  | "missing";

/** How one analyzer did. */
export type TempoOutcomeReport = {
  /** Tracks with each outcome, every outcome listed. */
  counts: { [key: string]: number };
  /** The tracks it didn't get fully right, in the order they came, with what it did instead. */
  misses: [string, TempoOutcome][];
  /** The mean credit, 0 to 1: MIREX's weighted key score, or tempo Accuracy1. */
  score: number;
  tracks: number;
};

/** A stored tempo against an estimate of it. */
export type TempoRelation =
  | ("agrees" | "disagrees")
  /** The stored tempo is half the estimate. */
  | "half"
  /** The stored tempo is double the estimate. */
  | "double"
  /** The stored tempo is two thirds of the estimate. */
  | "two_thirds"
  /** The stored tempo is three halves of the estimate. */
  | "three_halves";

export type TempoSection = {
  bin_width: number;
  /** Where the middle 80% sits: the 10th and 90th percentiles. */
  core: BpmRange | null;
  /** Covering the 2nd to 98th percentile. */
  histogram: TempoBin[];
  max: number | null;
  median: number | null;
  min: number | null;
  p25: number | null;
  p75: number | null;
  /** Plays (or tracks) with a BPM. */
  tracks: number;
};

export type TidyItem = {
  bytes: number;
  files: number;
  is_dir: boolean;
  kind: TidyKind;
  /** Relative to the drive root. */
  path: string;
};

export type TidyKind = "apple_double" | "ds_store" | "thumbnails" | "temporary_items" | "spotlight" | "fs_events" | "trash" | "temp_files";

export type TidyPlan = {
  bytes: number;
  drive: string;
  items: TidyItem[];
  root: string;
  /** Space in the trash that `empty_trash` would free. */
  trash_bytes: number;
};

export type TidyReport = {
  bytes: number;
  failed: Failure[];
  removed: number;
};

export type TidyRequest = {
  drive: string;
  /** Also delete what's in `.Trashes` / `$RECYCLE.BIN` (for good). */
  empty_trash?: boolean;
};

/** One play in an imported log. */
export type TimedPlay = {
  entry: SetEntry;
  /** The player it was played on (`3`), when the log says. */
  player?: string | null;
  started: string;
  stopped?: string | null;
};

/** A drive, folder or file an operation reads or writes, and its state when it was planned. */
export type Touched = {
  /**
   * A drive (archive and retired drives are refused unless the plan allows it), rather than
   * files on this computer.
   */
  drive: boolean;
  name: string;
  /** The drive's root, or the folder or file on this computer. */
  root: string;
  /** Whether the operation writes to it (a sync's source is only read). */
  writes: boolean;
};

export type Track = {
  album: string;
  album_artist: string;
  /** Device path of the primary analysis file (`.DAT`); `.EXT`/`.2EX` sit next to it. */
  analyze_path: string;
  artist: string;
  artwork_id: number | null;
  /** Device path of the artwork thumbnail, if any. */
  artwork_path: string | null;
  bitrate: number;
  /** Beats per minute (as analysed). */
  bpm: number;
  color: TrackColor | null;
  comment: string;
  composer: string;
  date_added: string;
  disc_number: number;
  duration_secs: number;
  /** Absolute device path of the audio file, e.g. `/Contents/Artist/Album/Track.mp3`. */
  file_path: string;
  file_size: number;
  genre: string;
  /** Row id inside this export. Only meaningful within one export. */
  id: number;
  isrc: string;
  key: string;
  label: string;
  mix_name: string;
  original_artist: string;
  play_count: number;
  /** 0-5 stars. */
  rating: number;
  release_date: string;
  remixer: string;
  sample_depth: number;
  sample_rate: number;
  title: string;
  track_number: number;
  year: number;
};

/** A track's artwork in a batch: `image` is `None` when it has none. */
export type TrackArtwork_for_ArtworkFile = {
  image: ArtworkFile | null;
  track: number;
};

/** A track's artwork in a batch: `image` is `None` when it has none. */
export type TrackArtwork_for_ArtworkImage = {
  image: ArtworkImage | null;
  track: number;
};

export type TrackChange = {
  fields: FieldChange[];
  track: TrackRef;
};

/** rekordbox's eight track colours. */
export type TrackColor = "pink" | "red" | "orange" | "yellow" | "green" | "aqua" | "blue" | "purple";

export type TrackCueChange = {
  after: CueInfo[];
  before: CueInfo[];
  /** Analysis files that change, as drive keys. */
  files: string[];
  /** Whether the track's rows in `exportLibrary.db` change too. */
  onelibrary: boolean;
  track: TrackRef;
};

/** A drive track's cues. */
export type TrackCues = {
  bpm: number;
  cues: CueInfo[];
  /** How many cue rows OneLibrary (`exportLibrary.db`) has for the track, if it lists it. */
  onelibrary_cues: number | null;
  track: TrackRef;
  warnings: string[];
};

/**
 * Everything known about one track on a drive: its details, where it's filed, problems with it
 * and a summary of its analysis.
 */
export type TrackDetails = {
  analysis: AnalysisSummary;
  /** Whether the audio file is on the drive. */
  audio_present: boolean;
  /** Other tracks on the drive that are the same recording (artist, title and duration). */
  copies: TrackRef[];
  /** History lists (what players recorded) holding the track, by name. */
  history: string[];
  /** Playlists holding the track, by path, sorted. */
  playlists: string[];
  /** My Tags, as `Category/Tag`. Empty when the drive's tag database can't be read. */
  tags: string[];
  track: Track;
};

/**
 * Which tracks an operation targets: the tracks that fit every field given, so a playlist and a
 * filter together pick the playlist's tracks the filter matches. At least one field must be set,
 * and a blank query, filter, album, playlist or history list is refused rather than taken to
 * mean every track.
 */
export type TrackDetailsRequest = {
  /** Every track whose album has this name. */
  album?: string | null;
  drive: string;
  /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
  filter?: string | null;
  /** The tracks of a history list (what a player recorded), by name. */
  history?: string | null;
  ids?: number[];
  /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
  paths?: string[];
  playlist?: string | null;
  /** Matches "artist - title" or path substrings; must match at least one track. */
  query?: string | null;
};

export type TrackFailure = [track: TrackRef, error: string];

/**
 * New values for track details; fields left out stay as they are, and an empty name clears
 * artist, album, genre, label, key and the like.
 */
export type TrackFields = {
  album?: string | null;
  artist?: string | null;
  /** A colour name (`aqua`), rekordbox's number (1-8), or `none`. */
  color?: string | null;
  comment?: string | null;
  composer?: string | null;
  disc_number?: number | null;
  genre?: string | null;
  key?: string | null;
  label?: string | null;
  mix_name?: string | null;
  original_artist?: string | null;
  play_count?: number | null;
  /** 0-5 stars. */
  rating?: number | null;
  remixer?: string | null;
  title?: string | null;
  track_number?: number | null;
  year?: number | null;
};

export type TrackPlays = {
  /** History lists the track appears in. */
  history_lists: number;
  /** The play count rekordbox and the players keep (the higher of the two databases'). */
  play_count: number;
  track: TrackRef;
};

export type TrackPlaysQuery = {
  /** Part of `Artist - Title` (ignoring case), or a drive path (`/Contents/...`). */
  track: string;
};

export type TrackRef = {
  file_path: string;
  id: number;
  name: string;
};

/**
 * Which tracks an operation targets: the tracks that fit every field given, so a playlist and a
 * filter together pick the playlist's tracks the filter matches. At least one field must be set,
 * and a blank query, filter, album, playlist or history list is refused rather than taken to
 * mean every track.
 */
export type TrackSelector = {
  /** Every track whose album has this name. */
  album?: string | null;
  /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
  filter?: string | null;
  /** The tracks of a history list (what a player recorded), by name. */
  history?: string | null;
  ids?: number[];
  /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
  paths?: string[];
  playlist?: string | null;
  /** Matches "artist - title" or path substrings; must match at least one track. */
  query?: string | null;
};

/** Which history list becomes a recording's tracklist. */
export type Tracklist =
  | "none"
  /** The newest history list, for the newest recording. */
  | "latest"
  /** This history list (by name), for every recording copied. */
  | {
    named: string;
  };

export type TracksById = {
  /** Ids the library doesn't have. */
  missing: number[];
  /** In the order asked for. */
  tracks: Track[];
};

export type TracksGet = {
  drive: string;
  ids: number[];
};

export type TracksPage = {
  /** Tracks matching, before `offset` and `limit`. */
  total: number;
  total_duration_secs: number;
  tracks: Track[];
};

export type TracksQuery = {
  drive: string;
  /** See [`crate::query`]. */
  filter?: string | null;
  limit?: number | null;
  offset?: number;
  /** Only tracks in this playlist, in its order (unless `sort` is given). */
  playlist?: string | null;
  /** E.g. `key,bpm` or `-rating`. */
  sort?: string | null;
};

/** The change from one play to the next. */
export type Transition = {
  /** When the incoming track is first heard. */
  at: number;
  /** Indexes into [`MixReport::plays`]. */
  from: number;
  /** `same key`, `±1`, `relative`, `energy boost`, or `clash`, from the played keys. */
  harmony: string | null;
  /** Seconds both are heard together (negative: a gap between them). */
  overlap: number;
  /** Played tempo change, percent. */
  tempo_change: number | null;
  to: number;
};

/** A data byte of a settings file that no known setting covers. */
export type UndecodedByte = {
  /** What rekordbox 6 writes there. */
  expected: number;
  file: SettingsFile;
  /** Offset in the file's data section (after the 104-byte header). */
  offset: number;
  value: number;
};

/** What can be undone: every mounted drive's backups, and art embedded in files on this computer. */
export type UndoList = {
  drives: DriveBackups[];
  /** `art unembed` puts these files back. */
  embeds: EmbedBackup[];
};

export type UndoListRequest = {
  /** Only this drive [default: every mounted drive]. */
  drive?: string | null;
};

/** What putting a backup back would change. */
export type UndoPlan = {
  backup: BackupInfo;
  /** How the playlists and tracks change, from the drive as it is to the drive put back. */
  diff: PlaylistDiff;
  drive: string;
  /** The files that change. */
  files: FileChange[];
  /** History lists on the drive that the backup doesn't have: putting it back drops them. */
  history_lists: string[];
  /** Recorded edits taken off the journal: `edits replay` won't make them again. */
  journal: JournalEntry[];
  /**
   * Tracks the backup lists whose audio is neither on the drive nor in the backup: putting it
   * back is refused unless forced.
   */
  missing_audio: string[];
  root: string;
};

export type UndoRequest = {
  /** A backup other than the one `undo` takes (see [`super::list_backups`]). */
  backup?: string | null;
  /** The drive [default: the only mounted drive with backups]. */
  drive?: string | null;
  /**
   * Put it back even if the drive changed since its change, or its database lists audio
   * that's no longer on the drive (the library was replaced since).
   */
  force?: boolean;
};

export type UndoableBackup = {
  /**
   * What its change left in each file putting it back replaces or removes, by content (`null`:
   * absent). Missing when the change didn't finish, or an earlier version made the backup.
   */
  after?: { [key: string]: Hash | null } | null;
  /** How many of `files` are audio (removed tracks' files, which may be their only copy). */
  audio: number;
  created: string;
  files: string[];
  /** Each of `files` as it was backed up, by content. */
  hashes?: { [key: string]: Hash };
  id: string;
  /** Recorded edits putting it back takes off the journal. */
  journal: number;
  /** It's the one `undo` puts back. */
  next: boolean;
  reason: string;
  /**
   * Its place among the drive's backups, counting up (0: made by a version that didn't number
   * them).
   */
  seq: number;
  /**
   * When an undo took it: the backup that undo put back. Putting this one back redoes what the
   * undo took back; `undo` passes it by.
   */
  undoes?: string | null;
};

/** What taking embedded art back out would do. */
export type UnembedPlan = {
  backup: EmbedBackup;
  /** Files put back as they were. */
  restored: string[];
  /** Files changed since the art went in (or gone): left as they are. */
  skipped: Skip[];
};

/** What taking embedded art back out did. */
export type UnembedReport = {
  backup: string;
  restored: string[];
  /** Files changed since (or gone), left as they are. */
  skipped: Skip[];
};

export type UnembedRequest = {
  /** Which embed to take back [default: the latest]. */
  backup?: string | null;
};

/** Tracks one player won't load, for one reason. */
export type Unloadable = {
  /** The bit depth the player doesn't take, when that's the reason. */
  bit_depth?: number | null;
  drive: string;
  /** The format (`FLAC`), or the file extension when it's no format players list. */
  format: string;
  /** `problem`, or `warning` when it rests on a value the model's specs only infer. */
  level: Level;
  /** The `formats` item's message. */
  message: string;
  player: string;
  reason: Unplayable;
  /** The sample rate (Hz) the player doesn't take, when that's the reason. */
  sample_rate?: number | null;
  /** One per audio file. */
  tracks: TrackRef[];
};

/** How a model gets on with a file. */
export type Unplayable =
  | ("sample_rate" | "bit_depth" | "sample_rate_and_bit_depth")
  /** It doesn't play the format at all. */
  | "format";

/**
 * What the target holds that packing would replace and that nothing else keeps: it is saved in
 * the store first, as `sync` saves a drive back from a set before overwriting it.
 */
export type Unsaved = {
  /**
   * The drive changed since it was last packed from this source (memory cues, TAG LIST
   * playlists or history lists players saved, or edits).
   */
  changed: boolean;
  /** History lists players recorded that the packed library won't have. */
  history_lists: string[];
  /**
   * Its OneLibrary database can't be read, so history lists players that read only OneLibrary
   * recorded there can't be told apart: it's kept whole.
   */
  unreadable_onelibrary: boolean;
};

export type UnsetSettings = {
  drive?: string | null;
  keys: string[];
};

export type Unsubscribed = {
  /** False when there was no subscription. */
  unsubscribed: boolean;
};

/** Rename and/or recolour cues (`""` clears a name, `"none"` a colour). */
export type UpdateCue = {
  color?: string | null;
  cue: CueRef;
  name?: string | null;
};

export type Verdict =
  | "no_players"
  /** Every rekordbox player. */
  | "all_players"
  /** Newer players only. */
  | "some_players"
  /** Untested layouts players may or may not read. */
  | "unknown";

export type VerifyReport = {
  checked: number;
  corrupt: string[];
  missing: string[];
};

export type VerifyRequest = {
  /** Other mounted drives that may hold good copies. */
  against?: string[];
  drive: string;
  /** Compare with this snapshot (id prefix) [default: the snapshot of the drive's state]. */
  snapshot?: string | null;
};

/** What `version` answers. */
export type VersionInfo = {
  methods: string[];
  version: string;
};

/** One partition (or, for disks without a partition table, the disk itself). */
export type Volume = {
  cluster_size: number | null;
  filesystem: FsKind | null;
  /** The file system as the OS names it ("MS-DOS FAT32", "vfat", "exFAT"...). */
  filesystem_name: string | null;
  /** OS identifier: `disk4s1`, `/dev/sdb1`, `2:1` (disk:partition on Windows). */
  id: string;
  label: string | null;
  mount: string | null;
  size: number;
  uuid?: string | null;
};

/** What watching did about a drive. */
export type WatchAction =
  /** The store holds the drive's state already. */
  | {
    event: "watch.up_to_date";
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    library: string;
    /** Its adopted name, else the volume's name. */
    name: string;
    root: string;
  }
  /** The drive holds a state the store lacks, and it wasn't pulled (watching without `pull`). */
  | {
    /** It would be saved beside the library's newest snapshot (back from a set). */
    beside: boolean;
    event: "watch.pending";
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    /** History lists the store lacks. */
    history_lists: string[];
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    library: string;
    /** Its adopted name, else the volume's name. */
    name: string;
    root: string;
  }
  /** The drive's state was pulled into the store as a new snapshot. */
  | {
    /** Saved beside the library's newest snapshot, which stays the newest. */
    beside: boolean;
    event: "watch.pulled";
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    /** History lists the store lacked. */
    history_lists: string[];
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    library: string;
    /** Its adopted name, else the volume's name. */
    name: string;
    root: string;
    snapshot: SnapshotInfo;
  }
  /** The library's other mounted drives were synced from this one. */
  | {
    event: "watch.synced";
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    library: string;
    /** Its adopted name, else the volume's name. */
    name: string;
    root: string;
    targets: TargetResult[];
  }
  /**
   * Another operation holds the store or the drive: it's looked at again once that's done.
   * Said once per wait.
   */
  | {
    event: "watch.waiting";
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    /** Its adopted name, else the volume's name. */
    name: string;
    reason: string;
    root: string;
  }
  /** Left alone, and why. */
  | {
    event: "watch.skipped";
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    /** Its adopted name, else the volume's name. */
    name: string;
    reason: string;
    root: string;
  }
  /** Looking at the drive failed (other than waiting on a lock): `kind` is the error's code. */
  | {
    event: "watch.failed";
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    kind: string;
    message: string;
    /** Its adopted name, else the volume's name. */
    name: string;
    root: string;
  };

/** What a poll noticed. */
export type WatchEvent =
  /** A volume was mounted (or a different one mounted where another was). */
  | (MountedDrive & {
    event: "drive.attached";
  })
  /** A volume went away; the drive as it was last seen. */
  | (MountedDrive & {
    event: "drive.detached";
  })
  /** A mounted drive's databases or state changed. */
  | {
    changed: DriveChange[];
    event: "drive.changed";
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    /** Its adopted name, else the volume's name. */
    name: string;
    root: string;
  }
  /**
   * The store's libraries, snapshots, labels, drive registry, settings, smart playlists or
   * sets changed.
   */
  | {
    changed: string[];
    event: "store.changed";
    root: string;
  };

export type WatchStarted = {
  watching: boolean;
};

export type WatchStopped = {
  /** False when it wasn't watching. */
  stopped: boolean;
};

export type WavFormat = {
  bits: number;
  channels: number;
  sample_rate: number;
};

/** One column of a waveform: a back layer and a (shorter, brighter) front layer drawn over it. */
export type WaveColumn = {
  back: WaveLayer;
  front: WaveLayer;
};

/** One drawn layer of a waveform column. */
export type WaveLayer = {
  color: number[];
  /** 0.0-1.0 of the waveform's full height. */
  height: number;
};

export type Waveform = {
  columns: WaveColumn[];
  /**
   * Columns per second of audio for scrolling (detail) waveforms; `None` for previews, whose
   * columns span the whole track.
   */
  columns_per_sec: number | null;
  style: WaveformStyle;
};

/** Which waveforms `track.analysis` sends. */
export type WaveformChoice =
  | ("none" | "all")
  /** The whole-track overview. */
  | "preview"
  /** The scrolling one, which is large: about 150 columns a second. */
  | "detail";

/** A waveform as asked for. */
export type WaveformData = Waveform | PackedWaveform;

/** How waveforms are sent. */
export type WaveformEncoding =
  /** Each column as an object: the original shape, large (4-5 MB for a scrolling waveform). */
  | "json"
  /** Packed bytes in base64 ([`PackedWaveform`]), a few bytes a column. */
  | "base64";

export type WaveformStyle =
  /** Monochrome blue, older players (`PWAV`, `PWV3`). */
  | "blue"
  /** Colour by frequency content, nexus 2 and later (`PWV4`, `PWV5`). */
  | "rgb";

/**
 * When a session was recorded, as far as anyone can tell. Players have no clock, so for a
 * history list it's a window: after locallibrary last saw the drive without the list, before it
 * first saw it with it.
 */
export type Window = {
  /** Recorded after this, when known. */
  after: string | null;
  /** Recorded before this, when known. */
  before: string | null;
  /**
   * The times come from the log itself (an imported log): when the first track started and
   * the last one stopped.
   */
  exact: boolean;
};

/** Accepted by every method that writes (with its other parameters). */
export type WriteOptions = {
  /** Allow writing to archive and retired drives, which are refused (`conflict`) otherwise. */
  overwrite_archive?: boolean;
};

export type XmlReport = {
  cues: number;
  playlists: number;
  tracks: number;
  tracks_with_beatgrid: number;
};

/** Every method, by name (aliases included): its parameters and result. */
export interface Methods {
  /** control method */
  initialize: { params: NoParams; result: Initialized };
  /** read method */
  version: { params: NoParams; result: VersionInfo };
  /** control method */
  "rpc.describe": { params: DescribeRequest; result: unknown };
  /** control method */
  "$/cancel": { params: CancelRequest; result: Cancelled };
  /** control method */
  "jobs.list": { params: NoParams; result: JobInfo[] };
  /** control method */
  "events.subscribe": { params: Subscribe; result: Subscribed };
  /** control method */
  "events.unsubscribe": { params: NoParams; result: Unsubscribed };
  /** control method */
  "watch.start": { params: StartWatch; result: WatchStarted };
  /** control method */
  "watch.stop": { params: StopWatch; result: WatchStopped };
  /** control method */
  shutdown: { params: Shutdown; result: null };
  /** read method; long running: sends `progress`, can be cancelled */
  status: { params: NoParams; result: DriveStatus[] };
  /** read method */
  "drives.list": { params: ListDrives; result: DeviceInfo[] };
  /** read method */
  "drives.mounted": { params: NoParams; result: MountedDrive[] };
  /** read method */
  "drive.info": { params: DriveRef; result: DeviceInfo };
  /** plan method; paired with `drive.adopt.apply` */
  "drive.adopt.plan": { params: AdoptDrive & WriteOptions; result: AdoptPlan & PlanFields };
  /** apply method; paired with `drive.adopt.plan` */
  "drive.adopt.apply": { params: ApplyPlan | (AdoptDrive & WriteOptions); result: DeviceInfo };
  /** apply method; paired with `drive.adopt.plan`; an older name for `drive.adopt.apply` */
  "drive.adopt": { params: ApplyPlan | (AdoptDrive & WriteOptions); result: DeviceInfo };
  /** read method */
  "drive.library": { params: LibraryRequest; result: LibraryPage };
  /** read method */
  "playlists.tree": { params: DriveRef; result: PlaylistTree };
  /** read method */
  "tracks.get": { params: TracksGet; result: TracksById };
  /** read method */
  "tracks.query": { params: TracksQuery; result: TracksPage };
  /** read method */
  "track.analysis": { params: AnalysisRequest; result: AnalysisView };
  /** read method */
  "track.show": { params: TrackDetailsRequest; result: TrackDetails[] };
  /** read method */
  "track.matches": { params: MatchesRequest; result: HarmonicMatch[] };
  /** read method */
  "drive.backups": { params: DriveRef; result: BackupInfo[] };
  /** read method; @deprecated use `drive.restore.plan` */
  "drive.restore.preview": { params: RestoreBackup; result: RestorePreview };
  /** plan method; paired with `drive.restore.apply`; can delete or overwrite something */
  "drive.restore.plan": { params: RestoreBackup & WriteOptions; result: UndoPlan & PlanFields };
  /** apply method; paired with `drive.restore.plan`; can delete or overwrite something */
  "drive.restore.apply": { params: ApplyPlan | (RestoreBackup & WriteOptions); result: BackupInfo };
  /** apply method; paired with `drive.restore.plan`; can delete or overwrite something; an older name for `drive.restore.apply` */
  "drive.restore": { params: ApplyPlan | (RestoreBackup & WriteOptions); result: BackupInfo };
  /** read method */
  "undo.list": { params: UndoListRequest; result: UndoList };
  /** plan method; paired with `undo.apply`; can delete or overwrite something */
  "undo.plan": { params: UndoRequest & WriteOptions; result: UndoPlan & PlanFields };
  /** apply method; paired with `undo.plan`; can delete or overwrite something */
  "undo.apply": { params: ApplyPlan | (UndoRequest & WriteOptions); result: BackupInfo };
  /** read method; long running: sends `progress`, can be cancelled */
  compare: { params: ComparePair; result: Comparison };
  /** plan method; paired with `sync.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "sync.plan": { params: SyncRequest & WriteOptions; result: SyncPreview & PlanFields };
  /** apply method; paired with `sync.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "sync.apply": { params: ApplyPlan | (SyncRequest & WriteOptions); result: SyncResult };
  /** plan method; paired with `sync.many.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "sync.many.plan": { params: SyncManyRequest & WriteOptions; result: SyncManyPreview & PlanFields };
  /** plan method; paired with `sync.many.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something; an older name for `sync.many.plan` */
  "sync_many.plan": { params: SyncManyRequest & WriteOptions; result: SyncManyPreview & PlanFields };
  /** apply method; paired with `sync.many.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "sync.many.apply": { params: ApplyPlan | (SyncManyRequest & WriteOptions); result: TargetResult[] };
  /** apply method; paired with `sync.many.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something; an older name for `sync.many.apply` */
  "sync_many.apply": { params: ApplyPlan | (SyncManyRequest & WriteOptions); result: TargetResult[] };
  /** plan method; paired with `sync.group.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "sync.group.plan": { params: SyncSetRequest & WriteOptions; result: SyncManyPreview & PlanFields };
  /** plan method; paired with `sync.group.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something; an older name for `sync.group.plan` */
  "sync_group.plan": { params: SyncSetRequest & WriteOptions; result: SyncManyPreview & PlanFields };
  /** plan method; paired with `sync.group.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something; an older name for `sync.group.plan` */
  "sync_set.plan": { params: SyncSetRequest & WriteOptions; result: SyncManyPreview & PlanFields };
  /** apply method; paired with `sync.group.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "sync.group.apply": { params: ApplyPlan | (SyncSetRequest & WriteOptions); result: TargetResult[] };
  /** apply method; paired with `sync.group.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something; an older name for `sync.group.apply` */
  "sync_group.apply": { params: ApplyPlan | (SyncSetRequest & WriteOptions); result: TargetResult[] };
  /** apply method; paired with `sync.group.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something; an older name for `sync.group.apply` */
  "sync_set.apply": { params: ApplyPlan | (SyncSetRequest & WriteOptions); result: TargetResult[] };
  /** plan method; paired with `pull.apply`; long running: sends `progress`, can be cancelled */
  "pull.plan": { params: PullRequest & WriteOptions; result: PullPlan & PlanFields };
  /** apply method; paired with `pull.plan`; long running: sends `progress`, can be cancelled */
  "pull.apply": { params: ApplyPlan | (PullRequest & WriteOptions); result: PullReport };
  /** apply method; paired with `pull.plan`; long running: sends `progress`, can be cancelled; an older name for `pull.apply` */
  pull: { params: ApplyPlan | (PullRequest & WriteOptions); result: PullReport };
  /** plan method; paired with `push.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "push.plan": { params: PushRequest & WriteOptions; result: PushPreview & PlanFields };
  /** apply method; paired with `push.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "push.apply": { params: ApplyPlan | (PushRequest & WriteOptions); result: SyncReport };
  /** plan method; paired with `merge.apply`; long running: sends `progress`, can be cancelled */
  "merge.plan": { params: MergeRequest & WriteOptions; result: MergePlan & PlanFields };
  /** apply method; paired with `merge.plan`; long running: sends `progress`, can be cancelled */
  "merge.apply": { params: ApplyPlan | (MergeRequest & WriteOptions); result: MergeReport };
  /** plan method; paired with `clean.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "clean.plan": { params: DriveRef & WriteOptions; result: CleanPlan & PlanFields };
  /** apply method; paired with `clean.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "clean.apply": { params: ApplyPlan | (DriveRef & WriteOptions); result: CleanReport };
  /** plan method; paired with `convert.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "convert.plan": { params: ConvertRequest & WriteOptions; result: ConvertPlan & PlanFields };
  /** apply method; paired with `convert.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "convert.apply": { params: ApplyPlan | (ConvertRequest & WriteOptions); result: ConvertReport };
  /** read method */
  "store.info": { params: NoParams; result: StoreInfo };
  /** read method */
  "store.history": { params: HistoryRequest; result: SnapshotInfo[] };
  /** plan method; paired with `store.gc.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "store.gc.plan": { params: NoParams & WriteOptions; result: GcReport & PlanFields };
  /** apply method; paired with `store.gc.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "store.gc.apply": { params: ApplyPlan | (GcRequest & WriteOptions); result: GcReport };
  /** apply method; paired with `store.gc.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something; an older name for `store.gc.apply` */
  "store.gc": { params: ApplyPlan | (GcRequest & WriteOptions); result: GcReport };
  /** read method; long running: sends `progress`, can be cancelled */
  "store.verify": { params: NoParams; result: VerifyReport };
  /** plan method; paired with `store.prune.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "store.prune.plan": { params: PruneOptions & WriteOptions; result: PruneReport & PlanFields };
  /** apply method; paired with `store.prune.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "store.prune.apply": { params: ApplyPlan | (PruneOptions & WriteOptions); result: PruneReport };
  /** apply method; paired with `store.prune.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something; an older name for `store.prune.apply` */
  "store.prune": { params: ApplyPlan | (PruneOptions & WriteOptions); result: PruneReport };
  /** immediate method; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "store.copy": { params: StoreCopyRequest & WriteOptions; result: CopyReport };
  /** read method */
  "store.copies": { params: NoParams; result: CopyStatus[] };
  /** immediate method */
  "snapshot.label": { params: LabelRequest & WriteOptions; result: SnapshotLabel };
  /** read method */
  "library.list": { params: NoParams; result: LibraryListing[] };
  /** immediate method */
  "library.rename": { params: RenameLibrary & WriteOptions; result: LibraryRecord };
  /** plan method; paired with `edit.apply`; can delete or overwrite something */
  "edit.plan": { params: EditRequest & WriteOptions; result: EditPlan & PlanFields };
  /** apply method; paired with `edit.plan`; can delete or overwrite something */
  "edit.apply": { params: ApplyPlan | (EditRequest & WriteOptions); result: EditReport };
  /** read method */
  "edits.list": { params: DriveRef; result: JournalEntry[] };
  /** plan method; paired with `edits.replay.apply` */
  "edits.replay.plan": { params: DriveRef & WriteOptions; result: EditPlan & PlanFields };
  /** apply method; paired with `edits.replay.plan` */
  "edits.replay.apply": { params: ApplyPlan | (DriveRef & WriteOptions); result: EditReport };
  /** plan method; paired with `edits.clear.apply`; can delete or overwrite something */
  "edits.clear.plan": { params: ClearEdits & WriteOptions; result: ClearEditsPlan & PlanFields };
  /** apply method; paired with `edits.clear.plan`; can delete or overwrite something */
  "edits.clear.apply": { params: ApplyPlan | (ClearEdits & WriteOptions); result: ClearedEdits };
  /** apply method; paired with `edits.clear.plan`; can delete or overwrite something; an older name for `edits.clear.apply` */
  "edits.clear": { params: ApplyPlan | (ClearEdits & WriteOptions); result: ClearedEdits };
  /** read method */
  "smart.list": { params: NoParams; result: SmartPlaylists };
  /** immediate method */
  "smart.set": { params: SetSmartPlaylist & WriteOptions; result: SmartPlaylists };
  /** immediate method; can delete or overwrite something */
  "smart.remove": { params: RemoveSmartPlaylist & WriteOptions; result: SmartPlaylists };
  /** plan method; paired with `smart.write.apply`; can delete or overwrite something */
  "smart.write.plan": { params: DriveRef & WriteOptions; result: EditPlan & PlanFields };
  /** plan method; paired with `smart.write.apply`; can delete or overwrite something; an older name for `smart.write.plan` */
  "smart.apply.plan": { params: DriveRef & WriteOptions; result: EditPlan & PlanFields };
  /** apply method; paired with `smart.write.plan`; can delete or overwrite something */
  "smart.write.apply": { params: ApplyPlan | (DriveRef & WriteOptions); result: EditReport };
  /** apply method; paired with `smart.write.plan`; can delete or overwrite something; an older name for `smart.write.apply` */
  "smart.apply": { params: ApplyPlan | (DriveRef & WriteOptions); result: EditReport };
  /** plan method; paired with `dedupe.apply`; can delete or overwrite something */
  "dedupe.plan": { params: DedupeRequest & WriteOptions; result: EditPlan & PlanFields };
  /** apply method; paired with `dedupe.plan`; can delete or overwrite something */
  "dedupe.apply": { params: ApplyPlan | (DedupeRequest & WriteOptions); result: EditReport };
  /** plan method; paired with `relocate.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "relocate.plan": { params: RelocateRequest & WriteOptions; result: RelocatePlan & PlanFields };
  /** apply method; paired with `relocate.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "relocate.apply": { params: ApplyPlan | (RelocateRequest & WriteOptions); result: EditReport };
  /** plan method; paired with `history.delete.apply`; can delete or overwrite something */
  "history.delete.plan": { params: DeleteHistoryRequest & WriteOptions; result: EditPlan & PlanFields };
  /** apply method; paired with `history.delete.plan`; can delete or overwrite something */
  "history.delete.apply": { params: ApplyPlan | (DeleteHistoryRequest & WriteOptions); result: EditReport };
  /** plan method; paired with `import.apply` */
  "import.plan": { params: ImportRequest & WriteOptions; result: ImportPreview & PlanFields };
  /** apply method; paired with `import.plan` */
  "import.apply": { params: ApplyPlan | (ImportRequest & WriteOptions); result: EditReport };
  /** plan method; paired with `pack.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "pack.plan": { params: PackRequest & WriteOptions; result: PackPlan & PlanFields };
  /** apply method; paired with `pack.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "pack.apply": { params: ApplyPlan | (PackRequest & WriteOptions); result: PackReport };
  /** read method */
  "tags.list": { params: DriveRef; result: MyTags };
  /** plan method; paired with `tags.apply`; can delete or overwrite something */
  "tags.plan": { params: TagsEdit & WriteOptions; result: TagPlan & PlanFields };
  /** apply method; paired with `tags.plan`; can delete or overwrite something */
  "tags.apply": { params: ApplyPlan | (TagsEdit & WriteOptions); result: TagReport };
  /** read method */
  "cues.list": { params: ListCues; result: TrackCues[] };
  /** plan method; paired with `cues.apply`; can delete or overwrite something */
  "cues.plan": { params: EditCues & WriteOptions; result: CuePlan & PlanFields };
  /** apply method; paired with `cues.plan`; can delete or overwrite something */
  "cues.apply": { params: ApplyPlan | (EditCues & WriteOptions); result: CueReport };
  /** plan method; paired with `playlist.restore.apply`; can delete or overwrite something */
  "playlist.restore.plan": { params: PlaylistRestoreRequest & WriteOptions; result: PlaylistRestorePreview & PlanFields };
  /** apply method; paired with `playlist.restore.plan`; can delete or overwrite something */
  "playlist.restore.apply": { params: ApplyPlan | (PlaylistRestoreRequest & WriteOptions); result: EditReport };
  /** apply method; paired with `playlist.restore.plan`; can delete or overwrite something; an older name for `playlist.restore.apply` */
  "playlist.restore": { params: ApplyPlan | (PlaylistRestoreRequest & WriteOptions); result: EditReport };
  /** read method */
  "settings.schema": { params: NoParams; result: SettingDef[] };
  /** read method */
  "settings.profile": { params: NoParams; result: SettingsProfile };
  /** immediate method */
  "settings.set": { params: SetSettings & WriteOptions; result: SettingsProfile };
  /** immediate method */
  "settings.unset": { params: UnsetSettings & WriteOptions; result: SettingsProfile };
  /** read method */
  "settings.show": { params: DriveRef; result: DriveSettingsView };
  /** plan method; paired with `settings.write.apply`; can delete or overwrite something */
  "settings.write.plan": { params: SettingsApplyRequest & WriteOptions; result: SettingsApplyPlan & PlanFields };
  /** plan method; paired with `settings.write.apply`; can delete or overwrite something; an older name for `settings.write.plan` */
  "settings.apply.plan": { params: SettingsApplyRequest & WriteOptions; result: SettingsApplyPlan & PlanFields };
  /** apply method; paired with `settings.write.plan`; can delete or overwrite something */
  "settings.write.apply": { params: ApplyPlan | (ApplySettings & WriteOptions); result: DriveSettingsResult[] | ApplyReport };
  /** apply method; paired with `settings.write.plan`; can delete or overwrite something; an older name for `settings.write.apply` */
  "settings.apply": { params: ApplyPlan | (ApplySettings & WriteOptions); result: DriveSettingsResult[] | ApplyReport };
  /** plan method; paired with `settings.import.apply`; can delete or overwrite something */
  "settings.import.plan": { params: ImportSettings & WriteOptions; result: ImportSettingsPlan & PlanFields };
  /** apply method; paired with `settings.import.plan`; can delete or overwrite something */
  "settings.import.apply": { params: ApplyPlan | (ImportSettings & WriteOptions); result: SettingsProfile };
  /** apply method; paired with `settings.import.plan`; can delete or overwrite something; an older name for `settings.import.apply` */
  "settings.import": { params: ApplyPlan | (ImportSettings & WriteOptions); result: SettingsProfile };
  /** read method */
  "art.missing": { params: DriveRef; result: TrackRef[] };
  /** read method */
  "art.get": { params: ArtworkRequest; result: OneOrMany_for_ArtworkImage };
  /** read method */
  "art.path": { params: ArtworkRequest; result: OneOrMany_for_ArtworkFile };
  /** plan method; paired with `art.set.apply`; can delete or overwrite something */
  "art.set.plan": { params: SetArtwork & WriteOptions; result: ArtworkSetPlan & PlanFields };
  /** apply method; paired with `art.set.plan`; can delete or overwrite something */
  "art.set.apply": { params: ApplyPlan | (SetArtwork & WriteOptions); result: ArtworkReport };
  /** apply method; paired with `art.set.plan`; can delete or overwrite something; an older name for `art.set.apply` */
  "art.set": { params: ApplyPlan | (SetArtwork & WriteOptions); result: ArtworkReport };
  /** plan method; paired with `art.auto.apply` */
  "art.auto.plan": { params: DriveRef & WriteOptions; result: AutoArtworkPlan & PlanFields };
  /** apply method; paired with `art.auto.plan`; long running: sends `progress`, can be cancelled */
  "art.auto.apply": { params: ApplyPlan | (DriveRef & WriteOptions); result: ArtworkReport };
  /** plan method; paired with `art.embed.apply`; can delete or overwrite something */
  "art.embed.plan": { params: EmbedArtwork & WriteOptions; result: EmbedPlan & PlanFields };
  /** apply method; paired with `art.embed.plan`; can delete or overwrite something */
  "art.embed.apply": { params: ApplyPlan | (EmbedArtwork & WriteOptions); result: EmbedReport };
  /** apply method; paired with `art.embed.plan`; can delete or overwrite something; an older name for `art.embed.apply` */
  "art.embed": { params: ApplyPlan | (EmbedArtwork & WriteOptions); result: EmbedReport };
  /** plan method; paired with `art.unembed.apply`; can delete or overwrite something */
  "art.unembed.plan": { params: UnembedRequest & WriteOptions; result: UnembedPlan & PlanFields };
  /** apply method; paired with `art.unembed.plan`; can delete or overwrite something */
  "art.unembed.apply": { params: ApplyPlan | (UnembedRequest & WriteOptions); result: UnembedReport };
  /** apply method; paired with `art.unembed.plan`; can delete or overwrite something; an older name for `art.unembed.apply` */
  "art.unembed": { params: ApplyPlan | (UnembedRequest & WriteOptions); result: UnembedReport };
  /** immediate method; long running: sends `progress`, can be cancelled */
  "xml.export": { params: ExportXml & WriteOptions; result: XmlReport };
  /** immediate method; long running: sends `progress`, can be cancelled */
  export: { params: ExportPlaylist & WriteOptions; result: ExportReport };
  /** read method */
  diff: { params: DiffRequest; result: PlaylistDiff };
  /** read method; long running: sends `progress`, can be cancelled */
  doctor: { params: DoctorRequest; result: DoctorReport };
  /** plan method; paired with `doctor.orphans.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "doctor.orphans.plan": { params: DriveRef & WriteOptions; result: OrphanPlan & PlanFields };
  /** apply method; paired with `doctor.orphans.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "doctor.orphans.apply": { params: ApplyPlan | (DriveRef & WriteOptions); result: OrphanCleanReport };
  /** read method; long running: sends `progress`, can be cancelled */
  check: { params: CheckRequest; result: CheckReport };
  /** read method; long running: sends `progress`, can be cancelled */
  "analysis.bench": { params: AnalysisBenchRequest; result: BenchReport };
  /** read method */
  "plays.history": { params: DriveRef; result: DriveHistory };
  /** read method; long running: sends `progress`, can be cancelled */
  "plays.archive": { params: HistoryArchiveRequest; result: HistoryArchive };
  /** read method */
  "plays.stats": { params: PlayStatsRequest; result: PlayStats };
  /** read method */
  "players.models": { params: NoParams; result: Model[] };
  /** read method */
  "players.show": { params: NoParams; result: RememberedPlayers };
  /** plan method; paired with `players.set.apply` */
  "players.set.plan": { params: SetPlayers & WriteOptions; result: PlayersPlan & PlanFields };
  /** apply method; paired with `players.set.plan` */
  "players.set.apply": { params: ApplyPlan | (SetPlayers & WriteOptions); result: RememberedPlayers };
  /** apply method; paired with `players.set.plan`; an older name for `players.set.apply` */
  "players.set": { params: ApplyPlan | (SetPlayers & WriteOptions); result: RememberedPlayers };
  /** plan method; paired with `players.clear.apply` */
  "players.clear.plan": { params: ClearPlayers & WriteOptions; result: PlayersPlan & PlanFields };
  /** apply method; paired with `players.clear.plan` */
  "players.clear.apply": { params: ApplyPlan | (ClearPlayers & WriteOptions); result: RememberedPlayers };
  /** apply method; paired with `players.clear.plan`; an older name for `players.clear.apply` */
  "players.clear": { params: ApplyPlan | (ClearPlayers & WriteOptions); result: RememberedPlayers };
  /** read method */
  "disks.list": { params: NoParams; result: DiskView[] };
  /** plan method; paired with `format.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "format.plan": { params: FormatRequest & WriteOptions; result: FormatPlan & PlanFields };
  /** apply method; paired with `format.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "format.apply": { params: ApplyPlan | (FormatApply & WriteOptions); result: FormatReport };
  /** read method */
  "account.status": { params: NoParams; result: AccountStatus };
  /** read method; long running: sends `progress`, can be cancelled */
  "account.check": { params: NoParams; result: AccountInfo };
  /** immediate method */
  "account.login.begin": { params: AccountLoginRequest & WriteOptions; result: LoginStart };
  /** immediate method; long running: sends `progress`, can be cancelled */
  "account.login.finish": { params: AccountLoginFinish & WriteOptions; result: AccountInfo };
  /** immediate method */
  "account.login.paste": { params: AccountLoginPaste & WriteOptions; result: AccountInfo };
  /** immediate method */
  "account.login.cancel": { params: AccountLoginCancel & WriteOptions; result: Cancelled };
  /** immediate method */
  "account.logout": { params: NoParams & WriteOptions; result: LogoutReport };
  /** read method */
  "account.client_metadata": { params: ClientMetadataRequest; result: unknown };
  /** read method; long running: sends `progress`, can be cancelled */
  "profile.build": { params: ProfileRequest; result: ProfileView };
  /** read method; long running: sends `progress`, can be cancelled */
  "profile.document": { params: ProfileRequest; result: ProfileDocument };
  /** read method */
  "profile.prefs": { params: NoParams; result: ProfilePrefs };
  /** immediate method */
  "profile.prefs.set": { params: PrefsUpdate & WriteOptions; result: ProfilePrefs };
  /** plan method; paired with `profile.publish.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "profile.publish.plan": { params: ProfileRequest & WriteOptions; result: PublishPlan & PlanFields };
  /** apply method; paired with `profile.publish.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "profile.publish.apply": { params: ApplyPlan | (ProfileRequest & WriteOptions); result: Published };
  /** plan method; paired with `sets.publish.apply`; can delete or overwrite something */
  "sets.publish.plan": { params: SetPublishRequest & WriteOptions; result: PublishPlan & PlanFields };
  /** apply method; paired with `sets.publish.plan`; can delete or overwrite something */
  "sets.publish.apply": { params: ApplyPlan | (SetPublishRequest & WriteOptions); result: Published };
  /** plan method; paired with `settings.publish.apply`; can delete or overwrite something */
  "settings.publish.plan": { params: NoParams & WriteOptions; result: PublishPlan & PlanFields };
  /** apply method; paired with `settings.publish.plan`; can delete or overwrite something */
  "settings.publish.apply": { params: ApplyPlan | (NoParams & WriteOptions); result: Published };
  /** plan method; paired with `settings.import_published.apply`; can delete or overwrite something */
  "settings.import_published.plan": { params: ImportPublishedSettings & WriteOptions; result: SettingsImportPlan & PlanFields };
  /** apply method; paired with `settings.import_published.plan`; can delete or overwrite something */
  "settings.import_published.apply": { params: ApplyPlan | (ImportPublishedSettings & WriteOptions); result: SettingsProfile };
  /** read method */
  "sets.list": { params: SetsQuery; result: Set[] };
  /** read method */
  "sets.get": { params: SetRef; result: Set };
  /** immediate method; long running: sends `progress`, can be cancelled */
  "sets.new": { params: NewSet & WriteOptions; result: SetChange };
  /** immediate method; long running: sends `progress`, can be cancelled */
  "sets.edit": { params: EditSet & WriteOptions; result: SetChange };
  /** immediate method; can delete or overwrite something */
  "sets.delete": { params: SetRef & WriteOptions; result: Set };
  /** immediate method */
  "sets.export": { params: SetExport & WriteOptions; result: SetExportReport };
  /** read method */
  "sets.played": { params: TrackPlaysQuery; result: Appearance[] };
  /** plan method; paired with `sets.playlist.apply` */
  "sets.playlist.plan": { params: SetPlaylist & WriteOptions; result: EditPlan & PlanFields };
  /** apply method; paired with `sets.playlist.plan` */
  "sets.playlist.apply": { params: ApplyPlan | (SetPlaylist & WriteOptions); result: EditReport };
  /** read method; long running: sends `progress`, can be cancelled */
  "sets.suggest": { params: SuggestSets; result: Suggestions };
  /** read method; long running: sends `progress`, can be cancelled */
  "sessions.list": { params: SessionsRequest; result: Gathered };
  /** immediate method; long running: sends `progress`, can be cancelled */
  "sessions.dismiss": { params: DismissSessions & WriteOptions; result: string[] };
  /** immediate method */
  "sessions.import": { params: ImportSessionLog & WriteOptions; result: ImportedSession };
  /** immediate method; long running: sends `progress`, can be cancelled */
  "mix.index": { params: IndexMixLibrary & WriteOptions; result: IndexReport };
  /** immediate method; long running: sends `progress`, can be cancelled */
  "mix.identify": { params: IdentifyMixRequest & WriteOptions; result: IdentifiedMix };
  /** immediate method; long running: sends `progress`, can be cancelled */
  "mix.export": { params: ExportMix & WriteOptions; result: MixExport };
  /** read method */
  "mix.reports.list": { params: NoParams; result: SavedReport[] };
  /** read method */
  "mix.reports.get": { params: ReportRef; result: IdentifiedMix };
  /** immediate method */
  "mix.reports.html": { params: ReportHtml & WriteOptions; result: ReportPage };
  /** read method; long running: sends `progress`, can be cancelled */
  "fleet.status": { params: FleetRequest; result: FleetStatus };
  /** read method */
  "drives.known": { params: FleetRequest; result: KnownDrive[] };
  /** plan method; paired with `drive.rename.apply` */
  "drive.rename.plan": { params: RenameDrive & WriteOptions; result: RenamePlan & PlanFields };
  /** apply method; paired with `drive.rename.plan` */
  "drive.rename.apply": { params: ApplyPlan | (RenameDrive & WriteOptions); result: RenameReport };
  /** apply method; paired with `drive.rename.plan`; an older name for `drive.rename.apply` */
  "drive.rename": { params: ApplyPlan | (RenameDrive & WriteOptions); result: RenameReport };
  /** immediate method; can delete or overwrite something */
  "drive.forget": { params: DriveRef & WriteOptions; result: DriveRecord };
  /** immediate method */
  "drive.role": { params: SetRole & WriteOptions; result: DriveRecord };
  /** immediate method */
  "drive.retire": { params: RetireDrive & WriteOptions; result: DriveRecord };
  /** read method */
  "drive.log": { params: DriveRef; result: DriveRecord };
  /** read method; long running: sends `progress`, can be cancelled */
  "drive.recognise": { params: DriveRef; result: Recognition };
  /** plan method; paired with `drive.adopt_as.apply`; can delete or overwrite something */
  "drive.adopt_as.plan": { params: AdoptAs & WriteOptions; result: AdoptAsPlan & PlanFields };
  /** apply method; paired with `drive.adopt_as.plan`; can delete or overwrite something */
  "drive.adopt_as.apply": { params: ApplyPlan | (AdoptAs & WriteOptions); result: DeviceInfo };
  /** apply method; paired with `drive.adopt_as.plan`; can delete or overwrite something; an older name for `drive.adopt_as.apply` */
  "drive.adopt_as": { params: ApplyPlan | (AdoptAs & WriteOptions); result: DeviceInfo };
  /** plan method; paired with `drive.reidentify.apply`; can delete or overwrite something */
  "drive.reidentify.plan": { params: Reidentify & WriteOptions; result: ReidentifyPlan & PlanFields };
  /** apply method; paired with `drive.reidentify.plan`; can delete or overwrite something */
  "drive.reidentify.apply": { params: ApplyPlan | (Reidentify & WriteOptions); result: DeviceInfo };
  /** apply method; paired with `drive.reidentify.plan`; can delete or overwrite something; an older name for `drive.reidentify.apply` */
  "drive.reidentify": { params: ApplyPlan | (Reidentify & WriteOptions); result: DeviceInfo };
  /** read method */
  "drive.groups": { params: NoParams; result: DriveSet[] };
  /** read method; an older name for `drive.groups` */
  "drive.sets": { params: NoParams; result: DriveSet[] };
  /** plan method; paired with `drive.eject.apply` */
  "drive.eject.plan": { params: EjectDrives & WriteOptions; result: EjectPlan & PlanFields };
  /** apply method; paired with `drive.eject.plan` */
  "drive.eject.apply": { params: ApplyPlan | (EjectDrives & WriteOptions); result: EjectReport[] };
  /** apply method; paired with `drive.eject.plan`; an older name for `drive.eject.apply` */
  "drive.eject": { params: ApplyPlan | (EjectDrives & WriteOptions); result: EjectReport[] };
  /** read method; long running: sends `progress`, can be cancelled */
  "drive.verify": { params: VerifyRequest; result: ScrubReport };
  /** immediate method; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "drive.repair": { params: RepairRequest & WriteOptions; result: RepairReport };
  /** immediate method; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "drive.fsck": { params: FsCheckRequest & WriteOptions; result: FsCheckReport };
  /** read method; long running: sends `progress`, can be cancelled */
  "drive.space": { params: DriveRef; result: SpaceReport };
  /** plan method; paired with `drive.contact.apply` */
  "drive.contact.plan": { params: SetContact & WriteOptions; result: ContactPlan & PlanFields };
  /** apply method; paired with `drive.contact.plan` */
  "drive.contact.apply": { params: ApplyPlan | (SetContact & WriteOptions); result: ContactReport };
  /** apply method; paired with `drive.contact.plan`; an older name for `drive.contact.apply` */
  "drive.contact": { params: ApplyPlan | (SetContact & WriteOptions); result: ContactReport };
  /** read method */
  "player.show": { params: DriveRef; result: PlayerIdentity };
  /** plan method; paired with `player.set.apply` */
  "player.set.plan": { params: SetPlayerIdentity & WriteOptions; result: PlayerPlan & PlanFields };
  /** apply method; paired with `player.set.plan` */
  "player.set.apply": { params: ApplyPlan | (SetPlayerIdentity & WriteOptions); result: PlayerIdentity };
  /** apply method; paired with `player.set.plan`; an older name for `player.set.apply` */
  "player.set": { params: ApplyPlan | (SetPlayerIdentity & WriteOptions); result: PlayerIdentity };
  /** plan method; paired with `tidy.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "tidy.plan": { params: TidyRequest & WriteOptions; result: TidyPlan & PlanFields };
  /** apply method; paired with `tidy.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "tidy.apply": { params: ApplyPlan | (TidyRequest & WriteOptions); result: TidyReport };
  /** read method */
  "recordings.list": { params: DriveRef; result: Recording[] };
  /** plan method; paired with `recordings.collect.apply` */
  "recordings.collect.plan": { params: CollectRecordings & WriteOptions; result: CollectPlan & PlanFields };
  /** apply method; paired with `recordings.collect.plan`; long running: sends `progress`, can be cancelled */
  "recordings.collect.apply": { params: ApplyPlan | (CollectRecordings & WriteOptions); result: CollectReport };
  /** apply method; paired with `recordings.collect.plan`; long running: sends `progress`, can be cancelled; an older name for `recordings.collect.apply` */
  "recordings.collect": { params: ApplyPlan | (CollectRecordings & WriteOptions); result: CollectReport };
  /** plan method; paired with `firmware.apply` */
  "firmware.plan": { params: FirmwareRequest & WriteOptions; result: FirmwarePlan & PlanFields };
  /** apply method; paired with `firmware.plan`; long running: sends `progress`, can be cancelled */
  "firmware.apply": { params: ApplyPlan | (FirmwareRequest & WriteOptions); result: FirmwareReport };
  /** plan method; paired with `firmware.remove.apply`; can delete or overwrite something */
  "firmware.remove.plan": { params: DriveRef & WriteOptions; result: FirmwareRemovePlan & PlanFields };
  /** apply method; paired with `firmware.remove.plan`; can delete or overwrite something */
  "firmware.remove.apply": { params: ApplyPlan | (DriveRef & WriteOptions); result: string[] };
  /** apply method; paired with `firmware.remove.plan`; can delete or overwrite something; an older name for `firmware.remove.apply` */
  "firmware.remove": { params: ApplyPlan | (DriveRef & WriteOptions); result: string[] };
  /** plan method; paired with `provision.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "provision.plan": { params: ProvisionRequest & WriteOptions; result: ProvisionPlan & PlanFields };
  /** apply method; paired with `provision.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "provision.apply": { params: ApplyPlan | (ProvisionApply & WriteOptions); result: ProvisionReport };
  /** plan method; paired with `refresh.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "refresh.plan": { params: RefreshRequest & WriteOptions; result: JobPlan & PlanFields };
  /** apply method; paired with `refresh.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "refresh.apply": { params: ApplyPlan | (RefreshRequest & WriteOptions); result: JobReport };
  /** plan method; paired with `prep.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "prep.plan": { params: SetJobRequest & WriteOptions; result: JobPlan & PlanFields };
  /** plan method; paired with `prep.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something; an older name for `prep.plan` */
  "set.plan": { params: SetJobRequest & WriteOptions; result: JobPlan & PlanFields };
  /** apply method; paired with `prep.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "prep.apply": { params: ApplyPlan | (SetJobRequest & WriteOptions); result: JobReport };
  /** apply method; paired with `prep.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something; an older name for `prep.apply` */
  "set.apply": { params: ApplyPlan | (SetJobRequest & WriteOptions); result: JobReport };
  /** read method; long running: sends `progress`, can be cancelled */
  "collection.info": { params: CollectionRef; result: CollectionInfo };
  /** read method; long running: sends `progress`, can be cancelled */
  "collection.library": { params: CollectionRef; result: CollectionView };
  /** read method; long running: sends `progress`, can be cancelled */
  "collection.diff": { params: CollectionDiffRequest; result: DriveDiff };
  /** plan method; paired with `interop.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "interop.plan": { params: InteropRequest & WriteOptions; result: InteropPlan & PlanFields };
  /** apply method; paired with `interop.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "interop.apply": { params: ApplyPlan | (InteropRequest & WriteOptions); result: InteropPlan };
}

export type MethodName = keyof Methods;
export type Params<M extends MethodName> = Methods[M]["params"];
export type Result<M extends MethodName> = Methods[M]["result"];

/** Notifications the sidecar sends, by method. */
export interface Notifications {
  event: Notice;
  progress: ProgressParams;
}
