// The locallibrary RPC sidecar's types, made from schema.json by `locallibrary rpc --emit-schema`.
// Don't edit: regenerate it.

/** The protocol these types describe (`initialize` answers it). */
export const PROTOCOL = 1;

/** No parameters (any object is accepted). */
export type NoParams = { [key: string]: unknown };

/** `rpc.describe`. */
export type DescribeRequest = {
  /** One method (or an alias of one); default every method. */
  method?: string | null;
};

/** `$/cancel`: stop a running request at its next safe point. */
export type CancelRequest = {
  /** The request's id (a notification, which has none, can't be cancelled). */
  id?: RequestId | null;
};

/**
 * A JSON-RPC request's id: a string or a number. Only describes ids in the schema: they're
 * passed through as they came.
 */
export type RequestId = number | string;

/** `events.subscribe`. */
export type Subscribe = {
  /** Event kinds (`drive.attached`) or groups (`drive`, `store`, `job`, `watch`); empty: all. */
  kinds?: string[];
  /**
   * How often drives and the store are looked at, in milliseconds (at least 100; default
   * 1000).
   */
  interval_ms?: number | null;
};

/** `watch.start`'s parameters (see [`super::WatchOptions`]). */
export type StartWatch = {
  /** Pull drives that hold changes the store lacks. */
  pull?: boolean;
  /** Then sync the rest of the drive's group from it (`sync_set`, the old name, works too). */
  sync_group?: boolean;
  /** How long a drive must go unchanged before it's looked at, in milliseconds. */
  settle_ms?: number | null;
  /** How often drives are polled, in milliseconds (at least 100). */
  interval_ms?: number | null;
};

/** `watch.stop`. */
export type StopWatch = {
  /** Stop what it's doing too, instead of letting it finish. */
  cancel?: boolean;
};

/** `shutdown`. */
export type Shutdown = {
  /** Cancel running requests instead of waiting for them. */
  cancel?: boolean;
};

export type ListDrives = {
  /** Include mounted volumes without a rekordbox export (possible sync targets). */
  all?: boolean;
};

export type DriveRef = {
  drive: string;
};

export type AdoptDrive = {
  drive: string;
  name?: string | null;
};

/** Accepted by every method that writes (with its other parameters). */
export type WriteOptions = {
  /** Allow writing to archive and retired drives, which are refused (`conflict`) otherwise. */
  overwrite_archive?: boolean;
};

/** `*.apply` by the id of the plan `*.plan` returned: applies exactly that plan, once. */
export type ApplyPlan = {
  plan_id: string;
};

/** `drive.library`, optionally paged: without `offset` or `limit` every track comes, as before. */
export type LibraryRequest = {
  drive: string;
  /**
   * The whole library, as exports have it: on a drive with both databases, with the tracks,
   * playlists and history only OneLibrary holds added. Those tracks' ids have the top bit set
   * (2^31 and up); edits refuse them.
   */
  combined?: boolean;
  offset?: number | null;
  limit?: number | null;
};

export type LibraryRef = {
  drive: string;
  /**
   * The whole library, as exports have it: on a drive with both databases, with the tracks,
   * playlists and history only OneLibrary holds added. Those tracks' ids have the top bit set
   * (2^31 and up); edits refuse them.
   */
  combined?: boolean;
};

export type TracksGet = {
  drive: string;
  /**
   * The whole library, as exports have it: on a drive with both databases, with the tracks,
   * playlists and history only OneLibrary holds added. Those tracks' ids have the top bit set
   * (2^31 and up); edits refuse them.
   */
  combined?: boolean;
  ids: number[];
};

export type TracksQuery = {
  drive: string;
  /**
   * The whole library, as exports have it: on a drive with both databases, with the tracks,
   * playlists and history only OneLibrary holds added. Those tracks' ids have the top bit set
   * (2^31 and up); edits refuse them.
   */
  combined?: boolean;
  /** See [`crate::query`]. */
  filter?: string | null;
  /** Only tracks in this playlist, in its order (unless `sort` is given). */
  playlist?: string | null;
  /** E.g. `key,bpm` or `-rating`. */
  sort?: string | null;
  offset?: number;
  limit?: number | null;
};

export type AnalysisRequest = {
  drive: string;
  /**
   * The whole library, as exports have it: on a drive with both databases, with the tracks,
   * playlists and history only OneLibrary holds added. Those tracks' ids have the top bit set
   * (2^31 and up); edits refuse them.
   */
  combined?: boolean;
  track: number;
  waveform?: WaveformChoice;
  encoding?: WaveformEncoding;
};

/** Which waveforms `track.analysis` and `tracks.analysis` send. */
export type WaveformChoice =
  | ("none" | "all")
  /** The whole-track overview. */
  | "preview"
  /** The scrolling one, which is large: about 150 columns a second. */
  | "detail";

/** How waveforms are sent. */
export type WaveformEncoding =
  /** Each column as an object: the original shape, large (4-5 MB for a scrolling waveform). */
  | "json"
  /** Packed bytes in base64 ([`PackedWaveform`]), a few bytes a column. */
  | "base64";

/** [`AnalysisRequest`] for several tracks, without waveforms unless asked. */
export type TracksAnalysisRequest = {
  drive: string;
  /**
   * The whole library, as exports have it: on a drive with both databases, with the tracks,
   * playlists and history only OneLibrary holds added. Those tracks' ids have the top bit set
   * (2^31 and up); edits refuse them.
   */
  combined?: boolean;
  /** At most 100 ([`MAX_ANALYSES`]). */
  ids: number[];
  /**
   * `none` (the default) or `preview`: scrolling waveforms, megabytes each, come one track at
   * a time from `track.analysis`.
   */
  waveform?: WaveformChoice;
  encoding?: WaveformEncoding;
};

export type PluginFieldsRequest = {
  drive: string;
  /**
   * The whole library, as exports have it: on a drive with both databases, with the tracks,
   * playlists and history only OneLibrary holds added. Those tracks' ids have the top bit set
   * (2^31 and up); edits refuse them.
   */
  combined?: boolean;
  /** Only these tracks, in this order (every track when left out). */
  tracks?: number[] | null;
  /** Only this plugin's fields. */
  plugin?: string | null;
};

/**
 * Which tracks an operation targets: the tracks that fit every field given, so a playlist and a
 * filter together pick the playlist's tracks the filter matches. At least one field must be set,
 * and a blank query, filter, album, playlist or history list is refused rather than taken to
 * mean every track.
 */
export type TrackDetailsRequest = {
  drive: string;
  /**
   * The whole library, as exports have it: on a drive with both databases, with the tracks,
   * playlists and history only OneLibrary holds added. Those tracks' ids have the top bit set
   * (2^31 and up); edits refuse them.
   */
  combined?: boolean;
  ids?: number[];
  /** Matches "artist - title" or path substrings; must match at least one track. */
  query?: string | null;
  playlist?: string | null;
  /** Every track whose album has this name. */
  album?: string | null;
  /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
  filter?: string | null;
  /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
  paths?: string[];
  /** The tracks of a history list (what a player recorded), by name. */
  history?: string | null;
};

export type MatchesRequest = {
  drive: string;
  /**
   * The whole library, as exports have it: on a drive with both databases, with the tracks,
   * playlists and history only OneLibrary holds added. Those tracks' ids have the top bit set
   * (2^31 and up); edits refuse them.
   */
  combined?: boolean;
  track: number;
  limit?: number | null;
};

export type RestoreBackup = {
  drive: string;
  backup?: string | null;
  /** Restore even if the backup references files that are gone. */
  force?: boolean;
};

export type UndoListRequest = {
  /** Only this drive [default: every mounted drive]. */
  drive?: string | null;
};

export type UndoRequest = {
  /** The drive [default: the only mounted drive with backups]. */
  drive?: string | null;
  /** A backup other than the one `undo` takes (see [`super::list_backups`]). */
  backup?: string | null;
  /**
   * Put it back even if the drive changed since its change, or its database lists audio
   * that's no longer on the drive (the library was replaced since).
   */
  force?: boolean;
};

export type ComparePair = {
  left: string;
  right: string;
};

export type SyncRequest = {
  left: string;
  right: string;
  direction?: Direction;
  /** Accept a low-confidence "newer" verdict in auto mode. */
  allow_low_confidence?: boolean;
  verify?: boolean;
  keep_extra?: boolean;
  delete_first?: boolean;
  /** Write to an archive or retired drive (see [`crate::fleet::Role`]). */
  overwrite_archive?: boolean;
  /**
   * With an explicit direction: the target came back from a set, so save it in the store
   * first (as an automatic sync does; see [`SyncPreview::after_set`]).
   */
  keep_player_changes?: boolean;
};

export type Direction =
  /** Copy from whichever side is newer (refuses when that can't be determined). */
  | "auto"
  /** Copy left -> right regardless. */
  | "left_to_right"
  /** Copy right -> left regardless. */
  | "right_to_left";

/** Sync any number of drives: the single newest one is copied onto all the others. */
export type SyncManyRequest = {
  /** Drives, or a library's name for its mounted clones (but archive and retired ones). */
  drives: string[];
  allow_low_confidence?: boolean;
  verify?: boolean;
  keep_extra?: boolean;
  delete_first?: boolean;
  overwrite_archive?: boolean;
};

/**
 * Sync a drive group: every mounted member of a library's group (not archive or retired drives)
 * from the newest of them, which should be the primary.
 */
export type SyncSetRequest = {
  /**
   * The library whose group to sync [default: the only group with two or more drives mounted;
   * with several, refused as `choose_library`, listing them].
   */
  library?: string | null;
  allow_low_confidence?: boolean;
  verify?: boolean;
  keep_extra?: boolean;
  delete_first?: boolean;
};

export type PullRequest = {
  drive: string;
  /** The library to pull into [default: the drive's, else a new one named after the drive]. */
  library?: string | null;
  /**
   * Start a new library when no library has the name `library` gives. Without it, an unknown
   * name is refused, so a typo doesn't start a new library.
   */
  new_library?: boolean;
  message?: string | null;
  /**
   * Label the snapshot (see [`Store::add_label`]); when the drive is already saved, the label
   * goes on the snapshot it matches.
   */
  label?: string | null;
  force?: boolean;
};

export type PushRequest = {
  drive: string;
  library?: string | null;
  snapshot?: string | null;
  force?: boolean;
  verify?: boolean;
  keep_extra?: boolean;
  delete_first?: boolean;
};

export type MergeRequest = {
  source: string;
  target: string;
  playlists?: PlaylistMode;
  /** Folder name for [`PlaylistMode::Folder`] (default: the source drive's name). */
  folder_name?: string | null;
  /**
   * Only merge these source playlists (paths like `Sets/Warmup` or unique names) and the
   * tracks in them. Empty = everything.
   */
  only_playlists?: string[];
  match_mode?: MatchMode;
  /**
   * Also bring the source's history playlists (what players recorded during a set) across,
   * except ones the target already has.
   */
  history?: boolean;
};

export type PlaylistMode =
  /** Recreate the source's playlist tree inside a new folder (default). */
  | "folder"
  /** Merge into playlists with the same folder path, creating missing ones. */
  | "by_name"
  /** Only add tracks. */
  | "none";

export type MatchMode =
  /** Same path + size, or same artist/title/duration (default). */
  | "metadata"
  /** Same path + size only. */
  | "path";

export type ConvertRequest = {
  drive: string;
  to: Target;
  /**
   * Rebuild the target database if the drive already has one, dropping what only it has.
   * Otherwise it keeps what it has and gains what it lacks.
   */
  replace?: boolean;
};

/** Which database to build. */
export type Target =
  /** `exportLibrary.db`, from `export.pdb`. */
  | "one_library"
  /** `export.pdb`, from `exportLibrary.db`. */
  | "device_library";

export type HistoryRequest = {
  library: string;
  limit?: number | null;
};

/** `store.gc` with a request (rather than a `plan_id`). */
export type GcRequest = {
  /** Say what would go, without deleting it. */
  dry_run?: boolean;
};

export type PruneOptions = {
  /** Keep each library's newest N snapshots. */
  keep_last?: number | null;
  /** Keep the newest snapshot of each of the last N days that have one. */
  keep_daily?: number | null;
  /** Keep the newest snapshot of each of the last N weeks that have one. */
  keep_weekly?: number | null;
  /** Only this library (name or id) [default: every library]. */
  library?: string | null;
  /** Say what would go, and remove nothing (for callers that plan and prune in one go). */
  dry_run?: boolean;
};

export type StoreCopyRequest = {
  /** Where the copy goes: a folder on a NAS, another disk, a cloud-synced folder. */
  path: string;
  /**
   * Also remove from the copy what the store no longer has (pruned or collected snapshots,
   * and files nothing in the store uses).
   */
  mirror?: boolean;
  /** Re-hash the files the copy has already, copying again any that changed. */
  verify?: boolean;
  /**
   * Copy over a copy that was written to since it was last copied to (with `--store`), or that
   * has snapshots the store doesn't.
   */
  force?: boolean;
  /** Say what would be copied, and copy nothing. */
  dry_run?: boolean;
};

export type LabelRequest = {
  label: string;
  /** The snapshot to label: an id prefix or another label. Not needed with `remove`. */
  snapshot?: string | null;
  /** Take the label off instead. */
  remove?: boolean;
};

export type RenameLibrary = {
  /** The library's name (any case) or id. */
  library: string;
  name: string;
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
 * One change to a drive's library. Playlists are given as paths (`Folder/Name`) or unique
 * names; positions in a playlist count from 1.
 */
export type Edit =
  /**
   * Create a playlist (or folder) at `path`, creating missing folders above it. Nothing
   * happens if it exists already.
   */
  | {
    op: "create_playlist";
    path: string;
    folder?: boolean;
  }
  | {
    op: "rename_playlist";
    playlist: string;
    name: string;
  }
  /**
   * Move a playlist or folder into folder `into` (the top level when absent or empty), at
   * `position` among its new siblings (the end when absent).
   */
  | {
    op: "move_playlist";
    playlist: string;
    into?: string | null;
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
    op: "add_tracks";
    playlist: string;
    tracks: TrackSelector;
    position?: number | null;
    /** Order the added tracks (see [`query::parse_sort`]). */
    sort?: string | null;
    allow_repeats?: boolean;
  }
  /** Remove every entry of the selected tracks and/or the entries at `positions`. */
  | {
    op: "remove_tracks";
    playlist: string;
    tracks?: TrackSelector | null;
    positions?: number[];
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
    op: "set_tracks";
    playlist: string;
    tracks: TrackSelector;
    sort?: string | null;
    limit?: number | null;
    create?: boolean;
  }
  /**
   * Make a playlist's entries exactly these audio files (device paths) in this order, repeats
   * and all; none empties it. With `create`, the playlist at exactly that path, made (with
   * missing folders above it) if missing. Imported tracklists are made this way. The journal
   * records these edits this way: filling a playlist (`SetTracks`), ordering it for mixing
   * (`PlanSet`), and removing or moving entries whose files have other entries in it too.
   */
  | {
    op: "set_entries";
    playlist: string;
    paths: string[];
    create?: boolean;
  }
  /** Reorder a playlist (see [`query::parse_sort`]). */
  | {
    op: "sort_playlist";
    playlist: string;
    by: string;
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
   * Order a playlist for mixing (see [`harmony::order_set`], with how sure each key is from
   * the keys an analyzer heard already, see [`crate::sound::key_confidences`]): in place, or
   * into a new playlist at `into`.
   */
  | {
    op: "plan_set";
    playlist: string;
    start?: TrackSelector | null;
    into?: string | null;
  }
  | {
    op: "edit_tracks";
    tracks: TrackSelector;
    fields: TrackFields;
  }
  /**
   * Delete tracks with their playlist and history entries, and (unless `keep_files`) their
   * audio, analysis and artwork files when no other track uses them. A track goes with every
   * row of its audio file, in each database; a path may name one only the drive's other
   * database lists (OneLibrary, drifted from `export.pdb`), which is deleted there.
   */
  | {
    op: "delete_tracks";
    tracks: TrackSelector;
    keep_files?: boolean;
  }
  /**
   * Replace one copy of a recording with another: playlist and history entries (and My Tags)
   * move to `keep`, then `duplicate` is deleted unless `keep_duplicate`.
   */
  | {
    op: "replace_track";
    duplicate: TrackSelector;
    keep: TrackSelector;
    keep_duplicate?: boolean;
  }
  /**
   * Point one track at another audio file on the drive (a device path), as when its file was
   * moved or renamed: in both databases and in the analysis files that record it. With `file`,
   * the file is another encoding of the recording (a lossless copy of an MP3): the rows take
   * its format, size, sample rate, bit depth and bit rate, and with `shift_ms` its cues and
   * beat grid move by that much, for audio that starts at another point.
   */
  | {
    op: "set_path";
    track: TrackSelector;
    path: string;
    file?: AudioFile | null;
    shift_ms?: number;
  }
  /** Delete history lists (what players recorded) by name, or all of them. */
  | {
    op: "delete_history";
    names?: string[];
    all?: boolean;
  }
  /**
   * Delete history lists, each the one with exactly its name and entries. The journal records
   * deleting history lists this way, so a replay leaves lists players recorded since, under a
   * reused name too.
   */
  | {
    op: "delete_history_lists";
    lists: HistoryListName[];
  }
  /**
   * A My Tag edit (see [`crate::tags`]), its tracks named by file. `tags` edits are recorded
   * this way too.
   */
  | (TagChange & {
    op: "my_tag";
  })
  /**
   * A cue edit (see [`crate::cue_edit`]) on the tracks of these audio files (device paths).
   * `cues` edits are recorded this way too.
   */
  | {
    op: "cues";
    tracks: string[];
    change: CueChange;
  }
  /**
   * Give tracks this artwork, scaled to the sizes players show (see [`crate::artwork`]). The
   * journal records the tracks by file and the image as staged on the drive.
   */
  | {
    op: "set_artwork";
    tracks: TrackSelector;
    image: ImageSource;
  }
  /**
   * Write a beat grid, phrases or a waveform into the analysis files of the track of this
   * audio file (a device path, see [`crate::analysis_edit`]). Planned, a grid brings the
   * track's BPM with it (an `edit_tracks`); the journal records a waveform as staged on the
   * drive.
   */
  | {
    op: "set_analysis";
    track: string;
    /** The beat grid: each beat's number in its bar (1-4), the tempo there and its time. */
    beats?: Beat[] | null;
    /** The phrases, named as the track's mood names them. */
    phrases?: SongStructure | null;
    waveform?: WaveSource | null;
  };

/**
 * Which tracks an operation targets: the tracks that fit every field given, so a playlist and a
 * filter together pick the playlist's tracks the filter matches. At least one field must be set,
 * and a blank query, filter, album, playlist or history list is refused rather than taken to
 * mean every track.
 */
export type TrackSelector = {
  ids?: number[];
  /** Matches "artist - title" or path substrings; must match at least one track. */
  query?: string | null;
  playlist?: string | null;
  /** Every track whose album has this name. */
  album?: string | null;
  /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
  filter?: string | null;
  /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
  paths?: string[];
  /** The tracks of a history list (what a player recorded), by name. */
  history?: string | null;
};

/**
 * New values for track details; fields left out stay as they are, and an empty name clears
 * artist, album, genre, label, key and the like (an empty ISRC or release date clears it too).
 */
export type TrackFields = {
  title?: string | null;
  artist?: string | null;
  album?: string | null;
  /** The databases keep it with the album, so a track needs one (given here too, or its own). */
  album_artist?: string | null;
  genre?: string | null;
  label?: string | null;
  key?: string | null;
  remixer?: string | null;
  original_artist?: string | null;
  composer?: string | null;
  comment?: string | null;
  mix_name?: string | null;
  /** 0-5 stars. */
  rating?: number | null;
  /** A colour name (`aqua`), rekordbox's number (1-8), or `none`. */
  color?: string | null;
  year?: number | null;
  play_count?: number | null;
  track_number?: number | null;
  disc_number?: number | null;
  /**
   * 1-999.99, kept to hundredths as rekordbox keeps it. The beat grid in the analysis files,
   * which players count beats by, stays as it is.
   */
  bpm?: number | null;
  /** Like `GBDUW0000059`: two letters, three letters or digits, and seven digits. */
  isrc?: string | null;
  /** `YYYY-MM-DD`. */
  release_date?: string | null;
};

/** What a track's rows say about its audio file, for a track pointed at another encoding. */
export type AudioFile = {
  size: number;
  sample_rate: number;
  bit_depth: number;
  /** kbps. */
  bitrate: number;
};

/** A history list as the journal names it. */
export type HistoryListName = {
  name: string;
  /** Its entries' audio files (device paths), in order. */
  tracks: string[];
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

/**
 * A cue edit as the drive's journal records it (see [`crate::edit::journal`]): a copy's source
 * by its audio file (a device path), like the tracks edited, so a replay on a later export, where
 * ids differ, finds them.
 */
export type CueChange =
  | {
    set: SetCue;
  }
  /** Several cues set in turn, each as `set` sets one (what an import adds to a track). */
  | {
    set_many: SetCue[];
  }
  | {
    update: UpdateCue;
  }
  | {
    delete: CueRef;
  }
  | {
    copy: {
      from: string;
    };
  }
  | {
    carry: {
      from: string;
      shift_ms: number;
    };
  };

/**
 * Add a cue, or replace the hot cue in its slot (or the memory cue at its position). A replaced
 * cue keeps its name and colour unless new ones are given.
 */
export type SetCue = {
  /** The hot cue slot; a memory cue when absent. */
  hot_cue?: HotCue | null;
  at: CuePosition;
  /** Move a time to the nearest beat of the grid. */
  snap?: boolean;
  /** Make it a loop this many beats long (a whole number, or 0.5, 0.25...). */
  loop_beats?: number | null;
  name?: string | null;
  /** See [`set_color`]. */
  color?: string | null;
};

/** A hot cue slot, A-H (written as its letter). */
export type HotCue = string;

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

/** Rename and/or recolour cues (`""` clears a name, `"none"` a colour). */
export type UpdateCue = {
  cue: CueRef;
  name?: string | null;
  color?: string | null;
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

/** The image an artwork edit gives tracks (see [`crate::edit::Edit::SetArtwork`]). */
export type ImageSource =
  /** An image file on this computer, or an audio file whose embedded art to use. */
  | {
    path: string;
  }
  /** The image file's bytes, in base64. */
  | {
    base64: string;
  }
  /**
   * An image staged on the drive (`.locallibrary/artwork/<hash>.jpg`, a JPEG of the larger
   * size players show), by its hash: how the journal records the others.
   */
  | {
    staged: Hash;
  };

/** A BLAKE3 hash, as 64 lowercase hex characters. */
export type Hash = string;

export type Beat = {
  /** Position in the bar, 1-4. */
  beat_number: number;
  bpm: number;
  time_ms: number;
};

/** rekordbox's phrase analysis (`PSSI`, in `.EXT`). */
export type SongStructure = {
  mood: Mood;
  phrases: Phrase[];
};

/** rekordbox's overall classification of a track, which decides how phrases are named. */
export type Mood = "high" | "mid" | "low";

export type Phrase = {
  kind: PhraseKind;
  /** 1-based beat numbers into the beat grid; the phrase runs up to (not including) `end_beat`. */
  start_beat: number;
  end_beat: number;
  /** Beat where a fill-in leads into the next phrase. */
  fill_beat?: number | null;
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

/**
 * A waveform an analysis edit writes: three bands, each column's lows, mids and highs (0-255),
 * 150 columns a second ([`Bands`]), from which each of the track's waveform tags is worked out.
 */
export type WaveSource =
  /** The bands packed (three bytes a column), in base64. */
  | {
    base64: string;
  }
  /**
   * Staged on the drive (`.locallibrary/waveforms/<hash>.bands`), by its hash: how the journal
   * records the others.
   */
  | {
    staged: Hash;
  };

export type ClearEdits = {
  drive: string;
  /** Only entries recorded before this time. */
  before?: string | null;
};

export type SetSmartPlaylist = {
  path: string;
  filter: string;
  sort?: string | null;
  limit?: number | null;
  /**
   * Only for these libraries, or the libraries of these drives (names, ids or mount paths);
   * every drive when empty.
   */
  drives?: string[];
};

export type RemoveSmartPlaylist = {
  path: string;
};

export type DedupeRequest = {
  drive: string;
  /** Also find copies by audio content (see [`crate::doctor`]). */
  hash_audio?: boolean;
  /**
   * Also find copies by their sound: the same recording in another file or encoding (see
   * [`crate::doctor`]).
   */
  by_sound?: boolean;
  /**
   * Also replace tracks that only share artist, title and duration (they may be different
   * edits of a recording).
   */
  metadata?: boolean;
};

export type RelocateRequest = {
  drive: string;
  /**
   * Folders to look in besides the drive (on this computer or another drive), with
   * everything below them.
   */
  folders?: string[];
};

export type UpgradeRequest = {
  drive: string;
  /** Folders of lossless files (on this computer or another drive), with everything below them. */
  folders: string[];
  /** Delete the lossy files the tracks used (they're backed up first). */
  remove_old?: boolean;
  /**
   * The players the drive is for: a lossless file one of them can't play isn't used. None: the
   * ones remembered for the drive's library (none remembered: not checked).
   */
  players?: string[];
};

export type DeleteHistoryRequest = {
  drive: string;
  /** History lists by name; all of them when empty. */
  names?: string[];
  /** Delete lists the store hasn't kept (`locallibrary pull` keeps them). */
  force?: boolean;
};

export type PackRequest = {
  source: string;
  target: string;
  /**
   * Playlists or folders (paths) to put on the target. Empty: the selection the target was
   * last packed with.
   */
  playlists?: string[];
  /** Keep the source's history lists too. */
  history?: boolean;
  /** Leave files on the target that the packed library doesn't use. */
  keep_extra?: boolean;
  /** Write to an archive or retired drive (see [`crate::fleet::check_target`]). */
  overwrite_archive?: boolean;
  /**
   * Pack tracks the players can't play as lossless copies they can.
   * Without playlists, as the target was last packed.
   */
  transcode?: Transcode | null;
};

/** Which players a packed drive's copies are for, and how they're made. */
export type Transcode = {
  /** Player models (see `players models`); none: the ones remembered for the source's library. */
  players?: string[];
  /** The container copies are written in: AIFF unless given. */
  container?: Container;
  /**
   * Also decode lossy files the players can't play (an MP3 at 22.05 kHz) into lossless copies:
   * bigger, but no further loss. Lossy files are never encoded lossy again.
   */
  decode_lossy?: boolean;
};

/** A lossless container a copy is written in. */
export type Container =
  | ("wav" | "flac")
  /** rekordbox and every player read its ID3 tags. */
  | "aiff";

/** A My Tag edit: `{"drive": "Home", "action": "add", "tag": "Genre/Deep House", "query": "..."}`. */
export type TagsEdit = {
  drive: string;
} & (
  /** Give the selected tracks a tag (`Category/Tag`), creating it where it's missing. */
  | {
    action: "add";
    tag: string;
    ids?: number[];
    /** Matches "artist - title" or path substrings; must match at least one track. */
    query?: string | null;
    playlist?: string | null;
    /** Every track whose album has this name. */
    album?: string | null;
    /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
    filter?: string | null;
    /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
    paths?: string[];
    /** The tracks of a history list (what a player recorded), by name. */
    history?: string | null;
  }
  /** Take a tag off the selected tracks. */
  | {
    action: "remove";
    tag: string;
    ids?: number[];
    /** Matches "artist - title" or path substrings; must match at least one track. */
    query?: string | null;
    playlist?: string | null;
    /** Every track whose album has this name. */
    album?: string | null;
    /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
    filter?: string | null;
    /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
    paths?: string[];
    /** The tracks of a history list (what a player recorded), by name. */
    history?: string | null;
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

/**
 * Which tracks an operation targets: the tracks that fit every field given, so a playlist and a
 * filter together pick the playlist's tracks the filter matches. At least one field must be set,
 * and a blank query, filter, album, playlist or history list is refused rather than taken to
 * mean every track.
 */
export type ListCues = {
  drive: string;
  /**
   * The whole library, as exports have it: on a drive with both databases, with the tracks,
   * playlists and history only OneLibrary holds added. Those tracks' ids have the top bit set
   * (2^31 and up); edits refuse them.
   */
  combined?: boolean;
  ids?: number[];
  /** Matches "artist - title" or path substrings; must match at least one track. */
  query?: string | null;
  playlist?: string | null;
  /** Every track whose album has this name. */
  album?: string | null;
  /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
  filter?: string | null;
  /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
  paths?: string[];
  /** The tracks of a history list (what a player recorded), by name. */
  history?: string | null;
};

/**
 * Which tracks an operation targets: the tracks that fit every field given, so a playlist and a
 * filter together pick the playlist's tracks the filter matches. At least one field must be set,
 * and a blank query, filter, album, playlist or history list is refused rather than taken to
 * mean every track.
 */
export type EditCues = {
  drive: string;
  ids?: number[];
  /** Matches "artist - title" or path substrings; must match at least one track. */
  query?: string | null;
  playlist?: string | null;
  /** Every track whose album has this name. */
  album?: string | null;
  /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
  filter?: string | null;
  /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
  paths?: string[];
  /** The tracks of a history list (what a player recorded), by name. */
  history?: string | null;
  edit: CueOp;
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
    op: "delete";
    cue: CueRef;
  }
  /** Replace the selected tracks' cues with a copy of one other track's. */
  | {
    op: "copy";
    from: TrackSelector;
  };

export type PlaylistRestoreRequest = {
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
  /** Take it from one of the drive's backups instead (see [`super::list_backups`]). */
  backup?: string | null;
};

export type SetSettings = {
  /** Setting key -> value id (see [`settings::SCHEMA`]). */
  values: { [key: string]: string };
  /** Make these per-drive overrides instead of base values. */
  drive?: string | null;
};

export type UnsetSettings = {
  keys: string[];
  drive?: string | null;
};

/** Writing the settings profile to several drives. */
export type SettingsApplyRequest = {
  /**
   * Drives, or libraries: each of a library's mounted clones (settings are each drive's own) but
   * archive and retired ones [default: every mounted rekordbox drive but archive and retired
   * ones].
   */
  drives?: string[];
  /** Write all four files even when the profile doesn't mention their settings. */
  create_missing?: boolean;
};

export type ApplySettings = {
  drive: string;
  dry_run?: boolean;
  /** Write all four files even when the profile doesn't mention their settings. */
  create_missing?: boolean;
};

export type ImportSettings = {
  /** Drive to import from; `None` imports rekordbox's own "My Settings" on this computer. */
  drive?: string | null;
};

/** One track (`track`) or several (`tracks`). */
export type ArtworkRequest = {
  drive: string;
  /**
   * The whole library, as exports have it: on a drive with both databases, with the tracks,
   * playlists and history only OneLibrary holds added. Those tracks' ids have the top bit set
   * (2^31 and up); edits refuse them.
   */
  combined?: boolean;
  track: number;
} | {
  drive: string;
  /**
   * The whole library, as exports have it: on a drive with both databases, with the tracks,
   * playlists and history only OneLibrary holds added. Those tracks' ids have the top bit set
   * (2^31 and up); edits refuse them.
   */
  combined?: boolean;
  tracks: number[];
};

/**
 * Which tracks an operation targets: the tracks that fit every field given, so a playlist and a
 * filter together pick the playlist's tracks the filter matches. At least one field must be set,
 * and a blank query, filter, album, playlist or history list is refused rather than taken to
 * mean every track.
 */
export type SetArtwork = {
  drive: string;
  /** Image file, or an audio file whose embedded art to use. */
  image: string;
  ids?: number[];
  /** Matches "artist - title" or path substrings; must match at least one track. */
  query?: string | null;
  playlist?: string | null;
  /** Every track whose album has this name. */
  album?: string | null;
  /** A filter (see [`crate::query`]) with at least one term; must match at least one track. */
  filter?: string | null;
  /** Device paths of the audio files (`/Contents/...`), as stable across exports. */
  paths?: string[];
  /** The tracks of a history list (what a player recorded), by name. */
  history?: string | null;
};

export type EmbedArtwork = {
  files: string[];
  image: string;
};

export type UnembedRequest = {
  /** Which embed to take back [default: the latest]. */
  backup?: string | null;
};

export type ExportPlaylist = {
  drive: string;
  /**
   * Playlist or folder (path like `Folder/Name`, or a unique name). The whole library when
   * neither this nor `history` is given.
   */
  playlist?: string | null;
  /** A history list (name or path) instead. */
  history?: string | null;
  /** Also look for `history` among the lists archived in the store. */
  archived?: boolean;
  /** Default: from `output`'s extension, else M3U8. */
  format?: ExportFormat | null;
  /** The file to write; with `split`, the folder to write into. */
  output: string;
  paths?: PathStyle;
  /** For a folder: one file per playlist below `output`, instead of one file with every track. */
  split?: boolean;
  /** Replace files already there (refused otherwise). */
  replace?: boolean;
};

export type ExportFormat = "m3u8" | "csv" | "txt";

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

export type DiffRequest = {
  /**
   * A library (its newest drive plugged in, else its latest snapshot), a drive, or
   * `snapshot:<id prefix>`.
   */
  left: string;
  right: string;
  /** Only this playlist, or the playlists in this folder. */
  playlist?: string | null;
};

export type DoctorRequest = {
  drive: string;
  /**
   * Also find duplicates by audio content. Only files whose size matches another track's are
   * hashed, through the drive's hash cache.
   */
  hash_audio?: boolean;
  /**
   * Also find the same recording in other files and encodings (a FLAC and an MP3 of it) by its
   * sound: every track is fingerprinted (once: the store keeps the prints) and tracks of about
   * the same length are compared. Decoding a library's audio the first time takes a while.
   */
  by_sound?: boolean;
  /** Sections to run (all but [`Section::Audio`] when empty). */
  only?: DoctorSection[];
  /** Also run [`Section::Audio`]. */
  audio?: boolean;
  /** Also run [`Section::Analysis`]. */
  analyze?: boolean;
  /**
   * Duplicates only by file, audio, sound or Sync Manager copy: leave out tracks that only share
   * artist, title and length (they may be different edits), also from groups the others join.
   */
  sure_only?: boolean;
};

export type DoctorSection =
  | ("duplicates" | "missing" | "orphans" | "unfiled" | "drift" | "large_files" | "compatibility")
  /** What plugins' checks find on the drive (see [`crate::plugin::Plugins::run_checks`]). */
  | "checks"
  /**
   * Audio files cut short or that don't decode. Only when asked for (`audio`, or `only`
   * naming it): it reads every file's last seconds.
   */
  | "audio"
  /**
   * Stored tempos and keys an analysis of the audio disagrees with, and tracks too loud,
   * clipped or far from the library's level. Only when asked for (`analyze`, or `only` naming
   * it): from what the analyzers chosen for tempo, key and loudness kept, which the app runs on
   * the tracks not analysed before, first.
   */
  | "analysis";

export type CheckRequest = {
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
  /**
   * Also open every audio file and decode its last seconds, to find files that are cut short
   * or don't decode. It reads a little of every file, so it takes a while on a big drive.
   */
  audio?: boolean;
};

export type AnalysisBenchRequest = {
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
  /** The datasets' audio to analyse. Without it, the drive's tracks are. */
  audio?: string | null;
};

export type HistoryArchiveRequest = {
  /** Only this library (name or id). */
  library?: string | null;
  /** Only the library this drive is linked to (every library when it isn't linked). */
  drive?: string | null;
};

export type PlayStatsRequest = {
  /**
   * A library (its newest drive plugged in, else its latest snapshot), a drive, or
   * `snapshot:<id prefix>`.
   */
  source: string;
  /**
   * Also report what was played since this snapshot (a label, `snapshot:<id prefix>` or an id
   * prefix), or since this date (`2026`, `2026-09` or `2026-09-30`: since the newest snapshot
   * of the source's library from before it).
   */
  since?: string | null;
  /** How many of the most played tracks to list (default 20). */
  limit?: number | null;
};

export type SetPlayers = {
  /** Model names (`CDJ-3000X`, `cdj-2000nxs2`...); stored as the table spells them. */
  models: string[];
  /** The library (name or id) they're for; without one, every library without models of its own. */
  library?: string | null;
};

export type ClearPlayers = {
  /** The library (name or id) whose models to forget; without one, the default. */
  library?: string | null;
};

export type FormatRequest = {
  /**
   * A mounted drive (path, or an adopted drive's name or id) or a disk (`disk4`, `/dev/sdb`,
   * `PhysicalDrive2`).
   */
  drive: string;
  /** Default: FAT32, which every player reads. */
  filesystem?: Filesystem | null;
  /** Volume label. Default: the drive's locallibrary name, else its current label. */
  label?: string | null;
  /** Bytes per cluster. Default: [`Filesystem::default_cluster_size`]. */
  cluster_size?: number | null;
  /** Test the disk's real capacity first (see [`capacity`]), and stop if it is fake. */
  check_capacity?: boolean;
};

/** File systems a drive can be formatted with. */
export type Filesystem = "fat32" | "exfat" | "hfsplus";

export type FormatApply = {
  drive: string;
  /** `identity` from the plan the user confirmed: the format is refused if the disk changed. */
  identity: string;
  /** Default: FAT32, which every player reads. */
  filesystem?: Filesystem | null;
  /** Volume label. Default: the drive's locallibrary name, else its current label. */
  label?: string | null;
  /** Bytes per cluster. Default: [`Filesystem::default_cluster_size`]. */
  cluster_size?: number | null;
  /** Test the disk's real capacity first (see [`capacity`]), and stop if it is fake. */
  check_capacity?: boolean;
};

export type AccountLoginRequest = {
  /**
   * A handle (`alice.bsky.social`), a DID, or the account's server (`https://bsky.social`).
   * Default: the handle last signed in with.
   */
  handle?: string | null;
  /**
   * The URL of another client metadata document. Default: `$LOCALLIBRARY_OAUTH_CLIENT_ID`,
   * else locallibrary's own (falling back to the development client while it isn't online).
   */
  client_id?: string | null;
  /** Sign in as the `http://localhost` development client. */
  dev_client?: boolean;
  /**
   * Keep the session in a file only you can read instead of the system keychain. Default:
   * `$LOCALLIBRARY_SESSION_STORE=file`.
   */
  session_file?: boolean | null;
  /** Scopes to ask for (default: `atproto`, which identifies you and nothing more). */
  scopes?: string[];
  /**
   * Also allow publishing: writing the My Settings, profile, set and recording records to the
   * account ([`atmosphere::PUBLISH_SCOPES`]) and uploading recordings
   * ([`atmosphere::UPLOAD_SCOPE`]), and nothing else.
   */
  publish?: boolean;
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

export type AccountLoginCancel = {
  id: string;
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

export type ProfileRequest = {
  /**
   * Drives to read. Default: the mounted rekordbox drives in the store's registry, so a drive
   * someone else plugged in doesn't count as the DJ's; every mounted rekordbox drive when the
   * registry is empty.
   */
  drives?: string[];
  /** Leave the store out (its sets, archived history lists and libraries). */
  no_store?: boolean;
  /**
   * Only these store libraries (names or ids). Default: the libraries the named drives are
   * linked to, or every library.
   */
  libraries?: string[];
  /**
   * Only sets played from this day: `2026`, `2026-09` or `2026-09-30`. Without declared sets,
   * sessions first archived from it.
   */
  since?: string | null;
  /** Only sets played up to this day (same forms). */
  until?: string | null;
  /** Without declared sets: sessions with fewer tracks don't count (default 5). */
  min_set_tracks?: number | null;
  /** Entries in ranked lists (default 10). */
  top?: number | null;
  /** Skip analysis files: no phrase mood or cue statistics, but much faster. */
  no_analysis?: boolean;
};

/** Changes to [`ProfilePrefs`], applied in field order after `reset`. */
export type PrefsUpdate = {
  /** Start again from the defaults. */
  reset?: boolean;
  /** Publish exactly these sections. */
  sections?: ProfileSection[] | null;
  include?: ProfileSection[];
  exclude?: ProfileSection[];
  /** Go back to the default sections. */
  default_sections?: boolean;
  precision?: Precision | null;
  pin?: string[];
  unpin?: string[];
  hide?: string[];
  unhide?: string[];
  hide_artist?: string[];
  unhide_artist?: string[];
  alias?: { [key: string]: string };
  unalias?: string[];
  /** The DJ's own headline (empty: back to the generated one). */
  headline?: string | null;
};

export type ProfileSection = "summary" | "sets" | "tempo" | "arc" | "mixing" | "keys" | "energy" | "genres" | "labels" | "artists" | "era" | "signature" | "prep" | "collection" | "style";

export type Precision =
  | "exact"
  /** Counts as "at least" round numbers, shares to 5%, BPM to whole numbers. */
  | "rounded";

export type SetPublishRequest = {
  /** The set: its id (or a prefix of 4 or more characters), title or day, or `last`. */
  set: string;
  /**
   * Positions (from 1) of tracks to publish without naming them. Tracks the set marks unnamed
   * and tracks the profile hides (`profile hide`) are never named either.
   */
  hide?: number[];
};

export type ImportPublishedSettings = {
  /** Whose My Settings: a handle or DID. Default: the signed-in account's. */
  from?: string | null;
  /**
   * Replace the settings profile's whole base with theirs, so settings they didn't publish
   * become unset. Default: add theirs to the base, over the values it has for them.
   */
  replace?: boolean;
};

export type SetsQuery = {
  /** Only sets at a venue whose name contains this (ignoring case). */
  venue?: string | null;
  /** Only sets of this kind (ignoring case). */
  kind?: string | null;
  /** Only sets played on or after this day (`2026`, `2026-09` or `2026-09-30`). */
  since?: string | null;
  /** Only sets played on or before this day (`2026` is the whole year). */
  until?: string | null;
};

export type SetRef = {
  /** The set: `last`, an id prefix of 4 or more, a title, or a day. */
  set?: string | null;
  /** Without `set`: the latest set at the venue whose name contains this (ignoring case). */
  venue?: string | null;
};

/** A set's details. Left out, a field stays as it is; empty (`""`), it's cleared. */
export type NewSet = {
  title?: string | null;
  /** The day it was played, `2026-10-03`. */
  date?: string | null;
  /** `club`, `festival`, `radio`, `livestream`, `mix`, or another word. */
  kind?: string | null;
  venue?: string | null;
  city?: string | null;
  /** ISO 3166 code, `GB`. */
  country?: string | null;
  /** The `at://` URI of the event's `community.lexicon.calendar.event` record. */
  event?: string | null;
  /**
   * Who they played with: names, or `did:...` for an Atmosphere account. Replaces the list;
   * empty clears it.
   */
  with?: string[] | null;
  /** Where people can listen to it (`https://...`). */
  link?: string | null;
  /** Private notes, never published. */
  notes?: string | null;
  /** Sessions to build it from, in play order (ids, or any unique prefix of 4 or more). */
  sessions?: string[];
  /**
   * A recording of the set: identified against `drive`'s library, its report kept in the
   * store. Its tracklist beats the sessions'.
   */
  recording?: string | null;
  /** Or a report kept in the store already (`mix.reports.list`). */
  report?: string | null;
  /**
   * The drive whose library a recording is matched against (default: the only mounted
   * rekordbox drive).
   */
  drive?: string | null;
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
  /** The set: `last`, an id prefix of 4 or more, a title, or a day. */
  set: string;
  title?: string | null;
  /** The day it was played, `2026-10-03`. */
  date?: string | null;
  /** `club`, `festival`, `radio`, `livestream`, `mix`, or another word. */
  kind?: string | null;
  venue?: string | null;
  city?: string | null;
  /** ISO 3166 code, `GB`. */
  country?: string | null;
  /** The `at://` URI of the event's `community.lexicon.calendar.event` record. */
  event?: string | null;
  /**
   * Who they played with: names, or `did:...` for an Atmosphere account. Replaces the list;
   * empty clears it.
   */
  with?: string[] | null;
  /** Where people can listen to it (`https://...`). */
  link?: string | null;
  /** Private notes, never published. */
  notes?: string | null;
  /** Positions to drop. */
  drop?: number[];
  moves?: Move[];
  insert?: Insert[];
  /** Sessions whose tracks to add. */
  add_sessions?: string[];
  /** A recording to attach (identified against `drive`'s library), or a kept report. */
  recording?: string | null;
  report?: string | null;
  /**
   * The drive inserted tracks are looked up on and a recording is matched against (default:
   * the only mounted rekordbox drive).
   */
  drive?: string | null;
  /** Positions to publish without naming the track. */
  unnamed?: number[];
  /** Positions to name again. */
  named?: number[];
};

export type Move = {
  from: number;
  to: number;
};

export type Insert = {
  at: number;
  /**
   * A search for one of the drive's tracks (`Artist - Title`, part of it, or a path), else
   * the track as typed: `Artist - Title`.
   */
  track: string;
};

export type SetExport = {
  set: string;
  format: SetFormat;
  /** For `m3u8`: the drive the tracks are on (default: the only mounted rekordbox drive). */
  drive?: string | null;
  /**
   * Write it to this file (otherwise it's answered as `text`). An M3U8's paths are relative
   * to it.
   */
  output?: string | null;
  /** Replace a file already at `output` (refused otherwise). */
  replace?: boolean;
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
  /** Podcasting 2.0 JSON chapters, for podcast apps: a chapter for each track with a start time. */
  | "chapters"
  /** `#,Start,Artist,Title,Album,Label,Genre,BPM,Key,ISRC,Year,Unnamed`. */
  | "csv"
  /** A playlist of the tracks on a drive. */
  | "m3u8";

export type TrackPlaysQuery = {
  /** Part of `Artist - Title` (ignoring case), or a drive path (`/Contents/...`). */
  track: string;
};

export type SetPlaylist = {
  set: string;
  drive: string;
  /** The playlist's path (default: `Sets/<date> <title or venue>`). */
  name?: string | null;
};

export type SuggestSets = {
  /**
   * Only this drive's sessions and those of its library in the store. Default: every mounted
   * rekordbox drive's and the whole store's.
   */
  drive?: string | null;
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

export type DismissSessions = {
  /** Ids (any unique prefix of 4 or more characters) or list names. */
  sessions: string[];
  /** Take the dismissal back. */
  undo?: boolean;
};

export type ImportSessionLog = {
  /** A Beat Link Trigger Playlist Writer CSV. */
  file: string;
  /**
   * The drive whose library the log's tracks are looked up in. Default: the only mounted
   * rekordbox drive (with none mounted, the log is kept as it is).
   */
  drive?: string | null;
  /** Default: the file's name. */
  name?: string | null;
};

export type IndexMixLibrary = {
  /** Default: the only mounted rekordbox drive. */
  drive?: string | null;
};

/**
 * `mix.identify`'s request ([`super::mix_identify_cached`]): the mix, and what to answer with
 * its report.
 */
export type IdentifyMixRequest = {
  /** Audio file or Audacity project (`.aup3`/`.aup4`). */
  mix: string;
  drive?: string | null;
  /** Playback speeds to search, as a fraction: 0.16 is ±16%. */
  tempo_range?: number | null;
  /**
   * Also each play's overview waveform, hot cues and phrases (`extras`), for following
   * along without asking for every track's analysis.
   */
  extras?: boolean;
  /**
   * Keep the report in the store's reports folder (`saved` is its id; see
   * [`mix_reports_list`]).
   */
  save?: boolean;
};

export type ExportMix = {
  /** Audio file or Audacity project (`.aup3`/`.aup4`). */
  mix: string;
  drive?: string | null;
  /** Playback speeds to search, as a fraction: 0.16 is ±16%. */
  tempo_range?: number | null;
  /** Also write the document to this file. */
  output?: string | null;
  /** Replace a file already at `output` (refused otherwise). */
  replace?: boolean;
};

export type ReportRef = {
  /** The report's id (`mix.reports.list`). */
  id: string;
  /**
   * Also each play's waveform, hot cues and phrases, from the drive the mix was matched
   * against when it's mounted.
   */
  extras?: boolean;
};

export type ReportHtml = {
  /** The report's id (`mix.reports.list`). */
  id: string;
  /** Also answer the page itself (otherwise just where it is). */
  inline?: boolean;
};

export type FleetRequest = {
  /** Include drives that aren't mounted. */
  all?: boolean;
  /** Include retired drives. */
  retired?: boolean;
};

export type RenameDrive = {
  drive: string;
  name: string;
  /** Also change the volume label. */
  label?: boolean;
};

export type SetRole = {
  drive: string;
  /** `primary`, `backup`, `set` (or `gig`, its old name) or `archive`; `None` clears it. */
  role?: string | null;
};

export type RetireDrive = {
  drive: string;
  undo?: boolean;
};

export type AdoptAs = {
  drive: string;
  /** The registered drive whose identity it takes. */
  known: string;
};

export type Reidentify = {
  drive: string;
  name?: string | null;
  /**
   * Give it a new identity though nothing shows it's a copy (no other drive mounted has its
   * identity, and it's on the disk recorded with it).
   */
  force?: boolean;
};

export type EjectDrives = {
  /** Drives, or a library's name for its mounted clones (but archive and retired ones). */
  drives: string[];
};

export type VerifyRequest = {
  drive: string;
  /** Compare with this snapshot (id prefix) [default: the snapshot of the drive's state]. */
  snapshot?: string | null;
  /** Other mounted drives that may hold good copies. */
  against?: string[];
};

export type RepairRequest = {
  drive: string;
  /** The problems a [`drive_verify`] found (each is checked again before anything is written). */
  problems: FileProblem[];
  against?: string[];
};

export type FileProblem = {
  path: string;
  class: FileClass;
  size: number;
  expected?: Hash | null;
  actual?: Hash | null;
  reference?: ReferenceKind | null;
  /**
   * The modification time the good copy had (restored on repair, so syncs and caches see the
   * file they knew).
   */
  mtime?: number | null;
  /** Why it couldn't be read. */
  error?: string | null;
  /** `None`: no known-good copy is available. */
  repair?: RepairSource | null;
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

/** Where a file's expected content came from. */
export type ReferenceKind =
  /** A snapshot of exactly this library state. */
  | "snapshot"
  /** The drive's hash cache. */
  | "cache"
  /** An older snapshot holding the same version of the file. */
  | "history";

/** Where a damaged or missing file can be rewritten from. */
export type RepairSource =
  | {
    from: "store";
  }
  | {
    from: "drive";
    name: string;
    root: string;
  }
  /** The copy of the store at a plugin's destination, from which it's fetched into the store. */
  | {
    from: "destination";
    plugin: string;
    destination: string;
  };

export type FsCheckRequest = {
  drive: string;
  repair?: boolean;
};

export type SetContact = {
  drive: string;
  contact: string;
  /** Also add a top-level playlist named after it (shown first on the player). */
  playlist?: boolean;
};

export type SetPlayerIdentity = {
  drive: string;
  name?: string | null;
  /** rekordbox's background colour code, 0 (none) to 8. */
  colour?: number | null;
};

export type TidyRequest = {
  drive: string;
  /** Also delete what's in `.Trashes` / `$RECYCLE.BIN` (for good). */
  empty_trash?: boolean;
};

export type CollectRecordings = {
  drive: string;
  /** Folder to copy into; a subfolder named after the drive is created. */
  to: string;
  /** Only these recordings (paths as [`list`] gives them) [default: all]. */
  only?: string[];
  tracklist?: Tracklist;
  /** Delete each recording from the drive once its copy is verified. */
  delete?: boolean;
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

export type FirmwareRequest = {
  drive: string;
  file: string;
  /** Remove other update files from the drive's root. */
  replace?: boolean;
  /** Copy it even if a check fails. */
  force?: boolean;
};

export type ProvisionRequest = {
  /** The new drive: a mounted volume, or (with `format`) a disk such as `disk4`. */
  target: string;
  /** A drive to copy the library from [default: push the library's latest snapshot]. */
  like?: string | null;
  /** The library to push (without `like`) [default: the only library]. */
  library?: string | null;
  /** locallibrary name for the new drive. */
  name?: string | null;
  /** Erase and format it first. */
  format?: boolean;
  /** Default: FAT32, which every player reads. */
  filesystem?: Filesystem | null;
  /** Volume label. Default: the drive's locallibrary name, else its current label. */
  label?: string | null;
  /** Bytes per cluster. Default: [`Filesystem::default_cluster_size`]. */
  cluster_size?: number | null;
  /** Test the disk's real capacity first (see [`capacity`]), and stop if it is fake. */
  check_capacity?: boolean;
  role?: string | null;
  /** Device Name and colour players show. */
  player_name?: string | null;
  player_colour?: number | null;
  /** Write the settings profile [default: yes]. */
  settings?: boolean;
  /** Read everything back afterwards [default: yes]. */
  verify?: boolean;
  eject?: boolean;
};

/** [`provision_apply`] with a request (rather than a plan id). */
export type ProvisionApply = {
  /** The new drive: a mounted volume, or (with `format`) a disk such as `disk4`. */
  target: string;
  /** A drive to copy the library from [default: push the library's latest snapshot]. */
  like?: string | null;
  /** The library to push (without `like`) [default: the only library]. */
  library?: string | null;
  /** locallibrary name for the new drive. */
  name?: string | null;
  /** Erase and format it first. */
  format?: boolean;
  /** Default: FAT32, which every player reads. */
  filesystem?: Filesystem | null;
  /** Volume label. Default: the drive's locallibrary name, else its current label. */
  label?: string | null;
  /** Bytes per cluster. Default: [`Filesystem::default_cluster_size`]. */
  cluster_size?: number | null;
  /** Test the disk's real capacity first (see [`capacity`]), and stop if it is fake. */
  check_capacity?: boolean;
  role?: string | null;
  /** Device Name and colour players show. */
  player_name?: string | null;
  player_colour?: number | null;
  /** Write the settings profile [default: yes]. */
  settings?: boolean;
  /** Read everything back afterwards [default: yes]. */
  verify?: boolean;
  eject?: boolean;
  /** The plan's `format.identity`, when formatting: the disk the user confirmed. */
  format_identity?: string | null;
};

/** After rekordbox exported to a drive: put back what the export replaced, then save it. */
export type RefreshRequest = {
  /** The drive rekordbox exported to [default: see [`refresh_drive`]]. */
  drive?: string | null;
  /**
   * Steps to leave out (`clean`, `replay`, `smart`, `player`, `settings`, `operations`, `pull`,
   * `sync`).
   */
  skip?: StepKind[];
  /** Then copy the drive onto the other mounted drives of its library, when it's the newest. */
  sync?: boolean;
  /**
   * Plugins' operations to run on the drive after the steps that put it right, in this order,
   * before it's saved.
   */
  operations?: OperationStep[];
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
  /** Run plugins' operations (`locallibrary PLUGIN OPERATION`). */
  | "operations"
  /** Save drives in the store (`pull`). */
  | "pull"
  /** Copy the newest library onto the other drives (`sync`). */
  | "sync"
  /** Check the drives for the players (`check`). */
  | "check"
  /** Eject the drives (`eject`). */
  | "eject";

/** A plugin's operation a job runs. */
export type OperationStep = {
  /** `<plugin>.<operation>`, as `plugins.describe` lists them. */
  operation: string;
  /**
   * As the operation's `params` schema says; `null` is none. One filled from the selection
   * that isn't given is the drive refreshed, if that's what it takes.
   */
  params?: unknown;
};

/** Getting drives ready for a set. */
export type SetJobRequest = {
  /**
   * The drives you're taking, or a library's name for its mounted clones [default: the mounted
   * drives of one library, see [`drive_group`]].
   */
  drives?: string[];
  /** The library whose mounted drives to take, when `drives` is empty. */
  library?: string | null;
  /** Player models to check against [default: the ones remembered, see `players set`]. */
  players?: string[];
  /** Also decode the end of every audio file (slow). */
  audio?: boolean;
  /** Eject the drives once everything passed [default: yes]. */
  eject?: boolean;
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

/** Which `master.db` to read, and how. */
export type CollectionDiffRequest = {
  /** Default: `$LOCALLIBRARY_MASTERDB`, else rekordbox's own (see [`masterdb::default_path`]). */
  db?: string | null;
  /**
   * SQLCipher passphrase. Default: `$LOCALLIBRARY_MASTERDB_KEY`, else the one every rekordbox
   * 6/7 installation uses.
   */
  key?: string | null;
  drive: string;
  /** Only this playlist, or the playlists in this folder (and no track-level changes). */
  playlist?: string | null;
};

/** Which `master.db` to read, and how. */
export type CollectionExportRequest = {
  /** Default: `$LOCALLIBRARY_MASTERDB`, else rekordbox's own (see [`masterdb::default_path`]). */
  db?: string | null;
  /**
   * SQLCipher passphrase. Default: `$LOCALLIBRARY_MASTERDB_KEY`, else the one every rekordbox
   * 6/7 installation uses.
   */
  key?: string | null;
  /** The drive or folder to export to. */
  drive: string;
  /**
   * Playlists or folders (paths) to export, with the tracks they use. Empty: every track and
   * playlist, or the selection the target was last exported with.
   */
  playlists?: string[];
  /** Export every track and playlist, whatever the target was exported with last. */
  all?: boolean;
  /**
   * Replace a library the target holds that wasn't exported from the collection, or the
   * files on a target with no library.
   */
  replace?: boolean;
  /** Write to an archive or retired drive (see [`crate::fleet::check_target`]). */
  overwrite_archive?: boolean;
};

export type PluginRef = {
  /** The plugin's id. */
  plugin: string;
};

export type SetPluginSetting = {
  plugin: string;
  key: string;
  /**
   * Of the setting's type (a secret's, text: kept in the system's keychain and never
   * answered; a service's switch, `services.<name>`, true or false); `null` (or left out)
   * puts it back to its default, or forgets a secret.
   */
  value?: unknown;
};

export type AccountRef = {
  plugin: string;
  /** The account's id, as the plugin's manifest has it. */
  account: string;
};

export type FinishSignIn = {
  /** What `plugins.account.sign_in.begin` answered. */
  id: string;
  /** How long to wait for the browser (default 300, an hour at most). */
  timeout_secs?: number | null;
};

export type PasteSignIn = {
  id: string;
  /** The address the browser ended up on after approving. */
  url: string;
};

export type CancelSignIn = {
  id: string;
};

/** What to write to a drive's tracks' analysis files, from what one analyzer kept. */
export type ApplyAnalysis = {
  plugin: string;
  analyzer: string;
  /** The library or drive whose tracks to write to. */
  drive: string;
  /** What to write, which the analyzer must work out: `grid`, `phrases`, `waveform`. */
  parts: AnalysisPart[];
  /** Only these tracks, by id [default: every one]. */
  tracks?: number[] | null;
  /** Also over what a track's analysis files have: without it, only where they have none. */
  overwrite?: boolean;
};

/** A part of a track's analysis an analyzer can work out. */
export type AnalysisPart =
  /** The beat grid (a `beats` field). */
  | "grid"
  /** The phrases (a `phrases` field). */
  | "phrases"
  /** The waveform (a `waveform` field). */
  | "waveform";

export type RunPluginCommand = {
  plugin: string;
  command: string;
  /** As the command's `params` schema says (`plugins.describe`). */
  params?: unknown;
  context?: PluginContext;
};

/** Where the user is, for a plugin's command or operation. */
export type PluginContext = {
  /** A library's name, or a drive's mount path, name or id prefix. An operation needs one. */
  drive?: string | null;
  /** The playlist they're on, as `playlists.tree` names it. */
  playlist?: string | null;
  /** The tracks they chose, by id. */
  tracks?: number[];
  /** The set they're on, by id (`sets.list`). */
  set?: string | null;
};

export type CopyTo = {
  /** `<plugin>:<destination>`: one of a plugin's store destinations. */
  destination: string;
  /** Copy over a copy that has snapshots the store doesn't, or that's another store's. */
  force?: boolean;
  /** Say what would be copied, and copy nothing. */
  dry_run?: boolean;
};

export type RestoreFrom = {
  destination: string;
  /** Libraries to bring back, by name or id. Empty: all of them. */
  libraries?: string[];
  /** Say what would be fetched, and fetch nothing. */
  dry_run?: boolean;
};

export type VerifyFrom = {
  /** Whose good copies the files the store and the drives lack are offered from. */
  destination: string;
  drive: string;
  /** Compare with this snapshot (id prefix) [default: the snapshot of the drive's state]. */
  snapshot?: string | null;
  /** Other mounted drives that may hold good copies. */
  against?: string[];
};

export type RepairFrom = {
  /** Whose good copies are fetched into the store first. */
  destination: string;
  drive: string;
  /** The problems a [`drive_verify`] found (each is checked again before anything is written). */
  problems: FileProblem[];
  against?: string[];
};

export type PublishSetAt = {
  /** `<plugin>:<destination>`: one of a plugin's set destinations. */
  destination: string;
  /** The set: its id (or the start of it), title or day, or `last`. */
  set: string;
  /**
   * Positions (from 1) of tracks to publish without naming them. Tracks the set marks unnamed
   * and tracks the profile hides (`profile hide`) are never named either.
   */
  hide?: number[];
};

export type PlanPluginOp = {
  plugin: string;
  operation: string;
  /** As the operation's `params` schema says (`plugins.describe`). */
  params?: unknown;
  context: PluginContext;
};

export type RenderPluginPanel = {
  plugin: string;
  /** One of the plugin's panels (`plugins.describe`). */
  panel: string;
  /** A library's name, or a drive's mount path, name or id prefix. */
  drive: string;
  /** The track, by id. */
  track: number;
};

export type PlanPluginExport = {
  plugin: string;
  /** The exporter's format. */
  exporter: string;
  /**
   * The library or drive to export, or (for a set's or a mix's exporter) whose tracks to give
   * it.
   */
  drive?: string;
  /** Only these playlists (and folders), by path; every one when empty. */
  playlists?: string[];
  /** History lists to put in, by name. */
  history?: string[];
  /** A set's exporter's set: its id, title or day, or `last` (as `sets.get` takes it). */
  set?: string | null;
  /** A mix's exporter's mix: the id of its report in the store (`mix.reports.list`). */
  mix?: string | null;
  /**
   * The file, or folder, to write (absolute), on this computer: never on a drive or in the
   * store.
   */
  output: string;
  /** As the exporter's `options` schema says (`plugins.describe`). */
  options?: unknown;
};

export type PlanPluginImport = {
  plugin: string;
  /** The importer's format. */
  importer: string;
  /** The file to read (absolute), on this computer. */
  file: string;
  /** The library or drive whose tracks it's about. */
  drive: string;
  /** As the importer's `options` schema says (`plugins.describe`). */
  options?: unknown;
};

export type AnalyzeWithPlugin = {
  plugin: string;
  analyzer: string;
  /** The library or drive whose tracks to analyse. */
  drive: string;
  /** Only these tracks, by id [default: every one]. */
  tracks?: number[] | null;
};

/** What to write to a drive's tracks, from what one analyzer kept. */
export type ApplyFields = {
  plugin: string;
  analyzer: string;
  /** The library or drive whose tracks to write to. */
  drive: string;
  /** The roles to write, which the analyzer must have: `key`, `bpm` (what a track has of them). */
  fields: FieldRole[];
  /** Only these tracks, by id [default: every one]. */
  tracks?: number[] | null;
  /** Also over a key or BPM the track has: without it, only where it has none. */
  overwrite?: boolean;
  /**
   * Leave out a value the analyzer is less sure of than this, 0 to 1 (its `key_confidence` or
   * `bpm_confidence`), and one it doesn't say how sure it is of.
   */
  min_confidence?: number | null;
};

/**
 * What a field is to locallibrary, whichever plugin works it out: the host reads a role's value
 * (doctor's tempos, keys and levels, the check before a set, ordering a set, writing keys and
 * BPMs to tracks, a published set's MusicBrainz IDs) from the analyzer chosen for it. A closed
 * list, each with one type.
 */
export type FieldRole =
  /** A Camelot code (`8A`): a string. */
  | "key"
  /** Beats per minute: a number. */
  | "bpm"
  /** How sure the analyzer is of its key, 0 to 1: a number. */
  | "key_confidence"
  /** How sure it is of its tempo, 0 to 1: a number. */
  | "bpm_confidence"
  /** Integrated loudness in LUFS: a number. */
  | "loudness"
  /** True peak in dBTP: a number. */
  | "true_peak"
  /** Runs of samples at full scale: an integer. */
  | "clipped_runs"
  /** Equal values are the same recording: a string. */
  | "identity"
  /**
   * The MusicBrainz recording's ID, a bare lowercase UUID
   * (`8dfae998-efd4-4ebe-a482-1523ff687362`): a string.
   */
  | "recording"
  /** The MusicBrainz release's ID, a bare lowercase UUID: a string. */
  | "release"
  /**
   * The MusicBrainz IDs of the artists the track credits, a bare lowercase UUID for each name
   * loclib-model's `artist_credits` splits its artist into, in that order: a list of strings.
   */
  | "artists"
  /**
   * The ISRC the MusicBrainz IDs were looked up by, as loclib-model's `isrc` spells it (null:
   * the track had none): a string. The host takes them only for a track with that ISRC.
   */
  | "isrc"
  /**
   * The names its artists' IDs go with, as loclib-model's `artist_credits` split the artist it
   * worked from: a list of strings. The host takes them only for a track credited the same.
   */
  | "credits";

/** A change to a check's settings. */
export type SetCheck = {
  plugin: string;
  /** The check's name. */
  check: string;
  /** Run it, or not; left out, as it is. */
  on?: boolean | null;
  /**
   * For a guard: keep a plan it finds a warning or a problem about from being applied, or not;
   * left out, as it is.
   */
  blocking?: boolean | null;
  /** Put both back to their defaults first. */
  reset?: boolean;
};

export type SetTimeout = {
  /**
   * How long a check has to answer, in seconds (0.001 to 3600), before it's
   * stopped (a warning says so); `null` (or left out) puts it back to its default.
   */
  timeout?: number | null;
};

/** What to find, and for which tracks. */
export type Provide = {
  kind: ProviderKind;
  /** The library or drive whose tracks to find it for. */
  drive: string;
  /** Which tracks [default: every one]. */
  tracks?: TrackSelector;
  /** Only this plugin's providers, or this provider: `<plugin>` or `<plugin>.<provider>`. */
  from?: string | null;
  /**
   * Also for tracks with what's found already, to replace it: otherwise only for tracks without
   * cover art, or without one of the tags a provider finds.
   */
  replace?: boolean;
};

/** What a provider finds for a track. */
export type ProviderKind =
  /** Cover art: an image. */
  | "artwork"
  /** Tags: a title, an artist, an album... */
  | "metadata";

/** The edits to make of what was found. */
export type PlanProvided = {
  kind: ProviderKind;
  /** The library or drive whose tracks they're for. */
  drive: string;
  /** The candidates chosen, each for its track, as they were found. */
  picks: Pick[];
  /** The images they're of, by id, as they were found: the answer's `images`, or those picked. */
  images?: { [key: string]: string };
  /**
   * Also over what the tracks have: otherwise a track with cover art keeps it, and a tag it
   * has is kept.
   */
  replace?: boolean;
};

/** A candidate chosen for a track. */
export type Pick = {
  track: number;
  candidate: Found;
};

/** A candidate a provider found for a track. */
export type Found = {
  /** Its provider, `<plugin>.<provider>`. */
  provider: string;
  /** Where it's from, as the provider says. */
  source: string;
  /** What it is, in a line. */
  label: string;
  /** How sure the provider is that it's the track's, 0 to 1. */
  score: number;
  /** The image, by its id in the answer's `images`: an artwork provider's. */
  artwork?: string | null;
  /** The tags: a metadata provider's, each it has. */
  metadata?: TrackMetadata | null;
};

/** The tags a metadata provider has for a track: each it doesn't have left out. */
export type TrackMetadata = {
  title?: string | null;
  artist?: string | null;
  album?: string | null;
  year?: number | null;
  genre?: string | null;
  /** The record label. */
  label?: string | null;
  isrc?: string | null;
};

export type SetAutoPick = {
  /**
   * A candidate alone scoring this or more (0 to 1; more than 1: never) is picked for the user;
   * `null` (or left out) puts it back to its default.
   */
  auto_pick?: number | null;
};

export type ChooseRole = {
  role: FieldRole;
  /**
   * The analyzer to read it from, `<plugin>.<analyzer>` (one with the role); `null` (or left
   * out) puts it back to its default.
   */
  analyzer?: string | null;
  /** Read it from no analyzer instead (`recording`: `sets publish` looks up no MusicBrainz IDs). */
  none?: boolean;
};

/** Building a drive from a plugin's library source. */
export type SourceExport = {
  plugin: string;
  /** One of its library sources, by name. */
  source: string;
  /** As the source's `options` schema says. */
  options?: unknown;
  /**
   * The folders the plugin may read files in, absolute: granted for this read alone, and only
   * audio files in them are copied.
   */
  grants: string[];
  /** The drive or folder to write to. */
  drive: string;
  /**
   * Playlists or folders (paths) to export, with the tracks they use. Empty: every track and
   * playlist, or the selection the target was last exported with.
   */
  playlists?: string[];
  /** Export every track and playlist, whatever the target was exported with last. */
  all?: boolean;
  /**
   * Replace a library the target holds that wasn't exported from the collection, or the
   * files on a target with no library.
   */
  replace?: boolean;
  /** Write to an archive or retired drive (see [`crate::fleet::check_target`]). */
  overwrite_archive?: boolean;
};

/** What `initialize` answers. */
export type Initialized = {
  /** Changes only when a change would break clients written for the previous one. */
  protocol: number;
  /** The sidecar's version. */
  version: string;
  /**
   * Features a client can check for: `progress`, `progress.scope`, `cancel`, `jobs.list`,
   * `shutdown.cancel`, `events`, `plan_id`, `describe`, `error.detail`, and `account` when the
   * sidecar was built with the Atmosphere account (the `account.*` and `*.publish.*` methods,
   * and `settings.import_published.*`).
   */
  capabilities: string[];
  /** Every method's name, aliases included (`rpc.describe` says more about each). */
  methods: string[];
};

/** What `version` answers. */
export type VersionInfo = {
  version: string;
  methods: string[];
};

/** Whether there was something to cancel. */
export type Cancelled = {
  cancelled: boolean;
};

/** A running request, as `jobs.list` lists it. */
export type JobInfo = {
  /** `null` for a notification (and for `watch`). */
  id: RequestId | null;
  method: string;
  started: string;
  /** Waiting for a worker. */
  queued?: boolean;
};

/** What `events.subscribe` answers: from here on, events say what changes. */
export type Subscribed = {
  kinds: string[];
  /** The drives mounted now. */
  drives: MountedDrive[];
};

/** A mounted drive, as cheaply as it can be described: no database is read. */
export type MountedDrive = {
  root: string;
  /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
  id: string | null;
  /** Its adopted name, else the volume's name. */
  name: string;
  /** Whether it holds a rekordbox export (either database). */
  has_export: boolean;
};

export type Unsubscribed = {
  /** False when there was no subscription. */
  unsubscribed: boolean;
};

export type WatchStarted = {
  watching: boolean;
};

export type WatchStopped = {
  /** False when it wasn't watching. */
  stopped: boolean;
};

/** One drive's standing relative to its library's latest snapshot. */
export type DriveStatus = {
  drive: DeviceInfo;
  library: string | null;
  /**
   * The library head (left) vs. the drive (right): `identical`, `left_newer` (the drive is
   * behind: push), `right_newer` (the drive has newer changes: pull them), `diverged`... `None`
   * if not linked to a library.
   */
  relation: Relation | null;
  head: SnapshotInfo | null;
  /**
   * Back from a set: the drive is newer than the head only by what players add (history
   * lists, TAG LIST playlists), with this many history lists new or longer. Pulling saves them.
   */
  back_from_set: number | null;
  /** Settings that differ from the profile. */
  settings_drift: number;
  error: string | null;
};

/** Quick, hash-free description of a drive, for listings and UIs. */
export type DeviceInfo = {
  root: string;
  volume_name: string;
  id: string | null;
  name: string | null;
  library_id: string | null;
  /** Either database below. */
  has_export: boolean;
  /** `export.pdb` present (the Device Library every player reads, except the newest). */
  has_device_library: boolean;
  /**
   * OneLibrary `exportLibrary.db` present (the only database CDJ-3000X, OPUS-QUAD, XDJ-AZ...
   * read).
   */
  has_library_plus: boolean;
  pdb: PdbHeader | null;
  exported_at: string | null;
  summary: LibrarySummary | null;
  settings_files: string[];
  total_space: number | null;
  free_space: number | null;
  pending: PendingOp | null;
  lineage_len: number;
  last_known: LineageEntry | null;
  /** How `export.pdb` and OneLibrary differ, on a drive that has both (empty when they agree). */
  drift: LibraryDrift | null;
  /** rekordbox's automatic playlist sync to this drive, per database. */
  rekordbox_sync: RekordboxSync[];
};

/** Header fields useful for telling exports apart. */
export type PdbHeader = {
  page_size: number;
  num_tables: number;
  /** Incremented by rekordbox on every save; higher means a later revision of the *same* file. */
  sequence: number;
};

export type LibrarySummary = {
  tracks: number;
  playlists: number;
  folders: number;
  artists: number;
  audio_bytes: number;
  total_duration_secs: number;
  tracks_without_artwork: number;
};

export type PendingOp = {
  op: LineageOp;
  started: string;
  description: string;
  /** The backup it made before changing anything: putting it back takes it back. */
  backup?: string | null;
  /**
   * The player name and colour the drive had before a sync, push or pack replaced its
   * `exportLibrary.db`, put back once it's done (see [`crate::player::stash`]).
   */
  player?: DbIdentity | null;
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
  /** rekordbox's collection written to this drive (see [`crate::collection_export`]). */
  | "export"
  /** Damaged files rewritten from a known-good copy (see [`crate::scrub`]). */
  | "repair"
  /** An operation from a newer version of locallibrary. */
  | "other";

/** The name and colour code in a OneLibrary database. */
export type DbIdentity = {
  name: string | null;
  /** rekordbox's background colour code (0: none). */
  colour: number | null;
};

export type LineageEntry = {
  fingerprint: Hash;
  at: string;
  op: LineageOp;
  snapshot?: Hash | null;
  /** Drive the content came from (sync/merge). */
  source?: string | null;
  tracks: number;
};

/**
 * Differences between a drive's two databases. rekordbox exports the same collection to both,
 * but they can drift apart (rekordbox applies some edits to only one of them), and then players
 * that read OneLibrary (CDJ-3000X, OPUS-QUAD...) show a different library from the others.
 */
export type LibraryDrift = {
  /** Tracks only `export.pdb` lists: OneLibrary players don't see them. */
  only_in_device_library: TrackRef[];
  /** Tracks only OneLibrary lists: older players don't see them. */
  only_in_onelibrary: TrackRef[];
  /** Tracks in both (same file) whose details differ. */
  changed_tracks: TrackChange[];
  /** Playlists (and folders, ending in `/`) in only one database, or with different contents. */
  playlists: string[];
  /** OneLibrary couldn't be read, so nothing could be compared. */
  error: string | null;
};

export type TrackRef = {
  id: number;
  name: string;
  file_path: string;
};

export type TrackChange = {
  track: TrackRef;
  fields: FieldChange[];
};

export type FieldChange = {
  field: string;
  old: string;
  new: string;
};

/**
 * rekordbox's automatic playlist sync to a drive (`playlists3.sync` / `playlists3Plus.sync`):
 * which playlists of its collection it keeps up to date there. Playlists locallibrary adds
 * aren't in it.
 */
export type RekordboxSync = {
  /** `device_library` (`export.pdb`) or `onelibrary`. */
  library: string;
  automatic: boolean;
  /** rekordbox collection the playlists come from (OneLibrary's `masterDbId`). */
  collection: number | null;
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

/** Snapshot without its file list, for listings. */
export type SnapshotInfo = {
  id: Hash;
  short_id: string;
  library_id: string;
  parents: Hash[];
  created: string;
  message: string | null;
  source: SnapshotSource | null;
  fingerprint: Hash;
  summary: LibrarySummary;
  files: number;
  bytes: number;
  /**
   * Its labels (see [`Store::add_label`]). Empty from [`Snapshot::info`]; the store's
   * listings fill them in.
   */
  labels?: string[];
  /**
   * Off the line back from the library's latest: saved beside it (a drive back from a set, see
   * [`crate::sync::pull_beside`]). False from [`Snapshot::info`]; [`crate::api::history`]
   * fills it in.
   */
  beside?: boolean;
};

export type SnapshotSource = {
  drive_id: string | null;
  drive_name: string;
};

/** A drive [`list_drives`] lists, with the name of the library it's a clone of. */
export type ListedDrive = {
  root: string;
  volume_name: string;
  id: string | null;
  name: string | null;
  library_id: string | null;
  /** Either database below. */
  has_export: boolean;
  /** `export.pdb` present (the Device Library every player reads, except the newest). */
  has_device_library: boolean;
  /**
   * OneLibrary `exportLibrary.db` present (the only database CDJ-3000X, OPUS-QUAD, XDJ-AZ...
   * read).
   */
  has_library_plus: boolean;
  pdb: PdbHeader | null;
  exported_at: string | null;
  summary: LibrarySummary | null;
  settings_files: string[];
  total_space: number | null;
  free_space: number | null;
  pending: PendingOp | null;
  lineage_len: number;
  last_known: LineageEntry | null;
  /** How `export.pdb` and OneLibrary differ, on a drive that has both (empty when they agree). */
  drift: LibraryDrift | null;
  /** rekordbox's automatic playlist sync to this drive, per database. */
  rekordbox_sync: RekordboxSync[];
  /** The store's name for the library `library_id` is, when the store has it. */
  library: string | null;
};

/** What adopting a drive would do: give it an identity (`.locallibrary/state.json`), or a name. */
export type AdoptPlan = {
  drive: string;
  root: string;
  /** Its identity, if it has one already. */
  id: string | null;
  /** The name it will have. */
  name: string;
};

/** What every plan says besides its own fields, as `*.plan` returns it (see [`Planned`]). */
export type PlanFields = {
  /** Names the plan in `*.apply {"plan_id": ...}`. */
  plan_id: string;
  /**
   * The RPC method's name without `.plan`, except `undo` for `drive.restore.plan`, `eject` for
   * `drive.eject.plan` and `set` for `prep.plan`; for a plugin's operation, its name.
   */
  operation: string;
  /** What will change, in a few lines, for a confirmation dialog. */
  summary: string[];
  /** Every warning, the plan's own included. */
  warnings: string[];
  /**
   * The plugin's guard (`<plugin>.<check>`) the user made blocking that found something about
   * it: applying it is refused (`blocked`).
   */
  blocked?: string | null;
  /**
   * It deletes or overwrites something (files, tracks, a library, a disk): a dialog should
   * say so.
   */
  destructive: boolean;
  /** Applying it changes nothing. */
  noop: boolean;
  /** The drives (and files on this computer) it reads or writes, its clones included. */
  drives: Touched[];
  /**
   * The library's other mounted clones, which applying copies the drive it writes onto, but
   * those `left_alone`.
   */
  clones?: CloneTarget[];
  planned_at: string;
};

/** A drive, folder or file an operation reads or writes, and its state when it was planned. */
export type Touched = {
  /** The drive's root, or the folder or file on this computer. */
  root: string;
  name: string;
  /** Whether the operation writes to it (a sync's source is only read). */
  writes: boolean;
  /**
   * A drive (archive and retired drives are refused unless the plan allows it), rather than
   * files on this computer.
   */
  drive: boolean;
};

/**
 * A mounted clone of the library a plan writes, which applying the plan copies the drive it wrote
 * onto unless it's left alone (see `Planned::reaching_clones`).
 */
export type CloneTarget = {
  name: string;
  root: string;
  /**
   * Why it isn't copied onto (it's newer, diverged, or back from a set), also among the
   * plan's warnings.
   */
  left_alone?: string | null;
};

export type LibraryPage = {
  summary: LibrarySummary;
  /** Its tracks are the page asked for (every track when unpaged). */
  library: Library;
  /** Set when paged. */
  page?: Page | null;
};

/** A rekordbox library as seen on a device export. */
export type Library = {
  tracks: Track[];
  playlists: Playlist[];
  /** Number of history playlists (players record what was played). */
  history_playlists: number;
  /**
   * The history playlists themselves (and, in OneLibrary, their folders), shaped like
   * playlists.
   */
  history: Playlist[];
};

export type Track = {
  /** Row id inside this export. Only meaningful within one export. */
  id: number;
  title: string;
  artist: string;
  album: string;
  album_artist: string;
  genre: string;
  label: string;
  key: string;
  remixer: string;
  original_artist: string;
  composer: string;
  comment: string;
  mix_name: string;
  isrc: string;
  /** Beats per minute (as analysed). */
  bpm: number;
  duration_secs: number;
  year: number;
  track_number: number;
  disc_number: number;
  /** 0-5 stars. */
  rating: number;
  color: TrackColor | null;
  play_count: number;
  bitrate: number;
  sample_rate: number;
  sample_depth: number;
  file_size: number;
  /** Absolute device path of the audio file, e.g. `/Contents/Artist/Album/Track.mp3`. */
  file_path: string;
  /** Device path of the primary analysis file (`.DAT`); `.EXT`/`.2EX` sit next to it. */
  analyze_path: string;
  date_added: string;
  release_date: string;
  artwork_id: number | null;
  /** Device path of the artwork thumbnail, if any. */
  artwork_path: string | null;
};

/** rekordbox's eight track colours. */
export type TrackColor = "pink" | "red" | "orange" | "yellow" | "green" | "aqua" | "blue" | "purple";

export type Playlist = {
  id: number;
  /** `None` for top-level nodes. */
  parent_id: number | null;
  name: string;
  is_folder: boolean;
  sort_order: number;
  /** Track ids in playlist order (empty for folders). */
  track_ids: number[];
};

export type Page = {
  offset: number;
  /** How many tracks the page holds. */
  count: number;
  /** How many tracks the library has. */
  total: number;
};

/** The playlist tree without the tracks: what a sidebar shows. */
export type PlaylistTree = {
  playlists: PlaylistNode[];
  /** The history lists players recorded (in folders, on OneLibrary). */
  history: PlaylistNode[];
};

/** A playlist or folder in [`PlaylistTree`]. */
export type PlaylistNode = {
  id: number;
  name: string;
  /** Through its folders, e.g. `Sets/2026/Warmup`. */
  path: string;
  folder: boolean;
  /** Entries in a playlist (a track can be in it twice); 0 for a folder. */
  tracks: number;
  /** The entries' total length. */
  duration_secs: number;
  /** A folder's contents, in display order. */
  children?: PlaylistNode[];
};

export type TracksById = {
  /** In the order asked for. */
  tracks: Track[];
  /** Ids the library doesn't have. */
  missing: number[];
};

export type TracksPage = {
  /** Tracks matching, before `offset` and `limit`. */
  total: number;
  total_duration_secs: number;
  tracks: Track[];
};

/**
 * [`TrackAnalysis`] with only the waveforms asked for, in the encoding asked for. With the
 * defaults it's the same as [`TrackAnalysis`].
 */
export type AnalysisView = {
  beats: Beat[];
  cues: Cue[];
  preview: WaveformData | null;
  detail: WaveformData | null;
  structure: SongStructure | null;
};

export type Cue = {
  /** `None` for memory cues; `Some(0)` = hot cue A, `Some(1)` = B, ... */
  hot_cue: number | null;
  start_ms: number;
  /** End of the loop, for loops. */
  end_ms: number | null;
  comment: string;
  color: number[] | null;
  /**
   * A memory cue's or loop's colour: a row of rekordbox's colour table (1 pink ... 8 purple,
   * as [`crate::model::TrackColor`]), 0 for none. Only the extended list has it.
   */
  color_id: number;
  /**
   * A loop's length in beats as rekordbox stored it, numerator and denominator (4/1, 1/2),
   * when it was set on the grid. Only the extended list has it.
   */
  loop_beats: LoopBeats | null;
};

export type LoopBeats = [numerator: number, denominator: number];

/** A waveform as asked for. */
export type WaveformData = Waveform | PackedWaveform;

export type Waveform = {
  style: WaveformStyle;
  /**
   * Columns per second of audio for scrolling (detail) waveforms; `None` for previews, whose
   * columns span the whole track.
   */
  columns_per_sec: number | null;
  columns: WaveColumn[];
};

export type WaveformStyle =
  /** Monochrome blue, older players (`PWAV`, `PWV3`). */
  | "blue"
  /** Colour by frequency content, nexus 2 and later (`PWV4`, `PWV5`). */
  | "rgb"
  /**
   * Lows, mids and highs drawn over one another, the CDJ-3000's (`PWV6`, `PWV7`), and what
   * analyzer plugins give ([`Bands`]).
   */
  | "three_band";

/** One column of a waveform: a back layer and a (shorter, brighter) front layer drawn over it. */
export type WaveColumn = {
  back: WaveLayer;
  front: WaveLayer;
};

/** One drawn layer of a waveform column. */
export type WaveLayer = {
  /** 0.0-1.0 of the waveform's full height. */
  height: number;
  color: number[];
};

/** A [`Waveform`] packed for sending: each layer's heights and colours as bytes in base64. */
export type PackedWaveform = {
  /** Always `base64`; a [`Waveform`] has none. */
  encoding: string;
  style: WaveformStyle;
  columns_per_sec: number | null;
  /** How many columns. */
  columns: number;
  back: PackedLayer;
  /** `None` when no column has a front layer (scrolling waveforms and blue ones). */
  front: PackedLayer | null;
};

/** One layer of a [`PackedWaveform`]. */
export type PackedLayer = {
  /** One byte a column: its height, 0-255 of the waveform's full height. */
  heights: string;
  /** Three bytes a column: its red, green and blue. */
  colors: string;
};

/**
 * Something a front end can offer to resolve an error: call a method, or send the request that
 * failed again with some parameters added (`method` absent), or again with administrator rights.
 */
export type Action = {
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
  /** Run it with administrator rights (restart the sidecar elevated first). */
  elevate?: boolean;
};

export type TracksAnalysis = {
  /** In the order asked for. */
  tracks: AnalysedTrack[];
  /** Ids the library doesn't have. */
  missing: number[];
};

/** One track's analysis in [`TracksAnalysis`]. */
export type AnalysedTrack = {
  track: number;
  analysis: AnalysisView;
};

export type PluginFields = {
  /** Tracks with fields kept for their audio file as it is now; the others are left out. */
  tracks: TrackPluginFields[];
  /** Ids asked for that the library doesn't have. */
  missing: number[];
};

/** One track's fields in [`PluginFields`]. */
export type TrackPluginFields = {
  track: number;
  /** Each analyzer's fields, by plugin id, then analyzer. */
  plugins: { [key: string]: { [key: string]: { [key: string]: FieldValue } } };
};

/**
 * A field's value, as an analyzer answers it and the host keeps it: of the type the manifest
 * declares (numbers are finite, as JSON's are), or null for one worked out with nothing to show
 * (a track left out of an answer has no value: it wasn't worked out).
 */
export type FieldValue = number | boolean | string | string[] | null;

/**
 * Everything known about one track on a drive: its details, where it's filed, problems with it
 * and a summary of its analysis.
 */
export type TrackDetails = {
  track: Track;
  /** Whether the audio file is on the drive. */
  audio_present: boolean;
  /** Playlists holding the track, by path, sorted. */
  playlists: string[];
  /** History lists (what players recorded) holding the track, by name. */
  history: string[];
  /** My Tags, as `Category/Tag`. Empty when the drive's tag database can't be read. */
  tags: string[];
  /** Other tracks on the drive that are the same recording (artist, title and duration). */
  copies: TrackRef[];
  analysis: AnalysisSummary;
};

/**
 * What a track's analysis files hold, counted rather than listed (`track.analysis` and
 * `cues.list` have the full data).
 */
export type AnalysisSummary = {
  /** Whether the drive has the track's analysis files. */
  analysed: boolean;
  beats: number;
  bars: number;
  /** The beat grid's slowest and fastest tempo. */
  tempo_range: BpmRange | null;
  hot_cues: number;
  memory_cues: number;
  /** Hot cues and memory cues that are loops. */
  loops: number;
  /** Whether there's a scrolling (detail) waveform. */
  waveform: boolean;
  mood: Mood | null;
  phrases: PhraseSummary[];
};

export type BpmRange = [low: number, high: number];

export type PhraseSummary = {
  /** `Intro`, `Up`, `Verse 2`... */
  kind: string;
  bars: number;
};

export type HarmonicMatch = {
  track: Track;
  /** Camelot code, e.g. `8A`. */
  key: string;
  /** `same key`, `±1`, `relative` or `energy boost`. */
  relation: string;
  /** 0.02 is 2% apart (half and double time allowed). */
  tempo_gap: number;
};

export type BackupInfo = {
  id: string;
  created: string;
  reason: string;
  files: string[];
  /**
   * Its place among the drive's backups, counting up (0: made by a version that didn't number
   * them).
   */
  seq: number;
  /** How many of `files` are audio (removed tracks' files, which may be their only copy). */
  audio: number;
  /**
   * When an undo took it: the backup that undo put back. Putting this one back redoes what the
   * undo took back; `undo` passes it by.
   */
  undoes?: string | null;
  /** Each of `files` as it was backed up, by content. */
  hashes?: { [key: string]: Hash };
  /**
   * What its change left in each file putting it back replaces or removes, by content (`null`:
   * absent). Missing when the change didn't finish, or an earlier version made the backup.
   */
  after?: { [key: string]: Hash | null } | null;
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

export type PlaylistDiff = {
  left: string;
  right: string;
  /** Tracks (in the collection) only on the right. */
  added_tracks: TrackRef[];
  /** Tracks only on the left. */
  removed_tracks: TrackRef[];
  /** Tracks on both sides whose details differ. */
  changed_tracks: TrackChange[];
  /** Playlists (and folders, ending in `/`) only on the right. */
  added_playlists: string[];
  /** Playlists (and folders) only on the left. */
  removed_playlists: string[];
  /** Playlists on both sides whose entries differ. */
  changed_playlists: PlaylistChanges[];
  /** Playlists on both sides with the same entries in the same order. */
  unchanged_playlists: number;
};

export type PlaylistChanges = {
  path: string;
  /** Entries only on the right, at their (1-based) position there. */
  added: DiffEntry[];
  /** Entries only on the left, at their position there. */
  removed: DiffEntry[];
  moved: MoveOutput[];
};

export type DiffEntry = {
  track: TrackRef;
  position: number;
};

export type PhraseOutput = {
  kind: PhraseKind;
  /** 1-based beat numbers into the beat grid; the phrase runs up to (not including) `end_beat`. */
  start_beat: number;
  end_beat: number;
  /** Beat where a fill-in leads into the next phrase. */
  fill_beat: number | null;
};

/** What putting a backup back would change. */
export type UndoPlan = {
  drive: string;
  root: string;
  backup: BackupInfo;
  /** The files that change. */
  files: FileChange[];
  /** How the playlists and tracks change, from the drive as it is to the drive put back. */
  diff: PlaylistDiff;
  /** History lists on the drive that the backup doesn't have: putting it back drops them. */
  history_lists: string[];
  /** Recorded edits taken off the journal: `edits replay` won't make them again. */
  journal: JournalEntry[];
  /**
   * Tracks the backup lists whose audio is neither on the drive nor in the backup: putting it
   * back is refused unless forced.
   */
  missing_audio: string[];
};

/** A file undoing changes. */
export type FileChange = {
  /** The file, as a drive key. */
  path: string;
  /**
   * `put back` (it's gone), `replaced` (it differs) or `removed` (a database the backup didn't
   * have).
   */
  change: string;
  bytes: number;
};

export type JournalEntry = {
  at: string;
  edits: EditOutput[];
  /** What the edits did, as described when they were applied. */
  changes: string[];
  /** The backup made before them, which `restore` undoes them with. */
  backup?: string | null;
};

export type MoveOutput = {
  track: TrackRef;
  /** 1-based position on the left. */
  from: number;
  /** 1-based position on the right. */
  to: number;
};

/** What can be undone: every mounted drive's backups, and art embedded in files on this computer. */
export type UndoList = {
  drives: DriveBackups[];
  /** `art unembed` puts these files back. */
  embeds: EmbedBackup[];
};

export type DriveBackups = {
  drive: string;
  root: string;
  /** Newest first. */
  backups: UndoableBackup[];
};

export type UndoableBackup = {
  id: string;
  created: string;
  reason: string;
  files: string[];
  /**
   * Its place among the drive's backups, counting up (0: made by a version that didn't number
   * them).
   */
  seq: number;
  /** How many of `files` are audio (removed tracks' files, which may be their only copy). */
  audio: number;
  /**
   * When an undo took it: the backup that undo put back. Putting this one back redoes what the
   * undo took back; `undo` passes it by.
   */
  undoes?: string | null;
  /** Each of `files` as it was backed up, by content. */
  hashes?: { [key: string]: Hash };
  /**
   * What its change left in each file putting it back replaces or removes, by content (`null`:
   * absent). Missing when the change didn't finish, or an earlier version made the backup.
   */
  after?: { [key: string]: Hash | null } | null;
  /** Recorded edits putting it back takes off the journal. */
  journal: number;
  /** It's the one `undo` puts back. */
  next: boolean;
};

/**
 * Copies of the audio files one embed rewrote, as they were, kept in the store
 * (`embed-backups/<id>/`) so [`restore_embedded`] can put them back.
 */
export type EmbedBackup = {
  id: string;
  created: string;
  /** The image that went in. */
  image: string;
  files: EmbeddedFile[];
};

export type EmbeddedFile = {
  /** The audio file. */
  path: string;
  /** Its copy as it was, in the backup's folder. */
  saved: string;
  /** Its modification time before (put back with it). */
  mtime_before: number;
  /** The file once the art went in: it's only put back while it's still that. */
  size: number;
  mtime: number;
};

export type Comparison = {
  relation: Relation;
  confidence: Confidence;
  suggestion: Suggestion;
  left: SideSummary;
  right: SideSummary;
  evidence: Evidence[];
  /** Shared tracks / tracks in the smaller library. */
  overlap: number;
  /** From the older side to the newer one (left -> right when undetermined). */
  diff: LibraryDiff;
};

export type Confidence = "low" | "medium" | "high";

/** What to do about it. */
export type Suggestion =
  | ("nothing" | "update_right" | "update_left" | "merge")
  /** Diverged: pick a side explicitly, or merge. */
  | "choose_or_merge";

export type SideSummary = {
  label: string;
  id: string | null;
  library_id: string | null;
  fingerprint: Hash | null;
  tracks: number;
  playlists: number;
  sequence: number | null;
  exported_at: string | null;
  changed_since_last_sync: boolean;
  interrupted_operation: boolean;
  last_synced: string | null;
};

export type Evidence = {
  signal: string;
  favors: Favors;
  detail: string;
};

export type Favors = "left" | "right" | "neither";

/**
 * Differences between two exports *of the same library*. Tracks are matched by file path,
 * which rekordbox keeps stable across exports; playlists by their place in the folder tree.
 */
export type LibraryDiff = {
  added_tracks: TrackRef[];
  removed_tracks: TrackRef[];
  changed_tracks: TrackChange[];
  added_playlists: string[];
  removed_playlists: string[];
  /** Playlists whose contents or order differ. */
  changed_playlists: string[];
};

export type SyncPreview = {
  comparison: Comparison;
  /** `None` when nothing needs to happen. */
  source: string | null;
  target: string | null;
  plan: SyncPlan | null;
  /**
   * The drives diverged, but the target's changes are all player-made (it came back from a
   * set): it is saved in the store before being overwritten (see [`crate::after_set`]).
   */
  after_set?: AfterSet | null;
};

export type SyncPlan = {
  /** Source drive name, or `snapshot <id>`. */
  source: string;
  target_name: string;
  target: string;
  copies: PlannedCopy[];
  deletes: PlannedDelete[];
  unchanged: number;
  copy_bytes: number;
  delete_bytes: number;
  copies_by_class: ClassCounts;
  deletes_by_class: ClassCounts;
  free_space: number | null;
  /** Allocation unit of the target filesystem, used to estimate space. */
  cluster_size: number;
  /** Whether the copies fit in the free space (deletes happen last). */
  fits: boolean;
  warnings: string[];
};

export type PlannedCopy = {
  path: string;
  size: number;
  mtime: number;
  class: FileClass;
  reason: CopyReason;
  hash?: Hash | null;
  /** Size of the file being replaced. */
  target_size?: number | null;
};

export type CopyReason = "new" | "changed";

export type PlannedDelete = {
  path: string;
  size: number;
  class: FileClass;
};

export type ClassCounts = {
  audio: number;
  analysis: number;
  artwork: number;
  database: number;
  other: number;
};

/** Which way a sync between two diverged drives can go. */
export type AfterSet = {
  /** The last state both held. */
  common: SnapshotInfo;
  /** The drive whose changes are all player-made: it is overwritten, after being saved. */
  back_from_set: DriveChanges;
  /** The other drive (a new export, an edit...). */
  newer: DriveChanges;
  /** Whether `left` (as given) is the drive back from the set. */
  left_is_back_from_set: boolean;
};

/** What changed on one drive since the last state both drives held. */
export type DriveChanges = {
  drive: string;
  /** History lists added (players add one per session). */
  history_lists: string[];
  /**
   * Analysis files rewritten for tracks that were already there (memory cues saved on a
   * player).
   */
  cue_files: number;
  /** Playlists a CDJ-3000X makes from its Tag List. */
  tag_lists: string[];
  /** Changes players don't make: new or removed tracks, edited playlists, new audio... */
  other: string[];
};

export type SyncResult = {
  source: string | null;
  target: string | null;
  report: SyncReport | null;
  /**
   * Snapshots taken along the way (after a set: the state the drive came back with, then the
   * result).
   */
  saved?: SnapshotInfo[];
};

export type SyncReport = {
  copied: number;
  copied_bytes: number;
  deleted: number;
  deleted_bytes: number;
  unchanged: number;
  fingerprint: Hash | null;
};

export type SyncManyPreview = {
  source_name: string;
  source: string;
  /** One entry per other drive; `plan` is `None` for drives already identical to the source. */
  targets: SyncPreview[];
};

/** One target of a [`sync_many_apply`]. */
export type TargetResult = {
  target: string;
  name: string;
  report: SyncReport | null;
  /** Why this target failed (the others carry on). */
  error: TargetError | null;
};

export type TargetError = {
  kind: string;
  message: string;
};

/** What saving a drive in the store (`pull`) would do. */
export type PullPlan = {
  drive: string;
  root: string;
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
  /** The library's latest snapshot. */
  head: SnapshotInfo | null;
};

export type PullReport = {
  library: string;
  library_id: string;
  snapshot: SnapshotInfo | null;
  up_to_date: boolean;
  imported_files: number;
  imported_bytes: number;
  reused_files: number;
};

export type PushPreview = {
  snapshot: SnapshotInfo;
  plan: SyncPlan;
};

export type MergePlan = {
  source: string;
  target: string;
  new_tracks: PlannedTrack[];
  /** Source tracks already in the target. */
  matched_tracks: number;
  playlists: PlannedPlaylist[];
  artwork_files: number;
  copy_bytes: number;
  /** Estimated space needed on the target (cluster-rounded, including the new database). */
  needed_bytes: number;
  free_space: number | null;
  fits: boolean;
  warnings: string[];
  /** History playlists to bring across (with [`MergeOptions::history`]). */
  history: string[];
  /** New tracks that bring My Tags along. */
  tagged_tracks: number;
  /** My Tag categories and tags the target gets for them. */
  new_tags: Created[];
};

export type PlannedTrack = {
  source_id: number;
  name: string;
  /** Target key of the audio file. */
  audio: string;
  renamed: boolean;
  size: number;
};

export type PlannedPlaylist = {
  /** Target path of the playlist. */
  path: string;
  create: boolean;
  tracks_added: number;
};

/** A category or tag an edit creates. */
export type Created = {
  /** `Category` or `Category/Tag`. */
  path: string;
  database: Source;
};

/** One of the two databases that hold My Tags. */
export type Source =
  /** `exportExt.pdb`, for players that read `export.pdb`. */
  | "export_ext"
  /** OneLibrary's `exportLibrary.db`. */
  | "one_library";

export type MergeReport = {
  tracks_added: number;
  tracks_matched: number;
  playlists_created: number;
  playlist_entries_added: number;
  artwork_added: number;
  history_playlists_added: number;
  hot_cue_bank_lists_added: number;
  /** New tracks given My Tags (in either database). */
  tagged_tracks: number;
  copied_bytes: number;
  backup: string | null;
  fingerprint: Hash | null;
};

export type ConvertPlan = {
  drive: string;
  root: string;
  to: Target;
  /**
   * The drive already has that database, and it's rebuilt, dropping what only it has (the
   * warnings name it).
   */
  replaces: boolean;
  /**
   * The drive already has that database, and it keeps what it has. The counts below are then
   * what it gains: tracks it lacks, playlists it lacks or that gain entries, folders made for
   * them, and history lists it lacks.
   */
  adds: boolean;
  tracks: number;
  playlists: number;
  folders: number;
  history_playlists: number;
  /** Artwork images copied under the other database's file names. */
  artwork: number;
  /**
   * Tracks whose My Tags come along from the other database's tags (that gain some, when
   * adding).
   */
  tagged_tracks: number;
  /** Space needed: the new database, copied artwork, and the backup of the database files. */
  needed_bytes: number;
  free_space: number | null;
  fits: boolean;
  warnings: string[];
};

/** What a conversion made (added, when the drive had the database: see [`ConvertPlan::adds`]). */
export type ConvertReport = {
  tracks: number;
  playlists: number;
  history_playlists: number;
  artwork: number;
  /** Tracks with My Tags in the converted database's tags (that gained some, when adding). */
  tagged_tracks: number;
  /** OneLibrary cue rows made from analysis files. */
  cues: number;
  backup: string | null;
  fingerprint: Hash | null;
};

export type StoreInfo = {
  root: string;
  stats: StoreStats;
  /** Size of the copies `art embed` keeps of the files it rewrote, for `art unembed`. */
  embed_backup_bytes: number;
  libraries: LibraryRecord[];
  /** When this store is a copy of another (`store copy`): when it was made. */
  copy_of?: CopyRecord | null;
};

export type StoreStats = {
  libraries: number;
  snapshots: number;
  objects: number;
  object_bytes: number;
  drives: number;
};

export type LibraryRecord = {
  id: string;
  name: string;
  head: Hash | null;
  created: string;
  updated: string;
};

/** A copy of a store: kept in the copy as `copy.json`, and in the store's `copies.json`. */
export type CopyRecord = {
  path: string;
  /** The store copied: its id, and where it was. */
  store: string;
  from: string;
  /** When the latest copy started. */
  started: string;
  /** When the latest copy finished; `None` while one runs, or after one was stopped. */
  finished: string | null;
  /** When a copy last finished with every snapshot: the copy holds the store as it was then. */
  last_complete: string | null;
  snapshots: number;
  objects: number;
  bytes: number;
  problems?: string[];
  /**
   * A hash of what the copy held besides stored files (snapshots, libraries, labels, the
   * store's other records) when the latest copy finished: one that changed since was written
   * to.
   */
  contents?: Hash | null;
};

export type GcReport = {
  removed_objects: number;
  freed_bytes: number;
  removed_snapshots: number;
  dry_run: boolean;
};

export type VerifyReport = {
  checked: number;
  corrupt: string[];
  missing: string[];
};

export type PruneReport = {
  libraries: LibraryPrune[];
  removed_snapshots: number;
  /**
   * Stored files removed: no snapshot left uses them. With `dry_run`, the ones that would
   * be.
   */
  removed_objects: number;
  freed_bytes: number;
  dry_run: boolean;
  warnings: string[];
};

export type LibraryPrune = {
  library: string;
  /** Newest first. */
  kept: KeptSnapshot[];
  /** Newest first. */
  removed: SnapshotInfo[];
};

export type KeptSnapshot = {
  snapshot: SnapshotInfo;
  reasons: KeepReason[];
  /** With `drive`: the drives last seen holding it. */
  drives?: string[];
  /** With `history`: the history lists no newer kept snapshot holds. */
  history?: string[];
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

export type CopyReport = {
  /** The folder copied to, or `<plugin>:<destination>` for a plugin's destination. */
  path: string;
  /** Snapshots the copy holds now. */
  snapshots: number;
  snapshots_copied: number;
  objects_copied: number;
  bytes_copied: number;
  /** Stored files the copy had already. */
  objects_present: number;
  /** With `verify`: files the copy had that had changed, copied again. */
  objects_repaired: number;
  /** With `mirror`: what was removed from the copy. */
  removed_snapshots: number;
  removed_objects: number;
  /** Snapshots left out because their files are damaged or missing in the store. */
  problems: string[];
  /** Whether the copy now holds every snapshot. */
  complete: boolean;
  dry_run: boolean;
};

/** A copy the store made, and how it stands now. */
export type CopyStatus = {
  path: string;
  /** The store copied: its id, and where it was. */
  store: string;
  from: string;
  /** When the latest copy started. */
  started: string;
  /** When the latest copy finished; `None` while one runs, or after one was stopped. */
  finished: string | null;
  /** When a copy last finished with every snapshot: the copy holds the store as it was then. */
  last_complete: string | null;
  snapshots: number;
  objects: number;
  bytes: number;
  problems?: string[];
  /**
   * A hash of what the copy held besides stored files (snapshots, libraries, labels, the
   * store's other records) when the latest copy finished: one that changed since was written
   * to.
   */
  contents?: Hash | null;
  /** Whether the copy is there now (its disk mounted, its share reachable). */
  available: boolean;
  /** Whether it holds every library's latest snapshot, when available. */
  up_to_date: boolean | null;
};

/**
 * A name given to a snapshot (`pull --label "before NYE"`). Labels are unique in a store (any
 * case), stand in for the snapshot's id wherever one is asked for, and keep the snapshot from
 * being pruned or collected.
 */
export type SnapshotLabel = {
  name: string;
  snapshot: Hash;
  created: string;
};

/** A library in the store, for listings. */
export type LibraryListing = {
  id: string;
  name: string;
  created: string;
  updated: string;
  /** Its snapshots, back from the latest through first parents. */
  snapshots: number;
  /** The latest snapshot. */
  head: SnapshotInfo | null;
  /** Drives linked to it, by name. */
  drives: string[];
};

export type EditPlan = {
  drive: string;
  root: string;
  /** One line per edit. */
  changes: string[];
  playlists_created: string[];
  playlists_deleted: string[];
  playlists_changed: PlaylistChange[];
  tracks_changed: number;
  tracks_removed: TrackRef[];
  history_removed: number;
  /** Files to delete (removed tracks' audio, analysis and artwork), as drive keys. */
  files: string[];
  bytes: number;
  /** `export.pdb` and/or `exportLibrary.db`. */
  databases: string[];
  needed_bytes: number;
  free_space: number | null;
  fits: boolean;
  warnings: string[];
  /**
   * The edits as the drive's journal records them (tracks named by file), which applying it
   * records there, unless it was planned with `no_journal`.
   */
  recorded: EditOutput[];
  /**
   * OneLibrary, beside `export.pdb`, changes. The counts above are `export.pdb`'s, so they
   * leave out edits only OneLibrary lacks, as after rekordbox exported over it alone.
   */
  onelibrary_changes: boolean;
};

/** A playlist whose contents change. */
export type PlaylistChange = {
  path: string;
  before: number;
  after: number;
  added: number;
  removed: number;
  /** Same tracks, different order. */
  reordered: boolean;
};

export type EditReport = {
  changes: number;
  databases: string[];
  files_deleted: number;
  freed_bytes: number;
  backup: string | null;
  fingerprint: Hash | null;
  warnings: string[];
};

/** The recorded edits clearing the journal would forget. */
export type ClearEditsPlan = {
  drive: string;
  root: string;
  entries: JournalEntry[];
};

export type ClearedEdits = {
  removed: number;
};

export type SmartPlaylists = {
  playlists: SmartPlaylist[];
};

export type SmartPlaylist = {
  /** Where the playlist goes on a drive, e.g. `Smart/Peak time`. */
  path: string;
  /** See [`crate::query`]. */
  filter: string;
  sort?: string | null;
  limit?: number | null;
  /**
   * The libraries it's limited to, by a library's id or one of its drives' (any clone of that
   * drive's library takes it, as clones are kept identical); every drive when empty.
   */
  drives?: string[];
};

export type RelocatePlan = {
  drive: string;
  root: string;
  /** Tracks pointed at their file where it is on the drive. */
  repointed: Relocation[];
  /** Tracks whose file is copied to the path they have. */
  copied: Relocation[];
  ambiguous: Ambiguous[];
  /** Tracks whose files of their name are none of them as long as the track: none picked. */
  unfit: Ambiguous[];
  /** Missing tracks with no file of their name. */
  not_found: TrackRef[];
  /** Audio copied onto the drive. */
  copy_bytes: number;
  /** Space needed for that, the backup and the new databases. */
  needed_bytes: number;
  free_space: number | null;
  fits: boolean;
  warnings: string[];
};

export type Relocation = {
  track: TrackRef;
  file: RelocateCandidate;
};

/** A file a missing track could be. */
export type RelocateCandidate = {
  /** A key on the drive, or a path on this computer. */
  path: string;
  on_drive: boolean;
  size: number;
  /** Seconds, when its headers say. */
  duration_secs: number | null;
};

/** A track and every file of its name found for it, none picked. */
export type Ambiguous = {
  track: TrackRef;
  candidates: RelocateCandidate[];
};

export type UpgradePlan = {
  drive: string;
  root: string;
  upgrades: Upgrade[];
  not_upgraded: NotUpgraded[];
  /** Lossy tracks with no lossless file of their recording in the folders. */
  unmatched: number;
  /** Lossy files deleted (with `remove_old`), as keys. */
  removed: string[];
  copy_bytes: number;
  /** Space for the copies, the backup and the new databases. */
  needed_bytes: number;
  free_space: number | null;
  fits: boolean;
  warnings: string[];
};

/** A track pointed at a lossless file of its recording. */
export type Upgrade = {
  track: TrackRef;
  /** The lossless file found, on this computer. */
  file: string;
  /** Where it goes on the drive (a key): beside the track's file. */
  to: string;
  size: number;
  /** How much later its cues and beat grid fall in the new file (ms; earlier when negative). */
  shift_ms: number;
  /** The file is on the drive already (left by an upgrade since undone): it isn't copied. */
  on_drive: boolean;
};

/** A track with a lossless file of its recording that isn't upgraded, and why. */
export type NotUpgraded = {
  track: TrackRef;
  file: string;
  why: string;
};

export type PackPlan = {
  source: string;
  target: string;
  target_root: string;
  playlists: string[];
  tracks: number;
  /** Size of the packed library's files. */
  library_bytes: number;
  copies: FileCopy[];
  copy_bytes: number;
  /** Files on the target the packed library doesn't use. */
  deletes: FileCopy[];
  delete_bytes: number;
  unchanged: number;
  /** The target holds a library that isn't an earlier pack of this source. */
  replaces_library: boolean;
  /** What the target holds that only it has: saved in the store before packing. */
  unsaved: Unsaved | null;
  /** Tracks the players can't play, packed as copies they can. */
  transcodes: PackTranscode[];
  /** About how much the copies to make take (not those current already). */
  transcode_bytes: number;
  /** The copies' players and how they're made, as the target will remember them. */
  transcode: TranscodeOutput | null;
  /** Tracks none of the players play that the pack leaves as they are. */
  unplayable: PackUnplayable[];
  /**
   * Space the pack needs on the target: the files it copies, the copies it makes, its new
   * databases and the backup of what it replaces.
   */
  needed_bytes: number;
  free_space: number | null;
  fits: boolean;
  warnings: string[];
};

/** A file a write copies or removes. */
export type FileCopy = {
  path: string;
  size: number;
};

/**
 * What the target holds that packing would replace and that nothing else keeps: it is saved in
 * the store first, as `sync` saves a drive back from a set before overwriting it.
 */
export type Unsaved = {
  /** History lists players recorded that the packed library won't have. */
  history_lists: string[];
  /**
   * The drive changed since it was last written this way (packed from this source, or exported
   * from the collection): memory cues, TAG LIST playlists or history lists players saved, or
   * edits.
   */
  changed: boolean;
  /**
   * Its OneLibrary database can't be read, so history lists players that read only OneLibrary
   * recorded there can't be told apart: it's kept whole.
   */
  unreadable_onelibrary: boolean;
};

/** A track packed as a copy its players play. */
export type PackTranscode = {
  track: TrackRef;
  /** The source's audio file and the copy's, as keys. */
  from: string;
  to: string;
  /** The players that don't play the source, and why. */
  why: string;
  /** What the copy is (`aiff-48000-24-tpdf-v1`): container, rate, depth, dither, version. */
  recipe: string;
  /** The copy's size: estimated, or what it is when `current`. */
  size: number;
  /** A copy of the same audio by the same recipe is on the target already, and stays. */
  current: boolean;
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
    op: "create_playlist";
    path: string;
    folder: boolean;
  }
  | {
    op: "rename_playlist";
    playlist: string;
    name: string;
  }
  /**
   * Move a playlist or folder into folder `into` (the top level when absent or empty), at
   * `position` among its new siblings (the end when absent).
   */
  | {
    op: "move_playlist";
    playlist: string;
    into: string | null;
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
    op: "add_tracks";
    playlist: string;
    tracks: TrackSelector;
    position: number | null;
    /** Order the added tracks (see [`query::parse_sort`]). */
    sort: string | null;
    allow_repeats: boolean;
  }
  /** Remove every entry of the selected tracks and/or the entries at `positions`. */
  | {
    op: "remove_tracks";
    playlist: string;
    tracks: TrackSelector | null;
    positions: number[];
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
    op: "set_tracks";
    playlist: string;
    tracks: TrackSelector;
    sort: string | null;
    limit: number | null;
    create: boolean;
  }
  /**
   * Make a playlist's entries exactly these audio files (device paths) in this order, repeats
   * and all; none empties it. With `create`, the playlist at exactly that path, made (with
   * missing folders above it) if missing. Imported tracklists are made this way. The journal
   * records these edits this way: filling a playlist (`SetTracks`), ordering it for mixing
   * (`PlanSet`), and removing or moving entries whose files have other entries in it too.
   */
  | {
    op: "set_entries";
    playlist: string;
    paths: string[];
    create?: boolean;
  }
  /** Reorder a playlist (see [`query::parse_sort`]). */
  | {
    op: "sort_playlist";
    playlist: string;
    by: string;
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
   * Order a playlist for mixing (see [`harmony::order_set`], with how sure each key is from
   * the keys an analyzer heard already, see [`crate::sound::key_confidences`]): in place, or
   * into a new playlist at `into`.
   */
  | {
    op: "plan_set";
    playlist: string;
    start: TrackSelector | null;
    into: string | null;
  }
  | {
    op: "edit_tracks";
    tracks: TrackSelector;
    fields: TrackFields;
  }
  /**
   * Delete tracks with their playlist and history entries, and (unless `keep_files`) their
   * audio, analysis and artwork files when no other track uses them. A track goes with every
   * row of its audio file, in each database; a path may name one only the drive's other
   * database lists (OneLibrary, drifted from `export.pdb`), which is deleted there.
   */
  | {
    op: "delete_tracks";
    tracks: TrackSelector;
    keep_files: boolean;
  }
  /**
   * Replace one copy of a recording with another: playlist and history entries (and My Tags)
   * move to `keep`, then `duplicate` is deleted unless `keep_duplicate`.
   */
  | {
    op: "replace_track";
    duplicate: TrackSelector;
    keep: TrackSelector;
    keep_duplicate: boolean;
  }
  /**
   * Point one track at another audio file on the drive (a device path), as when its file was
   * moved or renamed: in both databases and in the analysis files that record it. With `file`,
   * the file is another encoding of the recording (a lossless copy of an MP3): the rows take
   * its format, size, sample rate, bit depth and bit rate, and with `shift_ms` its cues and
   * beat grid move by that much, for audio that starts at another point.
   */
  | {
    op: "set_path";
    track: TrackSelector;
    path: string;
    file?: AudioFile | null;
    shift_ms?: number;
  }
  /** Delete history lists (what players recorded) by name, or all of them. */
  | {
    op: "delete_history";
    names: string[];
    all: boolean;
  }
  /**
   * Delete history lists, each the one with exactly its name and entries. The journal records
   * deleting history lists this way, so a replay leaves lists players recorded since, under a
   * reused name too.
   */
  | {
    op: "delete_history_lists";
    lists: HistoryListName[];
  }
  /**
   * A My Tag edit (see [`crate::tags`]), its tracks named by file. `tags` edits are recorded
   * this way too.
   */
  | (TagChange & {
    op: "my_tag";
  })
  /**
   * A cue edit (see [`crate::cue_edit`]) on the tracks of these audio files (device paths).
   * `cues` edits are recorded this way too.
   */
  | {
    op: "cues";
    tracks: string[];
    change: CueChange;
  }
  /**
   * Give tracks this artwork, scaled to the sizes players show (see [`crate::artwork`]). The
   * journal records the tracks by file and the image as staged on the drive.
   */
  | {
    op: "set_artwork";
    tracks: TrackSelector;
    image: ImageSource;
  }
  /**
   * Write a beat grid, phrases or a waveform into the analysis files of the track of this
   * audio file (a device path, see [`crate::analysis_edit`]). Planned, a grid brings the
   * track's BPM with it (an `edit_tracks`); the journal records a waveform as staged on the
   * drive.
   */
  | {
    op: "set_analysis";
    track: string;
    /** The beat grid: each beat's number in its bar (1-4), the tempo there and its time. */
    beats?: Beat[] | null;
    /** The phrases, named as the track's mood names them. */
    phrases?: SongStructure | null;
    waveform?: WaveSource | null;
  };

/** A track none of the players play that packing leaves as it is. */
export type PackUnplayable = {
  track: TrackRef;
  why: string;
};

export type PackReport = {
  copied: number;
  copied_bytes: number;
  /** Copies made for the players (see [`PackPlan::transcodes`]). */
  transcoded: number;
  deleted: number;
  deleted_bytes: number;
  fingerprint: Hash | null;
  /** The target's state, saved in the store before packing (see [`PackPlan::unsaved`]). */
  saved: SnapshotInfo | null;
  /**
   * The backup of what the pack replaced: `locallibrary undo` puts its databases, analysis and
   * artwork back. Tracks the pack removed have no audio in it (their audio is on the source),
   * so taking back a pack that removed tracks needs `--force` (and leaves them without audio)
   * or packing again with the old selection.
   */
  backup: string | null;
};

/** A drive's My Tags, from both databases. */
export type MyTags = {
  drive: string;
  /** The databases read. */
  sources: Source[];
  /** In display order: the first source's order, then anything only the other one has. */
  categories: MyTagCategory[];
  /** Where `exportExt.pdb` and `exportLibrary.db` disagree. */
  differences: string[];
  warnings: string[];
};

export type MyTagCategory = {
  name: string;
  /** The databases that have it. */
  sources: Source[];
  tags: TagInfo[];
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
  drive: string;
  root: string;
  /** `add`, `remove`, `rename` or `delete`. */
  action: string;
  /** The category or tag edited (as the drive spells it, if it has it). */
  tag: string;
  /** The new name, for a rename. */
  to: string | null;
  created: Created[];
  /** Tracks that gain (`add`) or lose (`remove`, `delete`) the tag. */
  tracks: TrackRef[];
  /** Selected tracks that already have (`add`) or don't have (`remove`) the tag. */
  unchanged: number;
  /** The databases the edit changes. */
  databases: Source[];
  warnings: string[];
};

export type TagReport = {
  /** The databases written. */
  databases: Source[];
  tracks_changed: number;
  backup: string | null;
  fingerprint: Hash | null;
};

/** A drive track's cues. */
export type TrackCues = {
  track: TrackRef;
  bpm: number;
  cues: CueInfo[];
  /** How many cue rows OneLibrary (`exportLibrary.db`) has for the track, if it lists it. */
  onelibrary_cues: number | null;
  warnings: string[];
};

/** A cue as shown to people. */
export type CueInfo = {
  /** `A`-`H`; `None` for memory cues. */
  hot_cue: string | null;
  start_ms: number;
  /** Where the loop ends, for loops. */
  end_ms: number | null;
  /** The beat of the grid the cue sits on (counting from 1), if it's on one. */
  beat: number | null;
  /** A quantised loop's length in beats: `"8"`, `"1/2"`. */
  loop_beats: string | null;
  name: string;
  /** `#rrggbb` for a hot cue with a palette colour, a colour name (`red`) for a memory cue. */
  color: string | null;
  active_loop: boolean;
};

export type CuePlan = {
  drive: string;
  root: string;
  /** Tracks whose cues change. */
  tracks: TrackCueChange[];
  /** Space needed for the backup and the rewritten files. */
  needed_bytes: number;
  free_space: number | null;
  fits: boolean;
  warnings: string[];
};

export type TrackCueChange = {
  track: TrackRef;
  before: CueInfo[];
  after: CueInfo[];
  /** Analysis files that change, as drive keys. */
  files: string[];
  /** Whether the track's rows in `exportLibrary.db` change too. */
  onelibrary: boolean;
};

export type CueReport = {
  tracks_changed: number;
  files_written: string[];
  onelibrary: CueRowChanges | null;
  backup: string | null;
  fingerprint: Hash | null;
};

/** What [`OneLibrary::replace_cues`] did. */
export type CueRowChanges = {
  updated: number;
  added: number;
  removed: number;
  /** Hot cue bank list entries that pointed at removed cues. */
  bank_list_entries_removed: number;
};

export type PlaylistRestorePreview = {
  source: RestoreSource;
  /** The playlist or folder's path, as the source has it (and as it will be on the drive). */
  path: string;
  folder: boolean;
  /** Playlists restored: one, or every playlist in the folder. */
  playlists: number;
  /** Entries put back. */
  entries: number;
  /** Entries left out because their track isn't on the drive any more. */
  missing: MissingEntry[];
  /** Playlists in the folder on the drive that the source doesn't have: left as they are. */
  untouched: string[];
  plan: EditPlan;
};

/** Where the restored playlist comes from. */
export type RestoreSource = {
  kind: "snapshot";
  snapshot: SnapshotInfo;
  library: string | null;
} | {
  kind: "backup";
  backup: BackupInfo;
};

/** An entry the source has whose track isn't on the drive any more. */
export type MissingEntry = {
  /** The playlist's path. */
  playlist: string;
  /** Its position in the source playlist, from 1. */
  position: number;
  /** `Artist - Title`. */
  track: string;
  /** The audio file, as the database lists it. */
  file: string;
};

export type SettingDef = {
  key: string;
  label: string;
  category: SettingCategory;
  file: SettingsFile;
  choices: Choice[];
  default: string;
};

export type SettingCategory = "player" | "display" | "mixer";

export type SettingsFile = "my_setting" | "my_setting2" | "djm_my_setting" | "dev_setting";

export type Choice = {
  id: string;
  label: string;
};

export type SettingsProfile = {
  /** Applied to every drive. */
  base: { [key: string]: string };
  /** Per-drive deviations, keyed by drive id. */
  drives: { [key: string]: DriveOverrides };
};

export type DriveOverrides = {
  /** Drive name when the override was last edited (for display only). */
  name: string;
  values: { [key: string]: string };
};

export type DriveSettingsView = {
  drive: string;
  id: string | null;
  current: SettingsSnapshot;
  drift: Drift[];
};

export type SettingsSnapshot = {
  values: { [key: string]: SettingValue };
  missing_files: SettingsFile[];
  problems: string[];
  /**
   * Bytes outside every known setting that differ from what rekordbox 6 writes: settings newer
   * rekordbox versions added (rekordbox 7.2 writes six). Preserved by `apply`, but not managed
   * by profiles until their meaning is known.
   */
  undecoded: UndecodedByte[];
};

export type SettingValue =
  | string
  /** A byte value not in the schema (newer firmware?). Preserved untouched. */
  | number;

/** A data byte of a settings file that no known setting covers. */
export type UndecodedByte = {
  file: SettingsFile;
  /** Offset in the file's data section (after the 104-byte header). */
  offset: number;
  value: number;
  /** What rekordbox 6 writes there. */
  expected: number;
};

/** How a drive's current settings deviate from what the profile wants. */
export type Drift = {
  key: string;
  expected: string;
  actual: SettingValue | null;
  /** Whether the expectation comes from a per-drive override. */
  overridden: boolean;
};

/** What writing the profile would change on each drive. */
export type SettingsApplyPlan = {
  targets: DriveSettingsPlan[];
};

export type DriveSettingsPlan = {
  drive: string;
  root: string;
  changes: ApplyReport;
};

export type ApplyReport = {
  changes: SettingChange[];
  files_written: SettingsFile[];
  files_created: SettingsFile[];
  dry_run: boolean;
};

export type SettingChange = {
  key: string;
  file: SettingsFile;
  /** `None` when the file did not exist yet. */
  from: SettingValue | null;
  to: string;
};

/** One drive's part of a settings write: its report, or why it failed (the others carry on). */
export type DriveSettingsResult = {
  drive: string;
  root: string;
  report: ApplyReport | null;
  error: TargetError | null;
};

/**
 * What importing a drive's settings, or rekordbox's, would make of the profile's base. (For
 * the My Settings an account published, see [`SettingsImportPlan`].)
 */
export type ImportSettingsPlan = {
  /** The drive, or `rekordbox on this computer`. */
  from: string;
  /** The values read. */
  values: number;
  changes: BaseChange[];
};

/** One setting in the settings profile's base, before and after importing. */
export type BaseChange = {
  key: string;
  /** `None`: not set. */
  from: string | null;
  to: string | null;
};

/** One answer for `One`, a list in the order asked for `Many`. */
export type OneOrMany_for_ArtworkImage = (ArtworkImage | null) | TrackArtwork_for_ArtworkImage[];

export type ArtworkImage = {
  mime: string;
  base64: string;
};

/** A track's artwork in a batch: `image` is `None` when it has none. */
export type TrackArtwork_for_ArtworkImage = {
  track: number;
  image: ArtworkImage | null;
};

/** One answer for `One`, a list in the order asked for `Many`. */
export type OneOrMany_for_ArtworkFile = (ArtworkFile | null) | TrackArtwork_for_ArtworkFile[];

/** A track's artwork file, for a webview to load directly. */
export type ArtworkFile = {
  /** Absolute, with symlinks resolved, and inside the drive. */
  path: string;
  mime: string;
};

/** A track's artwork in a batch: `image` is `None` when it has none. */
export type TrackArtwork_for_ArtworkFile = {
  track: number;
  image: ArtworkFile | null;
};

export type AutoArtworkPlan = {
  found: AutoArtworkItem[];
  not_found: TrackRef[];
};

export type AutoArtworkItem = {
  track: TrackRef;
  /** `embedded` or the image file's key. */
  source: string;
};

export type ArtworkReport = {
  tracks_updated: number;
  artwork_rows_added: number;
  files_written: string[];
  backup: string | null;
  fingerprint: Hash | null;
};

/** What embedding art in files would rewrite. */
export type EmbedPlan = {
  image: string;
  files: EmbedTarget[];
  /** Files left alone, and why (on a rekordbox export, missing...). */
  skipped: Skip[];
  /** Kept in the store before they're rewritten, so `art unembed` can put them back. */
  backup_bytes: number;
};

/** A file art would be embedded in. */
export type EmbedTarget = {
  path: string;
  size: number;
  /** It has a front cover already, which is replaced. */
  replaces_art: boolean;
};

export type Skip = [path: string, reason: string];

/** What embedding art into files did. */
export type EmbedReport = {
  embedded: string[];
  failed: Failure[];
  /** The copies of the files as they were (see [`restore_embedded`]). */
  backup: string | null;
};

export type Failure = [path: string, error: string];

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

export type ExportReport = {
  format: ExportFormat;
  files: ExportedFile[];
};

export type ExportedFile = {
  path: string;
  /** The playlist (or history list, or library) it holds. */
  name: string;
  tracks: number;
};

export type DoctorReport = {
  drive: string;
  root: string;
  tracks: number;
  /** Each section is `None` when it wasn't run (see [`DoctorOptions::only`]). */
  duplicates: DuplicateGroup[] | null;
  missing: BrokenTrack[] | null;
  /** `None` also when OneLibrary can't be read (then what it references is unknown). */
  orphans: OrphanFile[] | null;
  /** Tracks in no playlist. */
  unfiled: TrackRef[] | null;
  /** How `export.pdb` and OneLibrary differ (`None` also on a drive with only one of them). */
  drift: LibraryDrift | null;
  large_files: LargeFile[] | null;
  /** Audio files cut short or that don't decode (with [`DoctorOptions::audio`]). */
  audio: AudioReport | null;
  /**
   * What an analysis of the audio says about stored tempos, keys and levels (with
   * [`DoctorOptions::analyze`]). Opinions to look into, not problems.
   */
  analysis: AnalysisReport | null;
  /** How players get on with the drive's disk, when it is a mounted volume's root. */
  compatibility: Compatibility | null;
  /** The volume's file system as the OS names it. */
  file_system: string | null;
  /** What plugins' checks found, each named `<plugin>.<check>`. */
  checks: CheckItem[] | null;
  warnings: string[];
};

export type DuplicateGroup = {
  reasons: DuplicateReason[];
  /** Suggested keeper first. */
  tracks: DuplicateTrack[];
  /**
   * Id of the suggested keeper: the best audio (lossless over lossy, then the higher bitrate,
   * sample rate and bit depth), then the most cues, playlists and history lists, then the
   * earliest date added.
   */
  keep: number;
  /** What put the keeper ahead of the next copy (`lossless over lossy`, `320 kbps over 192`). */
  keep_reason: string;
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
  track: TrackRef;
  /** Playlists the track is in. */
  playlists: number;
  /** History lists (what players recorded) the track is in. */
  history: number;
  /** Hot cues, memory cues and loops in its analysis files. */
  cues: number;
  play_count: number;
  /** FLAC, WAV, AIFF or Apple Lossless. */
  lossless: boolean;
  bitrate: number;
  sample_rate: number;
  bit_depth: number;
  file_size: number;
  date_added: string;
};

export type BrokenTrack = {
  track: TrackRef;
  problem: DoctorProblem;
  /** The file concerned, as a drive key. */
  path: string;
  detail: string | null;
  /** The database listing the track: `device_library` (`export.pdb`) or `onelibrary`. */
  database: string;
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

export type OrphanFile = {
  /** Drive key. */
  path: string;
  size: number;
  class: FileClass;
};

export type LargeFile = {
  path: string;
  size: number;
  /** The track using it, if any. */
  track: TrackRef | null;
};

export type AudioReport = {
  /** Audio files opened. */
  checked: number;
  problems: AudioFinding[];
};

/** A track whose audio file has a problem. */
export type AudioFinding = {
  track: TrackRef;
  problem: AudioProblem;
};

/** What's wrong with an audio file. */
export type AudioProblem =
  /** It doesn't open as audio, or there's no decoder for it. */
  | {
    kind: "unreadable";
    error: string;
  }
  /**
   * Its audio stops before the length the database (or its own header) gives: a copy cut
   * short, most likely.
   */
  | {
    kind: "truncated";
    ends_secs: number;
    expected_secs: number;
  }
  /** Its last seconds don't decode. */
  | {
    kind: "undecodable";
    error: string;
  }
  /** Its header gives a different length from the database's (a replaced file?). */
  | {
    kind: "length_differs";
    file_secs: number;
    expected_secs: number;
  };

/**
 * Tracks whose stored tempo or key the estimates question, and tracks whose level may be a
 * problem.
 */
export type AnalysisReport = {
  /** Tracks with an analysis (kept from before, or new). */
  analyzed: number;
  /** Analysed just now (the rest were kept from before). */
  decoded: number;
  /** Tracks whose audio couldn't be analysed. */
  failed: TrackFailure[];
  tempo: TempoFinding[];
  key: KeyFinding[];
  loudness: LoudnessFinding[];
  /** The median integrated loudness of the analysed tracks (LUFS). */
  median_lufs: number | null;
};

export type TrackFailure = [track: TrackRef, error: string];

/** A track whose stored tempo the estimate doesn't agree with. */
export type TempoFinding = {
  track: TrackRef;
  /** The library's BPM. */
  stored: number;
  /** The analyzer's BPM. */
  estimate: number;
  /** 0 to 1 (none: the analyzer doesn't say). */
  confidence: number | null;
  /**
   * The stored tempo against the estimate: `half`, `double`, `two_thirds`, `three_halves` or
   * `disagrees`.
   */
  relation: TempoRelation;
  /** Which of the two the library's other tempos make more likely, if either. */
  likelier: Plausible | null;
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

/** Which of two tempos a [`TempoPrior`] finds more likely. */
export type Plausible = "stored" | "estimate";

/** A track whose stored key the estimate doesn't agree with. */
export type KeyFinding = {
  track: TrackRef;
  /** The library's key, as it stores it. */
  stored: string;
  /** The analyzer's key, as a Camelot code (`8A`). */
  estimate: string;
  /** 0 to 1 (none: the analyzer doesn't say). */
  confidence: number | null;
  /** The estimate against the stored key: `fifth`, `relative`, `parallel` or `other`. */
  relation: KeyOutcome;
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

/** A track whose level may be a problem. */
export type LoudnessFinding = {
  track: TrackRef;
  problems: LoudnessProblem[];
  /** LUFS (`None` for silence). */
  integrated_lufs: number | null;
  true_peak_dbtp: number;
  clipped_runs: number;
};

/** What may be wrong with a track's level. */
export type LoudnessProblem =
  /** Its true peak is above 0 dBTP: a player's converter or a lossy encode can clip it. */
  | "over_zero"
  /** Runs of samples at full scale: clipped when it was mastered or converted. */
  | "clipped"
  /** Its integrated loudness is more than [`FAR_FROM_MEDIAN_LU`] from the library's median. */
  | "far_from_median";

export type Compatibility = {
  verdict: Verdict;
  summary: string;
  /** Equipment that can't use it (when only some can). */
  unsupported: string[];
  /** Caveats worth knowing. */
  notes: string[];
};

export type Verdict =
  | "no_players"
  /** Every rekordbox player. */
  | "all_players"
  /** Newer players only. */
  | "some_players"
  /** Untested layouts players may or may not read. */
  | "unknown";

/**
 * What a check found on one of the drives. Its `check` is `export`, `same_library`, `databases`,
 * `files`, `audio`, `loudness`, `large_files`, `interrupted`, `history`, `file_system`, `players`
 * or `formats`, or a plugin's `<plugin>.<check>`; what was found fine is `info`.
 */
export type CheckItem = {
  /** The drive it's about. */
  drive: string;
  level: Level;
  /** The check that found it (`files`, a plugin's `<plugin>.<check>`). */
  check: string;
  /** What it found, in a line. */
  message: string;
  /** The tracks it's about, by id. */
  tracks?: number[];
  /** The playlist it's about, by path (`Folder/Name`). */
  playlist?: string | null;
  /** The files it's about, relative to the drive's root (`Contents/a.mp3`). */
  paths?: string[];
  /**
   * What fixes it, a plugin's check says: its plugin's operation, planned on the drive and
   * confirmed as any is.
   */
  fix?: Fix | null;
};

/** How much a finding matters, least first. */
export type Level =
  /** Worth knowing, nothing to fix (what was checked and found fine, too). */
  | "info"
  /** Worth a look: it may be a problem, or a problem for some players. */
  | "warning"
  /** Something to fix before it's used. */
  | "problem";

/**
 * The plugin operation that fixes a finding, with its params: planned and confirmed as any is
 * (`plugins.op.plan`, with the drive it was found on).
 */
export type Fix = {
  plugin: string;
  operation: string;
  params: unknown;
};

export type OrphanPlan = {
  drive: string;
  root: string;
  files: OrphanFile[];
  /** Total size of `files`. */
  bytes: number;
  /** Space needed for the backup of the database and the deleted files. */
  needed_bytes: number;
  free_space: number | null;
  fits: boolean;
  warnings: string[];
};

export type OrphanCleanReport = {
  files_deleted: number;
  freed_bytes: number;
  backup: string | null;
  fingerprint: Hash | null;
};

export type CheckReport = {
  drives: string[];
  items: CheckItem[];
  problems: number;
  warnings: number;
  /** The player models each drive was checked against, and where they came from. */
  players: DrivePlayers[];
  /** Tracks a player won't load, grouped as the `formats` items describe them. */
  unloadable: Unloadable[];
  /**
   * With [`CheckOptions::audio`]: audio files with a problem, grouped as the `audio` items
   * describe them.
   */
  audio: AudioIssues[];
  /** Tracks whose level may be a problem, as the `loudness` items describe them. */
  loudness: DriveLoudness[];
};

export type DrivePlayers = {
  drive: string;
  models: string[];
  from: PlayersFrom;
  /** The library they're remembered for (with `from: library`). */
  library?: string | null;
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

/** Tracks one player won't load, for one reason. */
export type Unloadable = {
  drive: string;
  player: string;
  /** `problem`, or `warning` when it rests on a value the model's specs only infer. */
  level: Level;
  /** The format (`FLAC`), or the file extension when it's no format players list. */
  format: string;
  reason: Unplayable;
  /** The sample rate (Hz) the player doesn't take, when that's the reason. */
  sample_rate?: number | null;
  /** The bit depth the player doesn't take, when that's the reason. */
  bit_depth?: number | null;
  /** The `formats` item's message. */
  message: string;
  /** One per audio file. */
  tracks: TrackRef[];
  /**
   * The files are lossless, so packing them as lossless copies the player plays fixes this
   * (see [`crate::pack::Transcode`]).
   */
  transcodable: boolean;
};

/** How a model gets on with a file. */
export type Unplayable =
  | ("sample_rate" | "bit_depth" | "sample_rate_and_bit_depth")
  /** It doesn't play the format at all. */
  | "format";

/** Audio files with one kind of problem. */
export type AudioIssues = {
  drive: string;
  level: Level;
  /** `truncated`, `unreadable`, `undecodable` or `length_differs` (see [`probe::AudioProblem`]). */
  kind: string;
  /** The `audio` item's message. */
  message: string;
  files: AudioFinding[];
};

/** One drive's tracks whose level may be a problem. */
export type DriveLoudness = {
  drive: string;
  tracks: LoudnessFinding[];
};

/** Each analyzer's scores. */
export type BenchReport = {
  /** Key reports by analyzer (`rekordbox`, `stratum-dsp`): `score` is MIREX's weighted score. */
  keys: { [key: string]: KeyOutcomeReport };
  /** Tempo reports by analyzer: `score` is Accuracy1. */
  tempos: { [key: string]: TempoOutcomeReport };
  /** Tempo Accuracy2 (percent) by analyzer. */
  accuracy2: { [key: string]: number };
  /**
   * Annotated tracks the analysis had no audio for: not on the drive, missing from it, or not in
   * the audio folder.
   */
  unmatched: number;
  /** Audio files that couldn't be analysed: `[name, error]`. */
  failed: [string, string][];
};

/** How one analyzer did. */
export type KeyOutcomeReport = {
  tracks: number;
  /** Tracks with each outcome, every outcome listed. */
  counts: { [key: string]: number };
  /** The tracks it didn't get fully right, in the order they came, with what it did instead. */
  misses: [string, KeyOutcome][];
  /** The mean credit, 0 to 1: MIREX's weighted key score, or tempo Accuracy1. */
  score: number;
};

/** How one analyzer did. */
export type TempoOutcomeReport = {
  tracks: number;
  /** Tracks with each outcome, every outcome listed. */
  counts: { [key: string]: number };
  /** The tracks it didn't get fully right, in the order they came, with what it did instead. */
  misses: [string, TempoOutcome][];
  /** The mean credit, 0 to 1: MIREX's weighted key score, or tempo Accuracy1. */
  score: number;
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

export type DriveHistory = {
  drive: string;
  lists: HistoryList[];
  warnings: string[];
};

/** One history list: the tracks a player played during one session, in order. */
export type HistoryList = {
  name: string;
  /** Folders and name (OneLibrary can keep history lists in folders). */
  path: string;
  /**
   * The database it was read from: `device_library` (`export.pdb`) or `onelibrary` (the first
   * one, when both hold it).
   */
  database: string;
  tracks: Track[];
  /** Entries whose track isn't in the library any more. */
  missing: number;
};

export type HistoryArchive = {
  /** Oldest first. */
  lists: ArchivedHistory[];
  /** Snapshots read. */
  snapshots: number;
  /** Snapshots that couldn't be read. */
  warnings: string[];
};

/** One history list: the tracks a player played during one session, in order. */
export type ArchivedHistory = {
  name: string;
  /** Folders and name (OneLibrary can keep history lists in folders). */
  path: string;
  /**
   * The database it was read from: `device_library` (`export.pdb`) or `onelibrary` (the first
   * one, when both hold it).
   */
  database: string;
  tracks: Track[];
  /** Entries whose track isn't in the library any more. */
  missing: number;
  /** The earliest snapshot holding it: it was recorded before then. */
  first_seen: Sighting;
  /**
   * It was recorded after this: when the snapshot before `first_seen` of the same drive was
   * taken (which didn't hold it). `None` when that drive wasn't pulled before.
   */
  recorded_after: string | null;
  /** How many snapshots hold it. */
  snapshots: number;
};

/** Where an archived list was first seen. */
export type Sighting = {
  snapshot: Hash;
  short_id: string;
  created: string;
  library: string;
  /** The drive the snapshot was pulled from. */
  drive: string | null;
};

export type PlayStats = {
  /** What the statistics are about: a drive, or a snapshot. */
  source: string;
  tracks: number;
  history_lists: number;
  /** Tracks with a play count or in a history list. */
  played: number;
  /** Play count 0 and in no history list. */
  never_played: TrackRef[];
  /** Most played first. */
  most_played: TrackPlays[];
  /** With a `since` snapshot: what was played between it and this state. */
  since: PlayedSince | null;
};

export type TrackPlays = {
  track: TrackRef;
  /** The play count rekordbox and the players keep (the higher of the two databases'). */
  play_count: number;
  /** History lists the track appears in. */
  history_lists: number;
};

export type PlayedSince = {
  snapshot: Hash;
  short_id: string;
  created: string;
  /** History lists recorded since. */
  new_histories: string[];
  /** Tracks played since, most first. */
  tracks: PlayDelta[];
};

export type PlayDelta = {
  track: TrackRef;
  before: number;
  after: number;
  /** History lists recorded since that the track appears in. */
  history_lists: number;
};

export type Model = {
  /** As AlphaTheta writes it: `CDJ-2000NXS2`, `OPUS-QUAD`. */
  name: string;
  kind: Kind;
  /** Reads Device Library (`export.pdb`). */
  export_pdb: Spec_for_boolean;
  /** Reads OneLibrary (`exportLibrary.db`, called Device Library Plus before October 2025). */
  onelibrary: Spec_for_boolean;
  exfat: Spec_for_Exfat;
  /** The formats it plays. A format missing from the list doesn't play. */
  audio: Audio[];
  /** The list of formats is itself inferred: a format missing from it probably doesn't play. */
  audio_inferred: boolean;
  /** What else is worth knowing about it. */
  notes: string[];
  /** Where the values come from. */
  source: string;
};

export type Kind =
  /** A multi player (CDJ, XDJ-1000...). */
  | "player"
  /** Players and a mixer in one unit (XDJ-XZ, OPUS-QUAD...). */
  | "all_in_one"
  /** A mixer that reads USB drives for settings and firmware only. */
  | "mixer";

/** A value, and whether it's inferred rather than documented. */
export type Spec_for_boolean = {
  value: boolean;
  inferred: boolean;
};

/** A value, and whether it's inferred rather than documented. */
export type Spec_for_Exfat = {
  value: Exfat;
  inferred: boolean;
};

/** Whether a model reads exFAT. */
export type Exfat =
  | ("no" | "yes")
  /** With this firmware version or later. */
  | {
    firmware: string;
  };

/** One audio format a model plays, at these sample rates (Hz) and bit depths. */
export type Audio = {
  codec: Codec;
  rates: number[];
  depths: number[];
  inferred: boolean;
};

/** An audio format players list. */
export type Codec = "mp3" | "aac" | "wav" | "aiff" | "flac" | "alac";

/** The player models `check` uses when none are given. */
export type RememberedPlayers = {
  /** For drives of a library without models of its own. */
  default: string[];
  libraries: LibraryPlayers[];
};

export type LibraryPlayers = {
  id: string;
  /** `None` when the library is no longer in the store. */
  name: string | null;
  models: string[];
};

/** What remembering (or forgetting) player models would change. */
export type PlayersPlan = {
  /**
   * The library they're for; `None` for the default (every library without models of its
   * own).
   */
  library: string | null;
  library_name: string | null;
  /** The models remembered now. */
  from: string[];
  /** The models remembered after (empty: forgotten). */
  to: string[];
};

/** A disk, how players would get on with it as it is, and whether it can be formatted. */
export type DiskView = {
  disk: Disk;
  identity: string;
  compatibility: Compatibility;
  /** Why it can't be formatted (`None`: it can). */
  problem: string | null;
};

export type Disk = {
  /** OS identifier: `disk4` (macOS), `/dev/sdb` (Linux), `PhysicalDrive2` (Windows). */
  id: string;
  /** Whole-disk device path. */
  device: string;
  model: string;
  serial?: string | null;
  size: number;
  sector_size: number;
  /** "USB", "SD"... */
  bus: string | null;
  /** External, removable or hot-pluggable. */
  removable: boolean;
  internal: boolean;
  /** Holds the running operating system. */
  system: boolean;
  /** A disk image, loop device or virtual disk. */
  virtual_disk: boolean;
  read_only: boolean;
  /** `None` when the disk has no partition table. */
  scheme: Scheme | null;
  volumes: Volume[];
};

export type Scheme =
  | "other"
  /** Master Boot Record ("FDisk", "dos"). */
  | "mbr"
  /** GUID partition table. */
  | "gpt"
  /** Apple partition map. */
  | "apm";

/** One partition (or, for disks without a partition table, the disk itself). */
export type Volume = {
  /** OS identifier: `disk4s1`, `/dev/sdb1`, `2:1` (disk:partition on Windows). */
  id: string;
  size: number;
  filesystem: FsKind | null;
  /** The file system as the OS names it ("MS-DOS FAT32", "vfat", "exFAT"...). */
  filesystem_name: string | null;
  label: string | null;
  mount: string | null;
  cluster_size: number | null;
  uuid?: string | null;
};

/** A file system found on a volume. */
export type FsKind = "fat12" | "fat16" | "fat32" | "exfat" | "hfsplus" | "apfs" | "ntfs" | "other";

export type FormatPlan = {
  disk: Disk;
  /** Pass this to [`apply`] to confirm the disk. */
  identity: string;
  filesystem: Filesystem;
  scheme: Scheme;
  label: string;
  cluster_size: number;
  /** Size of the new partition. */
  partition_size: number;
  erases: ErasedVolume[];
  /** The locallibrary name and id the drive keeps (only when the OS mounts it afterwards). */
  keeps_name: string | null;
  /** How players will get on with the result. */
  compatibility: Compatibility;
  /** What will run. */
  steps: string[];
  warnings: string[];
};

/** A volume that formatting destroys. */
export type ErasedVolume = {
  volume: Volume;
  /** locallibrary's name for the drive, if it was adopted. */
  drive_name: string | null;
  /**
   * Its role in the store. An archive or retired drive is only erased when writing to one is
   * allowed (see [`crate::fleet::check_target`]).
   */
  role: Role | null;
  /** Retired in the store. */
  retired: boolean;
  /** Tracks in the rekordbox library on it, if any. */
  tracks: number | null;
  /** Whether that library's current state is in the store (`None`: no library, or no store). */
  in_store: boolean | null;
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

export type FormatReport = {
  disk: string;
  filesystem: Filesystem;
  label: string;
  /** Where the new volume is mounted (`None` if the OS didn't mount it). */
  mount: string | null;
  /** Cluster size of the mounted volume, as the OS reports it. */
  cluster_size: number | null;
  capacity: number | null;
  /** The locallibrary name the drive kept. */
  drive_name: string | null;
};

export type AccountStatus = {
  /** The signed-in account (`None`: not signed in). */
  account: AccountInfo | null;
  /** The handle last signed in with, to offer next time. */
  last_handle: string | null;
};

/** The signed-in Atmosphere account. */
export type AccountInfo = {
  did: string;
  /** As the account's server reports it (`None` when the handle doesn't check out). */
  handle: string | null;
  /** The account's data server (PDS). */
  pds: string;
  /** Where it signed in: the PDS, or an entryway such as `https://bsky.social`. */
  auth_server: string;
  /** What was granted. */
  scopes: string[];
  signed_in_at: string;
  /** The latest a session can last on Bluesky's servers; sign in again before then. */
  sign_in_again_by: string;
  access_expires_at: string | null;
  client: Client;
  storage: Storage;
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
    kind: "hosted";
    client_id: string;
  };

/** Where secrets are kept. */
export type Storage =
  /** macOS Keychain, Windows Credential Manager, or the Secret Service on Linux. */
  | "keychain"
  /**
   * Files in the store folder, readable only by you (`atmosphere-session.json`,
   * `secrets/plugins/<id>.json`): for computers without a keychain (a Linux server without a
   * desktop session).
   */
  | "file";

/** A sign-in waiting for the browser. */
export type LoginStart = {
  /** Pass to [`finish_login`] / [`finish_login_with_url`]. */
  id: string;
  /** Open this in the browser. */
  url: string;
  /** The host the account signs in at, e.g. `bsky.social`. */
  server: string;
  redirect_uri: string;
  /** The client signing in. */
  client: Client;
  /** Why it isn't locallibrary's own client, when it fell back to the development one. */
  note: string | null;
};

export type LogoutReport = {
  /** The account that was signed in (`None`: nobody was). */
  account: AccountInfo | null;
  /**
   * Whether the server revoked the session's tokens. When it couldn't be reached (or offers
   * no revocation), the session is still removed from this computer and ends on its own.
   */
  revoked: boolean;
  /**
   * Why some of the session's secrets are still on this computer (the keychain didn't
   * answer); signing out again tries again. `None`: they're gone.
   */
  left_behind: string | null;
};

export type ProfileView = {
  /** Everything worked out (stays on this computer). */
  report: ProfileReport;
  prefs: ProfilePrefs;
  /** Sections the document includes. */
  published: ProfileSection[];
  /** What a public profile would show. */
  document: ProfileDocument;
};

export type ProfileReport = {
  generated: string;
  options: ProfileOptions;
  basis: Basis;
  sources: SourceInfo[];
  summary: ProfileSummary;
  sets: SetsSection;
  tempo: TempoSection;
  arc: ArcSection;
  mixing: MixingSection;
  keys: KeysSection;
  /** `None` without analysis files. */
  energy: EnergySection | null;
  genres: RankedSection;
  labels: RankedSection;
  artists: RankedSection;
  era: EraSection;
  signature: SignatureSection;
  /** `None` without analysis files. */
  prep: PrepSection | null;
  collection: CollectionSection;
  /** `None` when no drive has My Settings files and the store has no settings profile. */
  style: StyleSection | null;
  warnings: string[];
};

export type ProfileOptions = {
  /**
   * Only sets played on or after this day. Sessions (without declared sets) count from the day
   * a pull first archived them; those still only on a drive are recent, so they always count.
   */
  since: string | null;
  /** Only sets played on or before this day (sessions still only on a drive don't count). */
  until: string | null;
  /** Without declared sets: sessions with fewer tracks (sound checks, previews) don't count. */
  min_set_tracks: number;
  /** How many entries ranked lists keep. */
  top: number;
  /** Read analysis files for phrase mood and cue statistics (one or two small files per track). */
  analysis: boolean;
  /** The DJ's genre spellings, e.g. `{"dnb": "Drum & Bass"}` (see [`ProfilePrefs`]). */
  genre_aliases: { [key: string]: string };
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

export type SourceInfo = {
  /** `drive` or `store`. */
  kind: string;
  name: string;
  /** History lists (sessions) read. */
  history_lists: number;
  tracks: number;
};

export type ProfileSummary = {
  headline: string;
  tags: Tag[];
};

/** A trait the numbers support, with the evidence. */
export type Tag = {
  id: string;
  label: string;
  why: string;
};

/**
 * The sets the numbers come from: the DJ's, or the sessions standing in for them (see
 * [`ProfileReport::basis`]).
 */
export type SetsSection = {
  count: number;
  /** Track appearances across every set. */
  plays: number;
  distinct_tracks: number;
  median_tracks: number;
  longest: number;
  /** The days of the oldest and newest sets ([`SetInfo::date`]). */
  oldest: string | null;
  newest: string | null;
  /** Sessions only on a drive, which the store hasn't archived yet. */
  not_archived: number;
  /** Sessions left out for having fewer than `min_set_tracks` tracks. */
  too_short: number;
  /** Newest first (sessions only on a drive, undated, first). */
  list: SetInfo[];
};

export type SetInfo = {
  /**
   * The set's id, or the session's (its history list's fingerprint, see
   * [`crate::sets::Session::id`]).
   */
  id: string;
  /** The set's title, else its venue; the session's history list (folders and name). */
  name: string;
  /**
   * Where: the set's venue and city; the drive a session was read from, or whose snapshot
   * archived it.
   */
  place: string;
  /**
   * The day the set was played. A session has no such day: this is the day a pull first
   * archived it (it was played then or before), and none while it's only on a drive.
   */
  date: string | null;
  tracks: number;
  opening_bpm: number | null;
  closing_bpm: number | null;
  /** Share of transitions between keyed tracks that are harmonic. */
  harmonic_share: number | null;
};

export type TempoSection = {
  /** Plays (or tracks) with a BPM. */
  tracks: number;
  median: number | null;
  /** Where the middle 80% sits: the 10th and 90th percentiles. */
  core: BpmRange | null;
  p25: number | null;
  p75: number | null;
  min: number | null;
  max: number | null;
  bin_width: number;
  /** Covering the 2nd to 98th percentile. */
  histogram: TempoBin[];
};

export type TempoBin = [bpm: number, count: number];

export type ArcSection = {
  /** Sets long enough to have a shape (6+ tracks with a BPM). */
  sets: number;
  /** Median BPM over the first and last quarter of a set, averaged over sets. */
  opening_bpm: number | null;
  closing_bpm: number | null;
  /**
   * Shares of sets whose closing quarter is at least 2 BPM faster, within 2 BPM, or at least
   * 2 BPM slower than their opening quarter.
   */
  builds: number;
  steady: number;
  winds_down: number;
  /** BPM relative to each set's median, across ten stretches of a set, averaged over sets. */
  curve: (number | null)[];
};

export type MixingSection = {
  /**
   * Pairs of tracks played one after the other within sets (not across a track gone from
   * the library, nor a track played twice in a row).
   */
  transitions: number;
  /** Transitions where both keys are known. */
  keyed: number;
  same_key: number;
  adjacent: number;
  relative: number;
  energy_boost: number;
  clash: number;
  /** (same + adjacent + relative + energy boost) / keyed. */
  harmonic_share: number | null;
  /** Transitions where both BPMs are known. */
  timed: number;
  /**
   * Median tempo change between consecutive tracks, in percent (half and double time count
   * as the same tempo).
   */
  median_step_pct: number | null;
  within_3pct: number | null;
  within_6pct: number | null;
  /** Transitions that only line up at half or double time. */
  half_double: number;
  /** Transitions where both tracks have a genre. */
  genred: number;
  genre_change_share: number | null;
};

export type KeysSection = {
  tracks: number;
  /** Every Camelot key in wheel order (1A, 1B, 2A ... 12B), with its count. */
  wheel: KeyCount[];
  minor_share: number | null;
  top: Share[];
};

export type KeyCount = [key: string, count: number];

export type Share = {
  name: string;
  count: number;
  /** Of `known`. */
  share: number;
};

export type EnergySection = {
  /** Plays (or tracks) whose phrase analysis was read. */
  tracks: number;
  high: number;
  mid: number;
  low: number;
  /** Average mood (high 1, mid 0.5, low 0) across ten stretches of a set. */
  curve: (number | null)[];
};

export type RankedSection = {
  /** Plays (or tracks) with a value. */
  known: number;
  distinct: number;
  top: Share[];
};

export type EraSection = {
  /** Plays (or tracks) with a release year. */
  tracks: number;
  median_year: number | null;
  oldest: number | null;
  newest: number | null;
  /** Released the year it was played, or the year before. */
  recent_share: number | null;
  /** Oldest first. */
  decades: DecadeCount[];
};

export type DecadeCount = [decade: number, count: number];

export type SignatureSection = {
  /** Played in two or more sets, most sets first. */
  tracks: PlayedTrack[];
  /** The same move from one track into another, in two or more sets. */
  transitions: SignatureTransition[];
  /** Opened (or closed) two or more sets. */
  openers: PlayedTrack[];
  closers: PlayedTrack[];
  /** Every track played, most sets first. */
  played: PlayedTrack[];
};

export type PlayedTrack = {
  name: string;
  artist: string;
  title: string;
  /** Sets it appears in. */
  sets: number;
  plays: number;
  opened: number;
  closed: number;
  /**
   * A declared set marks it unnamed (`sets edit --unnamed`): the public document never names
   * it.
   */
  unnamed?: boolean;
};

export type SignatureTransition = {
  from: string;
  to: string;
  sets: number;
};

export type PrepSection = {
  /** Collection tracks whose cues were read. */
  tracks: number;
  hot_cue_share: number;
  /** Hot cues per track, on tracks that have any. */
  hot_cues_per_track: number | null;
  memory_cue_share: number;
  loop_share: number;
};

export type CollectionSection = {
  tracks: number;
  playlists: number;
  folders: number;
  rated_share: number | null;
  coloured_share: number | null;
  /** Tracks played in at least one set. */
  played_share: number | null;
  /**
   * Extra files of a recording already counted (the most any one library has): copies such
   * as `Track-1.mp3` left by repeated exports. `locallibrary doctor` lists them.
   */
  duplicate_files: number;
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

/** The DJ's choices about their profile, kept in the store. */
export type ProfilePrefs = {
  /**
   * Sections the document includes (`None`: the defaults, see
   * [`Section::published_by_default`]).
   */
  sections?: ProfileSection[] | null;
  precision: Precision;
  /** Tracks ("Artist - Title") to list first among signature tracks, in this order. */
  pinned: string[];
  /** Tracks never named (unreleased promos, edits, dubplates). They still count in the numbers. */
  hidden: string[];
  /** Artists never named, wherever they're credited. */
  hidden_artists: string[];
  /** The DJ's genre spellings: genre text as tagged → genre as shown. */
  genre_aliases: { [key: string]: string };
  /** The DJ's own one-line description, instead of the generated one. */
  headline?: string | null;
};

export type ProfileDocument = {
  version: number;
  generatedAt: string;
  precision: Precision;
  /** `plays`, `sessions` or `collection` (see [`Basis`]). */
  basis: Basis;
  headline?: string | null;
  tags?: string[];
  sets?: DocSets | null;
  tempo?: DocTempo | null;
  arc?: DocArc | null;
  mixing?: DocMixing | null;
  keys?: DocKeys | null;
  energy?: DocEnergy | null;
  genres?: DocShare[] | null;
  labels?: DocShare[] | null;
  /** A track credited to several artists counts for each, so shares can add up to more than 1. */
  artists?: DocShare[] | null;
  era?: DocEra | null;
  signature?: DocTrack[] | null;
  prep?: DocPrep | null;
  collection?: DocCollection | null;
  /** Setting key → value, e.g. `{"quantize": "off", "jogMode": "vinyl"}`. */
  style?: { [key: string]: string } | null;
};

/** A count: exact, or (rounded) a round number it's at least. */
export type DocSets = {
  /** Year of the oldest set. */
  since?: number | null;
  medianTracks: number;
} & ({
  count: number;
} | {
  atLeast: number;
});

export type DocTempo = {
  low: number;
  high: number;
  median: number;
};

export type DocArc = {
  shape: ArcShape;
  builds: number;
  steady: number;
  windsDown: number;
};

export type ArcShape = "builds" | "steady" | "windsDown" | "varies";

export type DocMixing = {
  harmonic?: number | null;
  /** Transitions that change tempo by 3% or less. */
  smoothTempo?: number | null;
  genreChanges?: number | null;
};

export type DocKeys = {
  top: string[];
  minor?: number | null;
};

export type DocEnergy = {
  high: number;
  mid: number;
  low: number;
};

export type DocShare = {
  name: string;
  share: number;
};

export type DocEra = {
  medianYear?: number | null;
  recent?: number | null;
};

export type DocTrack = {
  artist: string;
  title: string;
};

export type DocPrep = {
  hotCues: number;
  memoryCues: number;
  loops: number;
};

export type DocCollection = {
  tracks: Count;
  playlists: Count;
};

/** A count: exact, or (rounded) a round number it's at least. */
export type Count = {
  count: number;
} | {
  atLeast: number;
};

/** A record ready to publish, and what's published in its place now. */
export type PublishPlan = {
  /** The account it goes to. `None`: nobody's signed in, so it can only be looked at. */
  account: AccountInfo | null;
  collection: string;
  rkey: string;
  /** `at://did/collection/rkey` (`None` while signed out). */
  uri: string | null;
  /** The record exactly as it will be written. */
  record: unknown;
  /**
   * What's published there now, as Local Network's directory has it (`None`: nothing yet, as
   * far as it knows).
   */
  current: Fetched | null;
  /** Whether publishing changes anything (time stamps aside). */
  changed: boolean;
  /**
   * The scope the session lacks to write the record: sign in again allowing publishing
   * (`account.login.begin` with `publish: true`) before applying.
   */
  missing_scope: string | null;
  /** What the DJ should know before publishing, in words: guesses made, things left out. */
  notes: string[];
};

/** A record as the directory has it. */
export type Fetched = {
  uri: string;
  /** The version the directory has. */
  cid: string;
  /**
   * The record as its collection's record script kept it, which may leave fields out and add
   * its own.
   */
  value: unknown;
};

/** A record published. */
export type Published = {
  uri: string;
  /** Its content hash. */
  cid: string;
  /** False when what's published was already the same, so nothing was written. */
  written: boolean;
  record: unknown;
};

/**
 * What looking up the MusicBrainz IDs of a set's tracks did (`sets.lookup`), counting the tracks
 * the record names without IDs by file, once however often the set plays them.
 */
export type SetLookup = {
  /**
   * Tracks the analyzers ran on (or had worked out already): those on a plugged-in drive that
   * lists them.
   */
  asked: number;
  /** Of those, tracks that got MusicBrainz IDs, now kept in the set. */
  found: number;
  /** Of those, tracks an analyzer failed on. Each failure was a warning. */
  failed: number;
  /** Tracks whose files are on no plugged-in drive: not looked up. */
  unmounted: number;
};

/** Someone's published My Settings, and how importing them changes the settings profile. */
export type SettingsImportPlan = {
  from: Resolved;
  uri: string;
  cid: string;
  published: PublishedSettings;
  /**
   * Whether importing replaces the whole base, so settings they didn't publish become unset.
   * Otherwise their settings are added to the base, over the values it has for them.
   */
  replace: boolean;
  /** Base values that change. */
  changes: BaseChange[];
};

/** An account found from its handle or DID. */
export type Resolved = {
  did: string;
  /** When it was found by handle. */
  handle: string | null;
};

/** The settings a published settings record holds, as a settings profile's base values. */
export type PublishedSettings = {
  values: { [key: string]: string };
  /**
   * `group.field = value` entries this version of locallibrary doesn't know (newer firmware,
   * or a newer locallibrary), left out.
   */
  skipped: string[];
  updated_at: string | null;
};

/** A performance: what the DJ played, where and when. */
export type Set = {
  /**
   * A TID ([`new_id`]): the record key when it's published, and how commands name it (any
   * unique prefix of 4 or more characters).
   */
  id: string;
  title?: string | null;
  /** The day it was played (local). */
  date: string;
  /**
   * `club`, `festival`, `radio`, `livestream`, `mix` (the Lexicon's `knownValues`) or the DJ's
   * own word.
   */
  kind?: string | null;
  venue?: string | null;
  city?: string | null;
  /** ISO 3166 country code, `GB`. */
  country?: string | null;
  /** The event it was part of: the `at://` URI of a `community.lexicon.calendar.event` record. */
  event?: string | null;
  /** Who they played with (back to back). */
  with?: Partner[];
  /** Where people can listen to it (Mixcloud, SoundCloud, YouTube...). */
  link?: string | null;
  /** What the link is, as the plugin that published the set there named it (`plyr.fm`). */
  link_name?: string | null;
  /**
   * The Atmosphere record its recording can be played from (a plyr.fm track), as the plugin
   * that published it there said.
   */
  recording_record?: StrongRef | null;
  /** The recording on this computer the tracklist was found in. */
  recording?: SetRecording | null;
  /** The DJ's own notes. Never published. */
  notes?: string | null;
  /** The sessions it was built from, by id ([`Session::id`]). */
  sessions?: string[];
  /** In play order. */
  tracks: SetEntry[];
  created_at: string;
  updated_at: string;
};

/** Someone the DJ played with. */
export type Partner = {
  name: string;
  /** Their Atmosphere account, when known. */
  did?: string | null;
};

/** An Atmosphere record and the version of it meant, as `com.atproto.repo.strongRef` has them. */
export type StrongRef = {
  /** Its at:// URI. */
  uri: string;
  /** Its CID. */
  cid: string;
};

/** A recording of a set. */
export type SetRecording = {
  path: string;
  /** The mix report kept in the store for it (`mix.reports.get`). */
  report?: string | null;
};

/**
 * One track of a set, kept as it was when it went in, so the set outlives library changes and
 * reads without the drive.
 */
export type SetEntry = {
  title: string;
  artist: string;
  album?: string;
  label?: string;
  genre?: string;
  isrc?: string;
  /** As the library has it (note name, Camelot or Open Key). */
  key?: string;
  /** 0 when unknown. */
  bpm: number;
  year: number;
  duration_secs: number;
  /** The audio file's path in the library (`/Contents/...`), to find the track again. */
  file?: string | null;
  /** Seconds from the start of the set (from a recording or a timed log). */
  start_seconds?: number | null;
  /**
   * MusicBrainz IDs from the audio file's tags, else from what the analyzers chosen for their
   * roles worked out for it, read from a mounted drive when the set is made, edited or published.
   */
  musicbrainz?: MusicBrainzIds;
  /** Published without naming it: only its place in the set. */
  unnamed?: boolean;
  origin: EntryOrigin;
  /** The session that logged it. */
  session?: string | null;
};

/**
 * MusicBrainz IDs, as MusicBrainz Picard tags them: `MUSICBRAINZ_TRACKID` (ID3's `UFID`) is the
 * recording, `MUSICBRAINZ_ALBUMID` the release, `MUSICBRAINZ_ARTISTID` each credited artist. An
 * analyzer answers them as the roles `recording`, `release` and `artists`.
 */
export type MusicBrainzIds = {
  recording?: string | null;
  release?: string | null;
  /** In the credit's order. */
  artists?: string[];
};

/** Where a set's track came from. */
export type EntryOrigin =
  /** A session logged it. */
  | "session"
  /** It was heard in the recording. */
  | "recording"
  /** The DJ added it. */
  | "added";

/**
 * A set as it was saved, and what's worth telling the DJ about it (tentative matches, tracks
 * logged but not heard in the recording, a day to check, how inserted tracks were found).
 */
export type SetChange = {
  set: Set;
  notes: string[];
};

export type SetExportReport = {
  format: SetFormat;
  /** The document, when it wasn't written to a file. */
  text?: string | null;
  path?: string | null;
  /** Tracks written. */
  tracks: number;
  /** Rules of the format the set breaks, and tracks left out. */
  warnings: string[];
};

/** A track's place in a set. */
export type Appearance = {
  /** The set's id. */
  set: string;
  date: string;
  title?: string | null;
  venue?: string | null;
  /** From 1. */
  position: number;
  /** Seconds from the start of the set, when known. */
  start_seconds?: number | null;
  /** The track as the set has it: `Artist - Title`. */
  track: string;
};

/** The sets the sessions suggest, and the sessions they're made of. */
export type Suggestions = {
  /** Newest first. */
  candidates: Candidate[];
  /** The candidates' sessions, newest first. */
  sessions: Session[];
};

/** Sessions that look like one set. */
export type Candidate = {
  /** Their ids, newest first. */
  sessions: string[];
  /**
   * When it was played: the imported logs' times when there are some, else the time every
   * session's window shares.
   */
  window: Window;
  /** Different tracks the sessions logged. */
  tracks: number;
  /**
   * How much it looks like a set, 0 to 100: up to 40 for its tracks, 20 for a drive taken to
   * sets, 30 for a recording that has its tracks, 10 for several drives.
   */
  score: number;
  /** The evidence, in words. */
  reasons: string[];
};

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

/**
 * A session: one run of play logging. A history list a player recorded, or a log imported from
 * another tool ([`ImportedSession`]).
 */
export type Session = {
  /**
   * The list's fingerprint ([`crate::plays::HistoryList::fingerprint`]): the same wherever
   * it's read. Commands take any unique prefix of 4 or more characters.
   */
  id: string;
  name: string;
  /** Folders and name. */
  path: string;
  /**
   * `device_library` (`export.pdb`), `onelibrary`, or the tool a log was imported from
   * (`beat_link_trigger`, or a plugin's importer, `<plugin>.<importer>`).
   */
  source: string;
  /** The mounted drives holding it now. */
  drives: string[];
  /** The drive whose snapshot first held it. */
  first_seen_on?: string | null;
  window: Window;
  /** In play order; `start_seconds` is set for timed logs. */
  tracks: SetEntry[];
  /** Entries whose track isn't in the library any more. */
  missing: number;
  /** Sets built from it, by id. */
  sets: string[];
  /** The DJ said it wasn't a set. */
  dismissed: boolean;
};

/** Every session, newest first, and what couldn't be read. */
export type Gathered = {
  sessions: Session[];
  warnings: string[];
};

/** A play log imported from another tool, with real play times. */
export type ImportedSession = {
  /** Like a history list's fingerprint: the same log imported twice is one session. */
  id: string;
  name: string;
  /** `beat_link_trigger`, or the plugin's importer it was read with (`<plugin>.<importer>`). */
  tool: string;
  /** The file it was imported from. */
  file: string;
  imported_at: string;
  plays: TimedPlay[];
};

/** One play in an imported log. */
export type TimedPlay = {
  entry: SetEntry;
  started: string;
  stopped?: string | null;
  /** The player it was played on (`3`), when the log says. */
  player?: string | null;
};

export type IndexReport = {
  /** Tracks in the library. */
  tracks: number;
  /** Tracks in the index. */
  indexed: number;
  /** Fingerprinted just now (the rest came from the cache). */
  fingerprinted: number;
  /** Audio files missing from the drive. */
  missing: TrackRef[];
  /** Audio files that couldn't be decoded. */
  failed: TrackFailure[];
  /** Tracks over 20 minutes ([`LONGEST_TRACK_SECS`]), left out: recorded sets, not tracks. */
  long: TrackRef[];
  hashes: number;
};

/** A mix's report with what a front end needs to show it and follow along. */
export type IdentifiedMix = {
  mix: MixInfo;
  drive: string;
  /** The drive's identity, to find it again by (absent for a drive locallibrary never adopted). */
  drive_id?: string | null;
  library: IndexReport;
  plays: Play[];
  transitions: Transition[];
  unidentified: Span[];
  /** Fraction of the mix where a track was identified. */
  coverage: number;
  waveform: MixWaveform;
  tempo_range: number;
  analysed_at: string;
  elapsed_secs: number;
  /**
   * Audio a browser can play for the mix: the recording, or for an Audacity project an export
   * next to it (the file is there; `null` when there's none).
   */
  audio: string | null;
  /**
   * Each play's overview waveform, hot cues and phrases from the drive, in the order of
   * `plays` (asked for with `extras`; empty when the drive isn't mounted).
   */
  extras?: PlayExtras[];
  /** The id the report is kept under in the store's reports (`mix.reports.get`), when saved. */
  saved?: string | null;
};

/** What was analysed. */
export type MixInfo = {
  path: string;
  /** `audio` or `audacity`. */
  kind: string;
  title: string;
  artist: string | null;
  genre: string | null;
  date: string | null;
  /** The device that recorded it, when the file says (Pioneer DJ's recorders tag their model). */
  recorder: string | null;
  duration_secs: number;
  sample_rate: number;
  channels: number;
  /** Labels from an Audacity project's label tracks. */
  labels: Label[];
  /** Cover image: embedded in the audio file, else an image next to it named like it. */
  cover: CoverSource | null;
};

/** A label from a label track (DJs sometimes mark where tracks change). */
export type Label = {
  start: number;
  end: number;
  title: string;
};

export type CoverSource = {
  kind: "embedded";
  path: string;
} | {
  kind: "file";
  path: string;
};

/** One track heard in the mix. */
export type Play = {
  track: Track;
  /** Seconds into the mix where the track is first and last heard. */
  start: number;
  end: number;
  /** Seconds into the track at those points. */
  track_start: number;
  track_end: number;
  /** Playback speed: 1.0 as recorded, 1.03 is +3%. */
  tempo: number;
  /** The track's BPM at that speed. */
  played_bpm: number | null;
  /** Whether key lock (master tempo) was on: `None` when the speed is too close to 1.0 to tell. */
  key_lock: boolean | null;
  /** Pitch change from the speed, in semitones (0 with key lock). */
  semitones: number;
  /** The track's key in Camelot notation (the library may use note names or Open Key). */
  key: string | null;
  /** The key heard: the track's key moved by `semitones`, in Camelot notation. */
  played_key: string | null;
  /** 0-1. */
  confidence: number;
  votes: number;
  /**
   * How clearly the track is heard, every [`matcher::PRESENCE_STEP`] seconds from `start` (0-1).
   * Fades and EQ swaps show up as slopes.
   */
  presence: number[];
  presence_step: number;
  /** Loops and cue jumps inside the play. */
  jumps: number;
  /** Other library tracks matching the same stretch of the mix. */
  alternates: Alternate[];
};

export type Alternate = {
  track_id: number;
  name: string;
  file_path: string;
  /** The same recording (a duplicate or re-encode) rather than one sharing some material. */
  same_audio: boolean;
};

/** The change from one play to the next. */
export type Transition = {
  /** Indexes into [`MixReport::plays`]. */
  from: number;
  to: number;
  /** When the incoming track is first heard. */
  at: number;
  /** Seconds both are heard together (negative: a gap between them). */
  overlap: number;
  /** `same key`, `±1`, `relative`, `energy boost`, or `clash`, from the played keys. */
  harmony: string | null;
  /** Played tempo change, percent. */
  tempo_change: number | null;
};

/** Stretch of the mix with no track found. */
export type Span = {
  start: number;
  end: number;
};

/** The mix's loudness for drawing it: per column, 0-255. */
export type MixWaveform = {
  columns_per_sec: number;
  peak: number[];
  /** Below 200 Hz, 200 Hz-2 kHz, above 2 kHz. */
  low: number[];
  mid: number[];
  high: number[];
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
  /**
   * The track's overview waveform: up to 400 columns of four bytes (height 0-255, then red,
   * green and blue), in base64.
   */
  wave: string | null;
  /** Its phrases, in seconds into the track. */
  phrases: PhraseSpan[];
  /** Its hot cues, in seconds into the track. */
  cues: CueMark[];
  /** What analyzers worked out for it that a page shows (the HTML report's), in order. */
  fields?: ShownField[];
};

export type PhraseSpan = {
  start: number;
  end: number;
  /** `Intro`, `Up`, `Chorus`... */
  kind: string;
};

export type CueMark = {
  /** Seconds into the track. */
  t: number;
  /** The hot cue's letter and name: `A`, `B · Drop` (the HTML report gives only the letter). */
  label: string;
  /** `#rrggbb`. */
  color: string | null;
};

/** An analyzer's field, as a page shows it. */
export type ShownField = {
  /** `<plugin>.<field>` (`sound.lufs`). */
  label: string;
  /** As its analyzer says it's shown (`-8.2 LUFS`). */
  value: string;
};

export type MixExport = {
  format: string;
  version: number;
  generator: Generator;
  /** RFC 3339. */
  analysedAt: string;
  mix: ExportedMix;
  /** Playback speeds searched, as a fraction either way (0.16 is ±16%). */
  tempoRange: number;
  /** Fraction of the mix where a track was identified, 0-1. */
  coverage: number;
  waveform: ExportWaveform;
  plays: ExportPlay[];
  transitions: ExportTransition[];
  /** Stretches where no track was identified. */
  unidentified: ExportSpan[];
};

export type Generator = {
  name: string;
  version: string;
};

export type ExportedMix = {
  title: string;
  artist?: string | null;
  genre?: string | null;
  date?: string | null;
  /** The player that recorded it, when the file says. */
  recorder?: string | null;
  durationSecs: number;
  /** The cover embedded in the recording: a JPEG `data:` URL, 320 px square. */
  cover?: string | null;
};

/** The mix's loudness per column, 0-255, in three bands. */
export type ExportWaveform = {
  secondsPerColumn: number;
  /** Below 200 Hz. */
  low: number[];
  /** 200 Hz to 2 kHz. */
  mid: number[];
  /** Above 2 kHz. */
  high: number[];
};

export type ExportPlay = {
  track: ExportTrack;
  /** Seconds into the mix where the track is first and last heard. */
  start: number;
  end: number;
  /** Seconds into the track at those points. */
  trackStart: number;
  trackEnd: number;
  /** Playback speed: 1.03 is +3%. */
  tempo: number;
  playedBpm?: number | null;
  /** `None` when the speed is too close to 1.0 to tell. */
  keyLock: boolean | null;
  semitones: number;
  /** Camelot notation (`8A`). */
  key?: string | null;
  playedKey?: string | null;
  /** 0-1. */
  confidence: number;
  /** How clearly the track is heard, every `presenceStep` seconds from `start`, 0-100. */
  presence: number[];
  presenceStep: number;
  /** Loops and cue jumps. */
  jumps: number;
};

/** A track: "ID" for its artist and title (and no more that names it) when the DJ hides it. */
export type ExportTrack = {
  title: string;
  artist?: string | null;
  remixer?: string | null;
  album?: string | null;
  label?: string | null;
  genre?: string | null;
  year?: number | null;
  /** As the library stores it. */
  key?: string | null;
  /** As recorded. */
  bpm?: number | null;
  durationSecs: number;
  /** JPEG `data:` URL, 96 px square. */
  artwork?: string | null;
  /** rekordbox's overview waveform: heights 0-255 and `#rrggbb` colours, one per column. */
  overview?: Overview | null;
  cues?: ExportCue[];
  phrases?: ExportPhrase[];
};

export type Overview = {
  heights: number[];
  colors: string[];
};

/** A hot cue (A-H), seconds into the track. */
export type ExportCue = {
  at: number;
  label: string;
  color?: string | null;
};

/** A phrase from rekordbox's analysis (Intro, Up, Down, Chorus, Verse, Bridge, Outro). */
export type ExportPhrase = {
  start: number;
  end: number;
  kind: string;
};

export type ExportTransition = {
  /** Indexes into `plays`. */
  from: number;
  to: number;
  at: number;
  /** Seconds both are heard together (negative: a gap). */
  overlap: number;
  /** `same key`, `±1`, `relative`, `energy boost` or `clash`. */
  harmony?: string | null;
  /** Played tempo change, percent. */
  tempoChange?: number | null;
};

export type ExportSpan = {
  start: number;
  end: number;
};

/** A report kept in the store's `reports` folder. */
export type SavedReport = {
  /**
   * Names it in `mix.reports.get` and `mix.reports.html`: its files' name, the title's slug
   * and a hash of the recording's path (see [`id`]).
   */
  id: string;
  /** The mix's title (from its file name when only the HTML page is there). */
  title: string;
  artist: string | null;
  /** The recording it was made from. */
  mix: string | null;
  /** The drive whose library it was matched against. */
  drive: string | null;
  duration_secs: number | null;
  /** Tracks found. */
  tracks: number | null;
  /** Fraction of the mix identified. */
  coverage: number | null;
  analysed_at: string | null;
  /** The report as data (`<id>.json`): `mix.reports.get` reads it. */
  report: string | null;
  /** The interactive page (`<id>.html`). */
  html: string | null;
  /** The portable export (`<id>.mix.json`), for Local Network. */
  export: string | null;
  /** When one of its files last changed. */
  modified: string;
};

/** Where a report's interactive page was written. */
export type ReportPage = {
  id: string;
  /** `<id>.html` in the store's reports folder: open it in a browser or a webview. */
  path: string;
  bytes: number;
  /** The page, when asked for with `inline`. */
  html?: string | null;
};

export type FleetStatus = {
  mounted: FleetDrive[];
  /** Registered drives that aren't mounted (with `all`). */
  offline: KnownDrive[];
  /** Mounted drives sharing one identity (copies of each other). */
  duplicates: Duplicate[];
  /**
   * The libraries of the mounted drives (with `all`, of every registered drive too), by name:
   * each with its clones. Drives of no library in the store are in none.
   */
  libraries: FleetLibrary[];
};

/** A mounted drive's status, with what the registry knows about it. */
export type FleetDrive = {
  drive: DeviceInfo;
  library: string | null;
  /**
   * The library head (left) vs. the drive (right): `identical`, `left_newer` (the drive is
   * behind: push), `right_newer` (the drive has newer changes: pull them), `diverged`... `None`
   * if not linked to a library.
   */
  relation: Relation | null;
  head: SnapshotInfo | null;
  /**
   * Back from a set: the drive is newer than the head only by what players add (history
   * lists, TAG LIST playlists), with this many history lists new or longer. Pulling saves them.
   */
  back_from_set: number | null;
  /** Settings that differ from the profile. */
  settings_drift: number;
  error: string | null;
  role: Role | null;
  /** Write-protected: players can't save history or cues to it. */
  read_only: boolean;
  /** Less free space than players need for history and cues. */
  low_space: boolean;
  log: DriveLog | null;
  /** Other mounted drives claiming the same identity (copies of this one, or this a copy). */
  same_identity_as?: string[];
  /**
   * Recorded edits a rekordbox export replaced (see [`super::exported_over_edits`]): 0, or how
   * many `locallibrary refresh` (or `edits replay`) makes again.
   */
  exported_over_edits: number;
};

/** A drive's logbook, kept in the store's registry. */
export type DriveLog = {
  first_seen?: string | null;
  /** Bytes written to it by sync, push and repair. */
  bytes_written: number;
  /** Operations that wrote to it. */
  writes: number;
  clean_ejects: number;
  last_scrub?: CheckEntry | null;
  last_fs_check?: CheckEntry | null;
  /** Newest last, at most 50 (the oldest are dropped). */
  problems?: FleetProblem[];
  /** No longer in use: hidden from listings, refused as a sync target. */
  retired?: string | null;
};

export type CheckEntry = {
  at: string;
  ok: boolean;
  summary: string;
};

export type FleetProblem = {
  at: string;
  /** `damaged_files`, `unreadable_files`, `missing_files`, `repair_failed`, `fs_errors`. */
  kind: string;
  detail: string;
};

/** A drive from the registry. */
export type KnownDrive = {
  id: string;
  name: string;
  role: Role | null;
  library: string | null;
  /** Where it is mounted now, if it is. */
  mounted: string | null;
  last_mount: string;
  last_seen: string;
  capacity: number | null;
  standing: Standing;
  log: DriveLog;
};

/** Where a drive stood against its library's latest snapshot when it was last seen. */
export type Standing =
  /** It held the latest snapshot. */
  | {
    standing: "up_to_date";
  }
  /**
   * It held an older state: `snapshots` newer ones have been saved since. `held` is the
   * snapshot holding it, none when the library's latest went through it unsaved.
   */
  | {
    standing: "behind";
    snapshots: number;
    held: string | null;
  }
  /**
   * It held a state the store never saved, which the library's latest never went through (a
   * later export or edit that wasn't pulled).
   */
  | {
    standing: "unsaved";
  }
  /** Not linked to a library, or the library has no snapshots. */
  | {
    standing: "unknown";
  };

/** Mounted drives sharing one identity. */
export type Duplicate = {
  id: string;
  name: string;
  roots: string[];
  /** The one on the disk the identity was given on, when the hardware says. */
  original: string | null;
};

/** A library and its clones, the drives holding it: those plugged in, and those that aren't. */
export type FleetLibrary = {
  id: string;
  name: string;
  /** Its latest snapshot. */
  head: SnapshotInfo | null;
  /**
   * Where its clones that are plugged in are mounted (each is in `mounted`), by role: the
   * primary first.
   */
  mounted: string[];
  /** Its registered clones that aren't plugged in (retired ones with `retired`). */
  offline: KnownDrive[];
  /**
   * Whether its clones plugged in hold the same library: `None` with fewer than two, or when
   * one couldn't be compared with the store.
   */
  identical: boolean | null;
  /**
   * Whether a clone plugged in holds changes the store doesn't have (it's newer than the latest
   * snapshot, or diverged from it).
   */
  unsaved: boolean;
};

/** What renaming a drive would do. */
export type RenamePlan = {
  drive: string;
  root: string;
  from: string;
  to: string;
  /** The volume label it gets, with `label`. */
  label: string | null;
};

export type RenameReport = {
  id: string;
  old_name: string;
  name: string;
  /** The volume label, when it was changed too. */
  label: string | null;
};

export type DriveRecord = {
  id: string;
  name: string;
  library_id: string | null;
  last_mount: string;
  last_seen: string;
  last_fingerprint: Hash | null;
  /** What the drive is for (see [`crate::fleet`]). */
  role?: Role | null;
  /** The disk it was last seen on: hints for spotting copies and drives erased elsewhere. */
  hardware?: Hardware | null;
  /** Size of its volume when last seen. */
  capacity?: number | null;
  log: DriveLog;
};

/** The disk a drive was seen on. */
export type Hardware = {
  model: string;
  serial?: string | null;
  /** The file system's identifier. FAT and exFAT give every format a new one. */
  volume_uuid?: string | null;
  /** Whole-disk size in bytes. */
  size: number;
};

/** What an unadopted (or re-identified) drive matches. */
export type Recognition = {
  /** Its library state is a stored snapshot. */
  snapshot: SnapshotMatch | null;
  /** A registered drive last seen holding exactly this state. */
  drive_by_content: RegisteredRef | null;
  /** A registered drive last seen on this disk (same serial number). */
  drive_by_hardware: RegisteredRef | null;
};

export type SnapshotMatch = {
  library: string;
  snapshot: SnapshotInfo;
};

export type RegisteredRef = {
  id: string;
  name: string;
  last_seen: string;
};

/** What giving a drive back a known drive's identity would do. */
export type AdoptAsPlan = {
  drive: string;
  root: string;
  /** The registered drive whose identity it takes. */
  known: DriveRecord;
};

/** What giving a copied drive an identity of its own would do. */
export type ReidentifyPlan = {
  drive: string;
  root: string;
  /** The identity it shares with the drive it was copied from, which keeps it. */
  id: string;
  /** The name it gets. */
  name: string;
};

/**
 * A drive group: the mounted drives that share a library and can be synced together (not
 * archive or retired), plus the library's primary if it's mounted.
 */
export type DriveSet = {
  library_id: string | null;
  library: string | null;
  primary: string | null;
  members: string[];
  /** Mounted members left out: archive or retired. */
  excluded: string[];
};

export type EjectPlan = {
  drives: EjectTarget[];
};

/** A drive ejecting would unmount and power off. */
export type EjectTarget = {
  drive: string;
  root: string;
  /** The other volumes mounted from its disk: ejecting unmounts them too. */
  also_unmounts: string[];
  /** The commands that eject it. */
  commands: string[];
};

export type EjectReport = {
  name: string;
  root: string;
  /** What was run. */
  commands: string[];
  /** Whether the disk was also powered off (otherwise only unmounted). */
  powered_off: boolean;
};

export type ScrubReport = {
  drive: string;
  root: string;
  /** The snapshot of exactly this state, when there is one. */
  snapshot: Hash | null;
  files: number;
  bytes: number;
  /** Files compared with a reference and found intact. */
  verified: number;
  /**
   * Files read without error but with no reference (changed since locallibrary last saw
   * them, or never seen).
   */
  unverified: number;
  damaged: FileProblemOutput[];
  unreadable: FileProblemOutput[];
  missing: FileProblemOutput[];
  /** Tracks whose audio, analysis or artwork is affected. */
  tracks: TrackRef[];
  playlists: string[];
};

/** Which players a packed drive's copies are for, and how they're made. */
export type TranscodeOutput = {
  /** Player models (see `players models`); none: the ones remembered for the source's library. */
  players: string[];
  /** The container copies are written in: AIFF unless given. */
  container: Container;
  /**
   * Also decode lossy files the players can't play (an MP3 at 22.05 kHz) into lossless copies:
   * bigger, but no further loss. Lossy files are never encoded lossy again.
   */
  decode_lossy: boolean;
};

export type RepairReport = {
  drive: string;
  repaired: string[];
  bytes: number;
  /** Files that still differ after being rewritten: the flash itself is failing. */
  failed: Failure[];
  /** Files that were fine by the time of the repair, or had no good copy. */
  skipped: Skip[];
};

export type FsCheckReport = {
  drive: string;
  root: string;
  /** The volume's device (`disk4s1`, `/dev/sdb1`, `E:`). */
  volume: string | null;
  filesystem: string | null;
  command: string;
  /** Whether the file system is consistent (after the repair, when repairing). */
  ok: boolean;
  repaired: boolean;
  /** The tool's report, last lines. */
  output: string[];
  /** What to be wary of: the check hasn't run on this platform before. */
  warnings: string[];
};

export type SpaceReport = {
  drive: string;
  root: string;
  total: number | null;
  free: number | null;
  cluster_size: number;
  /** Largest first. */
  categories: SpaceCategory[];
  /** Largest first, at most 50. */
  loose_audio: LooseFile[];
  /** Files that couldn't be measured (no permission). */
  unreadable: number;
  forecast: Forecast | null;
  /** Whether this drive's library fits on the other drives of its library. */
  fits: DriveFit[];
};

export type SpaceCategory = {
  kind: SpaceKind;
  files: number;
  bytes: number;
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

export type LooseFile = {
  path: string;
  size: number;
};

export type Forecast = {
  library: string;
  /** Library size in the newest snapshot. */
  library_bytes: number;
  /** Average growth, bytes per 30 days (negative when it shrinks). */
  per_month: number;
  snapshots: number;
  span_days: number;
  /** When the drive runs out of space at that rate (counting the players' headroom). */
  full_around: string | null;
};

export type DriveFit = {
  name: string;
  capacity: number;
  /** The library plus the players' headroom. */
  needed: number;
  fits: boolean;
  mounted: boolean;
};

/** What putting a contact on a drive would write. */
export type ContactPlan = {
  drive: string;
  root: string;
  file: string;
  /** The drive has one already, which is replaced. */
  replaces: boolean;
  /** The playlist made, with `playlist`. */
  playlist: string | null;
};

export type ContactReport = {
  file: string;
  playlist: string | null;
};

export type PlayerIdentity = {
  drive: string;
  /** From OneLibrary (`None`: the drive has no readable OneLibrary database). */
  onelibrary: DbIdentity | null;
  /** rekordbox's own record, if rekordbox set the drive up. */
  rekordbox: RekordboxDevice | null;
};

/** rekordbox's record of the device. */
export type RekordboxDevice = {
  device_id: string;
  device_name: string;
  background_color: string;
  background_color_libplus: string;
  device_filesystem: string;
  backup_pc_name: string;
  backup_date: string;
};

/** What setting the name and colour players show would change. */
export type PlayerPlan = {
  drive: string;
  root: string;
  /** What players show now. */
  now: PlayerIdentity;
  name: string | null;
  colour: number | null;
};

export type TidyPlan = {
  drive: string;
  root: string;
  items: TidyItem[];
  bytes: number;
  /** Space in the trash that `empty_trash` would free. */
  trash_bytes: number;
};

export type TidyItem = {
  /** Relative to the drive root. */
  path: string;
  kind: TidyKind;
  is_dir: boolean;
  files: number;
  bytes: number;
};

export type TidyKind = "apple_double" | "ds_store" | "thumbnails" | "temporary_items" | "spotlight" | "fs_events" | "trash" | "temp_files";

export type TidyReport = {
  removed: number;
  bytes: number;
  failed: Failure[];
};

export type Recording = {
  /** Relative to the drive root. */
  path: string;
  size: number;
  format: WavFormat | null;
  /**
   * Length of the audio, from the WAV header (or the file size when recording was cut off
   * before the header was finalised).
   */
  duration_secs: number | null;
  /** When the file was last written: the end of the recording. */
  modified: string;
  started: string | null;
};

export type WavFormat = {
  channels: number;
  sample_rate: number;
  bits: number;
};

/** What collecting recordings would copy (and, with `delete`, delete from the drive). */
export type CollectPlan = {
  drive: string;
  root: string;
  to: string;
  recordings: Recording[];
  bytes: number;
  /** Each is deleted from the drive once its copy is verified. */
  delete: boolean;
};

export type CollectReport = {
  collected: Collected[];
  bytes: number;
  failed: Failure[];
};

export type Collected = {
  recording: string;
  copied_to: string;
  tracklist: string | null;
  deleted: boolean;
};

export type FirmwarePlan = {
  drive: string;
  root: string;
  file: string;
  file_name: string;
  size: number;
  model: FirmwareModel | null;
  /** Update files already in the root; removed with `replace`. */
  existing: string[];
  /**
   * What stops the device from taking the update. Applying refuses while there are any
   * (unless forced).
   */
  problems: string[];
  notes: string[];
};

/** A device's update rules. */
export type FirmwareModel = {
  model: string;
  /** The update file's name starts with this (any case) and ends in `.UPD`. */
  prefix: string;
  filesystems: FsKind[];
  /** Only one update file may be on the drive. */
  single_file: boolean;
  /** Which socket to use, where the guide says. */
  port: string | null;
};

export type FirmwareReport = {
  written: string;
  removed: string[];
};

/** The update files removing them would delete from a drive's root. */
export type FirmwareRemovePlan = {
  drive: string;
  root: string;
  files: string[];
  bytes: number;
};

export type ProvisionPlan = {
  steps: string[];
  /** The format's plan, when formatting (its `identity` confirms the disk on apply). */
  format: FormatPlan | null;
  warnings: string[];
};

export type ProvisionReport = {
  format: FormatReport | null;
  drive: DeviceInfo | null;
  copy: SyncReport | null;
  settings: ApplyReport | null;
  player: PlayerIdentity | null;
  verify: ScrubReport | null;
  eject: EjectReport | null;
  /** Where it stopped, when a step failed (the steps before it stay done). */
  stopped: StepError | null;
};

export type StepError = {
  step: string;
  kind: string;
  message: string;
};

/** A job's plan: its steps, in the order they run (see the module docs). */
export type JobPlan = {
  /** The drives it's about. */
  targets: JobDrive[];
  /** Every step, including the ones that won't run (`runs: false`, with the reason). */
  steps: JobStep[];
};

/** A drive a job is about. */
export type JobDrive = {
  name: string;
  root: string;
};

/** One step of a job's plan. */
export type JobStep = {
  step: StepKind;
  /** What it does, in a few words. */
  title: string;
  /** Whether it runs when the plan is applied; when it doesn't, `reason` says why. */
  runs: boolean;
  reason?: string | null;
  /**
   * What it will change, in a line each; the pull step also names the drives the store holds
   * already, which it leaves alone. A step after one that writes to the same drive is planned
   * on the drive as it is now, and planned again when it runs.
   */
  summary: string[];
  warnings: string[];
  /** It deletes or overwrites something. */
  destructive: boolean;
  /** The plans of the operations it runs, as their `*.plan` methods return them. */
  plans?: unknown[];
};

/** What applying a job's plan did. */
export type JobReport = {
  /** The steps that ran, in order (a step that failed part of the way is here with what it did). */
  steps: StepReport[];
  /** The step that failed, which stopped the job. The steps before it stay done. */
  stopped?: StepError | null;
  /** The check found problems, so the job stopped before ejecting the drives. */
  problems: boolean;
  /** The check's report, when it ran. */
  check?: CheckReport | null;
};

/** What one step did. */
export type StepReport = {
  step: StepKind;
  /** What it did, in a line each. */
  done: string[];
  /** Warnings that came up as it ran (the plan's own aside). */
  warnings: string[];
  /** Each operation's report, as its `*.apply` method returns it. */
  reports: unknown[];
};

/** What `master.db` holds, in numbers. */
export type CollectionInfo = {
  path: string;
  /** Size of `master.db`, and of its WAL when there is one. */
  bytes: number;
  wal_bytes: number | null;
  db_version: string | null;
  summary: LibrarySummary;
  history_lists: number;
  /** Paths of intelligent playlists (not evaluated, so listed empty). */
  smart_playlists: string[];
  hot_cues: number;
  memory_cues: number;
  loops: number;
  tag_categories: number;
  tags: number;
  tagged_tracks: number;
};

/**
 * Everything `collection.library` returns: counts, the library (tracks, playlists, history), cues
 * by track and My Tags.
 */
export type CollectionView = {
  info: CollectionInfo;
  library: Library;
  cues: Record<string, never>;
  tags: TagCategory[];
};

export type TagCategory = {
  name: string;
  tags: TagEntry[];
};

export type TagEntry = {
  name: string;
  track_ids: number[];
};

/**
 * [`Collection::diff_drive`]: `diff.added_*` is what the drive lacks, `diff.removed_*` what only
 * the drive has, and changed tracks are details the collection has since changed (or the drive
 * was edited).
 */
export type DriveDiff = {
  /** Collection tracks found on the drive by their file. */
  matched_by_file: number;
  /** Found by ISRC, or artist, title and length. */
  matched_by_details: number;
  /**
   * Collection tracks only a loose match (title alone, or similar words) finds on the drive;
   * counted as missing from it.
   */
  uncertain: TrackRef[];
  diff: PlaylistDiff;
};

export type CollectionExportPlan = {
  /** The `master.db` exported (empty for a plugin's library source). */
  collection: string;
  /** The plugin's library source written instead, `<plugin>:<source>`. */
  source?: string | null;
  target: string;
  target_root: string;
  /** The playlists chosen; empty for the whole collection. */
  playlists: string[];
  tracks: number;
  /** Playlists exported (not counting folders). */
  playlist_count: number;
  /** Size of the exported library's files. */
  library_bytes: number;
  copies: FileCopy[];
  copy_bytes: number;
  /** Files on the target the exported library doesn't use. */
  deletes: FileCopy[];
  delete_bytes: number;
  unchanged: number;
  /**
   * Audio files to copy that are online only in Dropbox or Google Drive: copying downloads
   * them.
   */
  downloads: number;
  download_bytes: number;
  /** Tracks left out because their audio isn't on this computer ("Artist - Title"). */
  missing: string[];
  /**
   * Tracks whose analysis or artwork Cloud Library Sync hasn't finished bringing to this
   * computer.
   */
  syncing: string[];
  /**
   * Tracks with an analysis file that can't be read, or (from a plugin's source) whose cues,
   * beat grid or waveform can't be written: exported without it.
   */
  unreadable: string[];
  /**
   * The target holds a library that wasn't exported from the collection, or files the export
   * removes though it holds no library.
   */
  replaces_library: boolean;
  /**
   * The folder exported to was made for this export (see
   * [`crate::api::collection_export_plan`]): a caller that doesn't apply the plan takes it away.
   */
  made_folder: boolean;
  /**
   * What the target holds that the export replaces and only it has (history lists players
   * recorded, changes since the last export): saved in the store first.
   */
  unsaved: Unsaved | null;
  /**
   * Space the export needs on the target: the files it copies, its new databases and the
   * backup of what it replaces.
   */
  needed_bytes: number;
  free_space: number | null;
  fits: boolean;
  warnings: string[];
};

export type CollectionExportReport = {
  tracks: number;
  playlists: number;
  copied: number;
  copied_bytes: number;
  deleted: number;
  deleted_bytes: number;
  /** OneLibrary cue rows made from the analysis files. */
  cues: number;
  /** Tracks with My Tags. */
  tagged_tracks: number;
  fingerprint: Hash | null;
  /**
   * The target's state, saved in the store before the export (see
   * [`CollectionExportPlan::unsaved`]).
   */
  saved: SnapshotInfo | null;
  /** The backup of what the export replaced (its databases, analysis and artwork). */
  backup: string | null;
};

/** A plugin, as `plugin list` and the sidecar's `plugins.list` show it. */
export type Listed = {
  id: string;
  name: string;
  version: string;
  capabilities: Granted[];
  /**
   * What it adds: its accounts by id, its commands, destinations, operations, panels,
   * analyzers, checks, providers, services, library sources and themes by name, the events it
   * listens for, its export and import formats and its settings' keys.
   */
  accounts: string[];
  commands: string[];
  destinations: string[];
  events: string[];
  operations: string[];
  panels: string[];
  analyzers: string[];
  checks: string[];
  exporters: string[];
  importers: string[];
  providers: string[];
  services: string[];
  sources: string[];
  themes: string[];
  settings: string[];
  /** It's running now: plugins start when they're first used, and stop once they're idle. */
  running: boolean;
  /**
   * Why it's left out, if it is: what it clashes with. Then nothing it adds can be used. Only
   * the sidecar lists these: the command line says why as it starts.
   */
  left_out: string | null;
};

/** A capability a plugin has, and what it lets the plugin do. */
export type Granted = {
  capability: Capability;
  /** In plain words. */
  means: string;
};

export type Capability = string;

/**
 * A plugin in full, as `plugin info` and the sidecar's `plugins.describe` show it: its manifest
 * (with the JSON Schemas of its params and options, to draw forms from), what its capabilities
 * let it do, its settings now, and whether each of its accounts is signed in.
 */
export type Described = {
  manifest: Manifest;
  capabilities: Granted[];
  settings: { [key: string]: unknown };
  /**
   * Whether each secret setting is set, never what it is; `null` for each when the keychain
   * can't say (see `secrets_unknown`).
   */
  secrets: { [key: string]: boolean | null };
  /** Why the keychain can't say, when it can't: the rest is answered all the same. */
  secrets_unknown?: string | null;
  /**
   * Each of its accounts, and whether it's signed in (never a token): none signed in when the
   * keychain can't say (see `secrets_unknown`).
   */
  accounts: PluginAccountStatus[];
  running: boolean;
};

/**
 * Read tolerantly: a key this side doesn't know is left out, and [`Manifest::validate`] refuses
 * it only in a plugin locallibrary ships (see [`Manifest::unknown`]).
 */
export type Manifest = {
  /** Lowercase letters, digits, `-` and `_`, starting with a letter. */
  id: string;
  name: string;
  /** The plugin's version (semver). */
  version: string;
  /** The protocol versions it speaks, as a semver requirement (`^0.2`). */
  protocol: string;
  /** What it may call back for (see [`Capability`]). */
  capabilities: Capability[];
  contributes: Contributions;
  /** How often the host may send `net/fetch` requests to some of the hosts it talks to. */
  net?: NetLimit[];
};

export type Contributions = {
  accounts: Account[];
  commands: Command[];
  destinations: Destination[];
  events: Listener[];
  operations: Operation[];
  panels: Panel[];
  analyzers: Analyzer[];
  checks: Check[];
  exporters: Exporter[];
  importers: Importer[];
  providers: Provider[];
  services: Service[];
  sources: LibrarySource[];
  themes: Theme[];
  settings: Setting[];
};

/** `[[contributes.accounts]]`: an account on a service, signed in to in the browser. */
export type Account = {
  /** How requests name it (`{account:<id>}`). */
  id: string;
  title: string;
  /** The service's authorization endpoint, where the browser signs in (https). */
  authorize_url: string;
  /**
   * Its token endpoint, where the host trades the browser's code for tokens and refreshes
   * them (https). An account has this or `token_exchange`.
   */
  token_url?: string | null;
  /**
   * Local Network's token exchange for its service
   * (`https://localnetwork.computer/api/token-exchange/<service>`), in `token_url`'s place, for
   * a service that wants a client secret there.
   */
  token_exchange?: string | null;
  /** What it asks the service for, as the service names them. */
  scopes?: string[];
  /**
   * The plugin's client at the service: a public one, without a secret, or Local Network's,
   * whose secret its token exchange holds.
   */
  client_id: string;
  /** Signing in uses PKCE: always, and a manifest saying otherwise is refused. */
  pkce: boolean;
  /** Where its tokens are revoked at signing out (RFC 7009), when the service has one (https). */
  revoke_url?: string | null;
  /** Where its access token goes in a request: `Authorization: Bearer <token>` unless it says. */
  token?: TokenPlace;
  /**
   * The port on 127.0.0.1 the browser comes back to, for a service that compares the redirect
   * URI with the one its client was registered with, port and all (SoundCloud): that's then
   * `http://127.0.0.1:<port>/callback`. Any free port otherwise.
   */
  redirect_port?: number | null;
};

/**
 * Where an account's access token goes in a `net/fetch` request: `token = { scheme = "OAuth" }`
 * or `token = { query = "access_token" }`. The request names the account there, and only there.
 */
export type TokenPlace =
  /** The `Authorization` header, as this scheme and the token (`OAuth <token>`). */
  | {
    scheme: string;
  }
  /**
   * This query parameter's value (`?access_token=<token>`), for a service that takes it only
   * there (Mixcloud).
   */
  | {
    query: string;
  };

/** Run with `command/run`. */
export type Command = {
  name: string;
  title: string;
  mode: Mode;
  /** Where hosts offer it (see [`Where`]). */
  where: Where[];
  /** A JSON Schema for its params (an object). */
  params?: unknown;
  /** Its own place on the command line (see [`Operation::cli`]). */
  cli?: string | null;
};

export type Mode =
  /** Answers any JSON, shown as it is (`--json`) or not at all. */
  | "headless"
  /** Answers a [`crate::View`]. */
  | "view";

/**
 * Where a host offers a command or an operation: in the menu of the tracks selected, of a
 * playlist, or of a drive. A host's palette has every one.
 */
export type Where = "track" | "playlist" | "drive";

/**
 * `[[contributes.destinations]]`: somewhere the user copies their store to (`store`), or
 * publishes sets at (`set`).
 */
export type Destination = {
  name: string;
  title: string;
  kind: DestinationKind;
};

/** What a destination keeps. */
export type DestinationKind =
  /** Copies of the store: `blobs/*` and `manifest/*`. */
  | "store"
  /** Sets, published: `set/publish`. */
  | "set";

/** `[[contributes.events]]`: what the plugin is told of (`event/notify`). */
export type Listener = {
  on: EventName[];
};

/**
 * What a host tells plugins of: a closed list, each needing a capability (see
 * [`EventName::capability`]).
 */
export type EventName =
  /** A drive was plugged in. */
  | "drive.mounted"
  /** A drive was unplugged. */
  | "drive.unmounted"
  /** A drive's databases or its locallibrary state changed (`payload.changed` says which). */
  | "drive.changed"
  /**
   * A drive was saved into the store as a new snapshot (`payload.history_lists`: the
   * history lists the store lacked).
   */
  | "drive.pulled"
  /** A drive back from a set was saved beside its library's newest snapshot. */
  | "set.returned"
  /** A request to the sidecar finished (`payload.method`, `payload.ok`). */
  | "job.finished"
  /** A plan was applied (`payload.operation`, `payload.summary`). */
  | "plan.applied";

/**
 * Planned with `op/plan`, applied by the host, which works out from the edits whether it's
 * destructive.
 */
export type Operation = {
  name: string;
  title: string;
  /** Where hosts offer it (see [`Where`]). */
  where: Where[];
  /** Planning takes a while: the host shows progress. */
  long_running: boolean;
  /** A JSON Schema for its params (an object). */
  params?: unknown;
  /**
   * Its own place on the command line, rather than `locallibrary <plugin> <name>`: a
   * top-level command (`import`), or one in a group of the host's (`playlist import`).
   * First-party plugins only.
   */
  cli?: string | null;
};

/**
 * Shown under a track's details, as `panel/render` answers for the track. Hosts ask for it as
 * the user comes to a track, so it answers quickly, reading only what it needs.
 */
export type Panel = {
  name: string;
  title: string;
  /** What it's shown with: a track, unless it says. */
  where: Where;
};

/** Run with `track/analyze`; its fields become columns. */
export type Analyzer = {
  name: string;
  title: string;
  /**
   * Its own version: the host keeps its values by it, so a new one works them out again. Bump
   * it whenever the values would change.
   */
  version: string;
  fields: { [key: string]: FieldType };
  /**
   * Which of its fields has each role it answers (`bpm = "bpm"`): what the host reads for
   * that role (see [`Role`]).
   */
  roles?: { [key: string]: string };
  /**
   * How hosts show some of its fields, by field (`display.lufs = { unit = "LUFS" }`): in the
   * player's columns, `track show` and the mix report.
   */
  display?: { [key: string]: FieldDisplay };
};

export type FieldType =
  | ("number" | "integer" | "string" | "boolean" | "string[]")
  /** A beat grid (feature `analysis`, see [`crate::Output`]). */
  | "beats"
  /** A song structure: the mood and the phrases (feature `analysis`). */
  | "phrases"
  /** A waveform in three bands, in a blob (feature `analysis`). */
  | "waveform";

/**
 * How hosts show a field's values, each part optional. Without them a number shows to two
 * places at most (trailing zeros left out), a list joined with commas, a boolean as yes or no.
 */
export type FieldDisplay = {
  /** Said after a value (`LUFS`); a `db` format's is `dB` unless it says. */
  unit?: string | null;
  /** A number's decimal places, 0 to 6. */
  precision?: number | null;
  format?: FieldFormat | null;
  /** How wide its column is in the player, in cells (1 to 40); for a `meter`, how long it is. */
  width?: number | null;
};

/** What a field's values are, for hosts to show them as such. */
export type FieldFormat =
  /** A key (a string), as the host shows keys. */
  | "key"
  /** Seconds (a number), as minutes and seconds (`3:07`, `1:02:03`). */
  | "duration"
  /** Decibels (a number), signed (`+0.4 dB`, `-8.2 dB`). */
  | "db"
  /** A fraction (a number, 0 to 1) as a percentage (`85%`; no decimal places unless it says). */
  | "percent"
  /** A fraction (a number, 0 to 1) as a bar, `width` cells long (8 unless it says). */
  | "meter";

/**
 * Run with `check/run` on a drive along with locallibrary's own checks, and with `plan/check`
 * before a plan is applied: as its `when` says.
 */
export type Check = {
  name: string;
  title: string;
  /** It runs unless the user turns it off; otherwise, once they turn it on. */
  default_on: boolean;
  /**
   * When it runs: with the check before a set and doctor (`check`, the default), and before a
   * plan is applied (`apply`: a guard).
   */
  when: When[];
};

/** When a check runs. */
export type When =
  /** With locallibrary's check before a set and doctor (`check/run`). */
  | "check"
  /** Before a plan is applied (`plan/check`): a guard. */
  | "apply";

/** Run with `export/write`. */
export type Exporter = {
  format: string;
  title: string;
  /**
   * The file it writes ends in this: lowercase letters and digits, without the dot (`nml`).
   * Either this or `folder`.
   */
  extension?: string | null;
  /** It writes a folder. */
  folder: boolean;
  /** A JSON Schema for its options (an object). */
  options?: unknown;
  /**
   * What it writes out [default: a drive's library]: one source (`"set"`), or a list of them
   * (`["drive", "set", "mix"]`) for an exporter that writes each out the same way.
   */
  source?: ExportSources;
};

/** What an exporter writes out, as its manifest says it: one source, or a list. */
export type ExportSources = ExportFrom | ExportFrom[];

/** What an exporter writes out. */
export type ExportFrom =
  /** A drive's library. */
  | "drive"
  /** A set the DJ declared (`locallibrary sets`). */
  | "set"
  /** A recorded mix's tracklist, as `locallibrary mix` found it. */
  | "mix";

/**
 * Run with `import/read` on a file the user names: the host finds what it read on a drive and
 * plans the edits itself, so it needs no capability to plan one.
 */
export type Importer = {
  format: string;
  title: string;
  /**
   * The file it reads usually ends in this: lowercase letters and digits, without the dot
   * (`nml`).
   */
  extension?: string | null;
  /** A JSON Schema for its options (an object). */
  options?: unknown;
  /** What it reads: what of a track's (and the plays) the host takes from it. */
  provides: ImportKind[];
};

/** What an importer reads. */
export type ImportKind =
  | "cues"
  /** A track's key and BPM. */
  | "fields"
  /** My Tags. */
  | "tags"
  /** What played, as sessions. */
  | "sessions";

/**
 * Asked with `provide/candidates` for what it has for tracks, of its kind: the host offers the
 * user every provider's, and makes the edits chosen itself. It's given the tracks' tags, which
 * takes `library:read`, and needs no capability to plan an edit.
 */
export type Provider = {
  name: string;
  title: string;
  kind: ProviderKind;
};

/**
 * Started with `service/start` by a host that stays open (the player, the sidecar, the `watch`
 * agent), once the user has turned it on (its plugin's setting `services.<name>`), never by a
 * command run once.
 */
export type Service = {
  name: string;
  title: string;
};

/**
 * Read with `library/read`, a page at a time, for the host to build a drive from. It's given the
 * folders the user grants it, which takes `fs:read`.
 */
export type LibrarySource = {
  name: string;
  title: string;
  /** A JSON Schema for its options (an object). */
  options?: unknown;
};

/**
 * `[[contributes.themes]]`: a theme's colours, by the names of [`theme::TOKENS`] as tables (`cues.hot =
 * "#28e214"`, `ui = { accent = "#ff8800" }`), each `#rrggbb` (a gradient, `waveform.*`, a list
 * of two or more). What it leaves out is the default's.
 */
export type Theme = {
  name: string;
  title: string;
  tokens: { [key: string]: unknown };
};

/** A setting the host stores and shows in its settings. */
export type Setting = {
  key: string;
  title: string;
  type: SettingType;
  default?: unknown;
  /** What an `enum` setting can be. */
  choices?: string[];
};

export type SettingType =
  | ("string" | "number" | "boolean" | "file" | "directory")
  /** One of `choices`. */
  | "enum"
  /**
   * A password or a key: kept in the system's keychain, never shown, and given to the plugin
   * only as the host fills it into its `net/fetch` requests. It has no default.
   */
  | "secret";

/**
 * `[[net]]`: at most `per_second` `net/fetch` requests a second to `host`, one of the plugin's
 * `net:` capabilities as it's written (`musicbrainz.org`, `*.archive.org`), however many hooks
 * are under way: the limit the server asks its clients to keep.
 */
export type NetLimit = {
  host: string;
  per_second: number;
};

/** An account of a plugin's, as hosts show it: never a token. */
export type PluginAccountStatus = {
  plugin: string;
  account: string;
  title: string;
  signed_in: boolean;
  /**
   * When its access token expires, when the service said: it's refreshed before then when it
   * can be.
   */
  expires?: string | null;
  /** It's refreshed without signing in again. */
  refreshes: boolean;
  /** What the service granted, as it names them. */
  scopes: string[];
};

/** A plugin's settings now: each one set, else its default. */
export type Settings = {
  plugin: string;
  /** Each one set, else its default; never a secret. */
  settings: { [key: string]: unknown };
  /**
   * Whether each secret setting is set, never what it is; `null` for each when the keychain
   * can't say (see `secrets_unknown`).
   */
  secrets: { [key: string]: boolean | null };
  /** Why the keychain can't say, when it can't: the rest is answered all the same. */
  secrets_unknown?: string | null;
};

/** A sign-in waiting for the browser. */
export type SignIn = {
  /** Finishes it: [`Plugins::finish_sign_in`], [`Plugins::finish_sign_in_with_url`]. */
  id: string;
  plugin: string;
  account: string;
  /** Open this in the browser. */
  url: string;
  /** Where the browser comes back to. */
  redirect_uri: string;
};

/** What signing out did. */
export type SignedOut = {
  plugin: string;
  account: string;
  title: string;
  signed_in: boolean;
  /**
   * When its access token expires, when the service said: it's refreshed before then when it
   * can be.
   */
  expires?: string | null;
  /** It's refreshed without signing in again. */
  refreshes: boolean;
  /** What the service granted, as it names them. */
  scopes: string[];
  /** It was signed in. */
  was_signed_in: boolean;
  /** The service was told to forget its tokens (it has a revocation endpoint, which answered). */
  revoked: boolean;
};

/** What a command answers. */
export type CommandOutput =
  /** A headless command's: shown as it is (`--json`), or not at all. */
  | unknown
  | View;

export type View =
  /** Lines, each with an optional second line. */
  | {
    kind: "list";
    title?: string | null;
    items: ListItem[];
  }
  /**
   * Rows of cells (strings, numbers, booleans or `null`) under column headings: each a name,
   * or (feature `views`) a name with what its cells are ([`Column`]).
   */
  | {
    kind: "table";
    title?: string | null;
    columns: Column[];
    rows: unknown[][];
  }
  /** Labelled values, in order. */
  | {
    kind: "kv";
    title?: string | null;
    pairs: Pair[];
  }
  /** Plain text. */
  | {
    kind: "text";
    title?: string | null;
    text: string;
  }
  /**
   * A question for the user (feature `choose`): the host asks it, then runs the command again
   * with what was chosen (see [`Choice`]). It's never drawn.
   */
  | (PluginChoice & {
    kind: "choose";
  })
  /** Items with items below them (feature `views`). */
  | {
    kind: "tree";
    title?: string | null;
    items: TreeItem[];
  }
  /** Points, each series a line or bars (feature `views`). */
  | {
    kind: "chart";
    title?: string | null;
    style: ChartStyle;
    series: Series[];
    x_label?: string | null;
    y_label?: string | null;
  }
  /**
   * A picture (features `views` and `blobs`): a file in the request's scratch folder, which
   * the host reads (see [`crate::Blob`]).
   */
  | {
    kind: "image";
    title?: string | null;
    /** Its file's name in the scratch folder. */
    blob: string;
    /** What it shows, in a few words: what a host that can't draw it says instead. */
    alt: string;
    /**
     * The picture as a `data:` URL, filled in by the host once it has read the blob and
     * found an image there (locallibrary's: a PNG, shrunk to a preview): whatever a plugin
     * sends here is replaced.
     */
    data?: string | null;
  }
  /** Views one after another, each under a title of its own (feature `views`). */
  | {
    kind: "sections";
    title?: string | null;
    items: Section[];
  };

export type ListItem = {
  label: string;
  detail?: string | null;
};

/**
 * A table's column: its heading alone (text), or (feature `views`) a heading and what its cells
 * are.
 */
export type Column = string | TypedColumn;

/** A column and what its cells are. */
export type TypedColumn = {
  name: string;
  kind: ColumnKind;
  unit?: string | null;
};

/** What a column's cells are, for a host to show them as it shows its own. */
export type ColumnKind =
  /** As they are. */
  | "text"
  /** A track, by id: shown by its artist and title where the host has the drive's library. */
  | "track"
  /** Seconds: shown as minutes and seconds (`3:07`). */
  | "duration"
  /** A key (`8A`, `Am`): shown in Camelot notation where it's one. */
  | "key"
  /** Numbers, with the column's unit after each. */
  | "number";

export type Pair = {
  label: string;
  value: unknown;
};

export type ChoiceItem = {
  label: string;
  detail?: string | null;
  /** What the param is set to when it's chosen (with `multi`, what its list has). */
  value: unknown;
};

export type PluginChoice = {
  /** What's asked, in a line. */
  title: string;
  /** The param the value chosen goes in: one of the hook's. */
  param: string;
  /** Any of the items may be chosen, none too: the param is set to a list of their values. */
  multi?: boolean;
  items: ChoiceItem[];
  /**
   * Questions asked after this one before the hook runs again, each about a param of its own
   * (and asking none of its own). A host that doesn't know them asks this one alone, so a hook
   * still asks what's left when it's run again.
   */
  more?: PluginChoice[];
};

export type TreeItem = {
  label: string;
  detail?: string | null;
  children?: TreeItem[];
};

/** How a chart draws its series. */
export type ChartStyle = "line" | "bar";

/** A chart's series: its name and its points, `[x, y]`. */
export type Series = {
  name: string;
  points: [number, number][];
};

export type Section = {
  title: string;
  view: View;
};

export type FetchReport = {
  /**
   * Where it was fetched from: the other computer's key, shortened, or a plugin's store
   * destination (`<plugin>:<destination>`).
   */
  peer: string;
  libraries: FetchedLibrary[];
  snapshots: number;
  /** Stored files copied, and the store had already. */
  files: number;
  bytes: number;
  present: number;
  dry_run: boolean;
};

export type FetchedLibrary = {
  /** Its name in this store. */
  name: string;
  /** Its name on the other computer, when that was taken here. */
  was?: string | null;
  id: string;
  /** The other computer's latest snapshot. */
  head: Hash | null;
  outcome: LibraryOutcome;
};

export type LibraryOutcome =
  /** This store didn't have it. */
  | "added"
  /** It moved to the other computer's newer latest snapshot. */
  | "updated"
  /** This store was as new, or newer. */
  | "up_to_date"
  /** Both changed: this store keeps its latest, and has the other's beside it. */
  | "diverged"
  /**
   * The other computer's latest snapshot couldn't be had (removed there during the fetch, or
   * missing from its store): the library is left as it was.
   */
  | "unavailable";

/** A set a plugin will publish, as its dry run said. */
export type SetPublish = {
  plugin: string;
  destination: string;
  /** The set, by id. */
  set: string;
  /** Its recording, on this computer, which goes too. */
  recording: string;
  tracks: number;
  /** Where it would be, when the dry run said. */
  url?: string | null;
  /** The link the set has now, which publishing replaces. */
  replaces?: string | null;
};

/** What publishing a set did. */
export type SetPublished = {
  set: string;
  /** Where it can be heard, which is the set's link now; none when the plugin didn't say. */
  url?: string | null;
  /** What the link is (`plyr.fm`), when the plugin named it. */
  name?: string | null;
  /** The Atmosphere record holding the recording, when the plugin keeps it as one. */
  record?: StrongRef | null;
  warnings?: string[];
};

/**
 * What planning a plugin's operation comes to: its plan, or a question for the user first, after
 * which it's planned again with each of its params set ([`Choice::answered`]).
 */
export type OpPlanned = {
  choose: PluginChoice;
} | (EditPlan & PlanFields);

/** An export a plugin will write. */
export type PluginExport = {
  plugin: string;
  exporter: string;
  /** The file, or folder, written to. */
  output: string;
  /** The files the export writes. */
  files: ExportFile[];
  /** What goes in, in the plugin's words (`{"tracks": 12, "cues": 40}`). */
  counts: { [key: string]: number };
  /**
   * What the format can't hold, and what to do with the files. (Problems with this library
   * are the plan's warnings.)
   */
  notes: string[];
};

/**
 * A file an export writes, as the plugin says, and whether it's there already (and so
 * overwritten).
 */
export type ExportFile = {
  /** Absolute, below the export's `output`. */
  path: string;
  /** What it holds, in a few words (`Traktor collection`). */
  what?: string;
  /** Its size, as far as it's known before it's written. */
  bytes: number;
  exists: boolean;
};

/** What `export/write` answers. */
export type Exported = {
  /** The files written (or, for a dry run, that would be). */
  files: PluginExportedFile[];
  /**
   * What went in, as labelled numbers in the order to show them (`{"tracks": 12, "cues":
   * 40}`): the labels are the plugin's own words.
   */
  counts?: { [key: string]: number };
  /** What the format can't hold, and what to do with the files: the same for any library. */
  notes?: string[];
  /** Problems with this library: missing audio, cues the format has no room for... */
  warnings: string[];
};

/** A file an export writes. */
export type PluginExportedFile = {
  /** Absolute, below the export's `output`. */
  path: string;
  /** What it holds, in a few words (`Traktor collection`). */
  what?: string;
  /** Its size, as far as it's known before it's written. */
  bytes: number;
};

/** An import a plugin read, as the host plans it. */
export type PluginImport = {
  plugin: string;
  importer: string;
  /** The file it read. */
  file: string;
  /**
   * How many of the tracks it read were found on the drive: one track (`found`), several it
   * could be (`ambiguous`) or none (`missing`); only those found are edited.
   */
  tracks: ImportSummary;
  /** The same, of its sessions' plays: those not found are kept as the log names them. */
  plays: ImportSummary;
  /** What's added to the tracks found, planned on the drive. */
  edits: EditPlan;
  /** The sessions it keeps in the store, as `sessions.list` will list them. */
  sessions: ImportedSession[];
};

export type ImportSummary = {
  entries: number;
  found: number;
  ambiguous: number;
  missing: number;
};

/** What an import did. */
export type PluginImported = {
  /** The edits' report, when there were any. */
  edits: EditReport | null;
  /** The sessions kept, by id. */
  sessions: string[];
};

/** What analysing a drive's tracks did. */
export type Analyzed = {
  /** Tracks the analyzer worked out fields for, now kept. */
  analyzed: number;
  /** Tracks with fields kept already, for their audio file as it is. */
  kept: number;
  /**
   * Tracks left out, each with a warning: their audio file isn't on the drive, or the plugin
   * failed on them.
   */
  failed: number;
};

/**
 * The plugins' checks, whether each runs, and how long one has to answer, as `plugin checks` and
 * the sidecar's `plugins.checks` show them.
 */
export type Checks = {
  /** In the order the plugins are. */
  checks: ListedCheck[];
  /**
   * How long a check has to answer, in seconds, before it's stopped (a warning says so): the
   * `checks.timeout` setting.
   */
  timeout: number;
};

/**
 * Whether a plugin's check runs, and whether a guard keeps a plan it finds something about from
 * being applied.
 */
export type ListedCheck = {
  plugin: string;
  name: string;
  title: string;
  /**
   * With the check before a set and doctor (`check`), before a plan is applied (`apply`: a
   * guard), or both.
   */
  when: When[];
  on: boolean;
  /** Only a guard's can be on. */
  blocking: boolean;
};

/** What the providers found. */
export type Provided = {
  kind: ProviderKind;
  /** How many tracks they were asked about. */
  asked: number;
  /**
   * Each track something was found for (tags only when they'd change it), in the library's
   * order.
   */
  offers: Offer[];
  /** The images the candidates are of, each once, by id (its content's hash): in base64. */
  images: { [key: string]: string };
  /**
   * A provider that failed (what the others found is offered), one that answered more than is
   * kept (the rest left out), and what each said it found nothing for.
   */
  warnings: string[];
};

/** What was found for a track. */
export type Offer = {
  track: number;
  /** The track, as it's named (`Artist - Title`). */
  name: string;
  /** Best first. */
  candidates: Found[];
  /**
   * The one picked for the user, by its place in `candidates`: the only one scoring the
   * `providers.auto_pick` setting or more.
   */
  picked: number | null;
};

/**
 * The providers the plugins have, and when one's candidate is picked for the user, as `plugin
 * providers` and the sidecar's `plugins.providers` show them.
 */
export type Providers = {
  /** In the order the plugins are. */
  providers: ListedProvider[];
  /**
   * A candidate alone scoring this or more (0 to 1) is picked for the user: the
   * `providers.auto_pick` setting.
   */
  auto_pick: number;
};

export type ListedProvider = {
  plugin: string;
  name: string;
  title: string;
  kind: ProviderKind;
};

/** A role, and the analyzers answering it. */
export type RoleChoice = {
  role: FieldRole;
  /**
   * The analyzer it's read from now, `<plugin>.<analyzer>` (none: no analyzer has it, or it's
   * turned off).
   */
  chosen: string | null;
  /** Chosen in the settings, rather than by default. */
  set: boolean;
  /** Read from no analyzer, as the settings say ([`turn_off`]). */
  off: boolean;
  /** Every analyzer with it, `<plugin>.<analyzer>`, in the order the app has its plugins. */
  offered: string[];
};

/** A `progress` notification's parameters. */
export type ProgressParams = {
  /** The request it's about. */
  id: RequestId | null;
  event: ProgressEvent;
};

/**
 * What happened, which drive and step it's about, and (for a coalesced `item`) how many files
 * it stands for.
 */
export type ProgressEvent = {
  target?: string | null;
  step?: Step | null;
  /** How many files an `item` stands for, when more than one (count `items ?? 1`). */
  items?: number | null;
} & (
  | Event
  /** A step begins (`step` says which). */
  | StepStarted);

export type Event =
  /** A phase begins. `items`/`bytes` are totals when known up front (0 otherwise). */
  | {
    event: "phase_start";
    phase: Phase;
    items: number;
    bytes: number;
  }
  /** Work on one item (usually a file) begins. */
  | {
    event: "item";
    phase: Phase;
    path: string;
    bytes: number;
  }
  /** `bytes` more bytes were processed in the current phase. */
  | {
    event: "bytes";
    phase: Phase;
    bytes: number;
  }
  | {
    event: "phase_end";
    phase: Phase;
  }
  | {
    event: "warning";
    message: string;
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
  | "analyze"
  /** Sending files to a server (a set's recording, by a plugin's `net/fetch`). */
  | "upload";

/** `{"event": "step"}`. */
export type StepStarted = {
  event: StepTag;
};

export type StepTag = "step";

/** Step `index` of `total`, counting from 1. `id` names it (`format`, `library`, `verify`, ...). */
export type Step = {
  index: number;
  total: number;
  id: string;
};

/**
 * An `event` notification's parameters: drives and the store (after `events.subscribe`),
 * requests starting and finishing, what `watch.start` does, and what plugins tell the user.
 */
export type Notice = WatchEvent | JobEvent | WatchAction | PluginEvent;

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
    event: "drive.changed";
    root: string;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    /** Its adopted name, else the volume's name. */
    name: string;
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    changed: DriveChange[];
  }
  /**
   * The store's libraries, snapshots, labels, drive registry, settings, smart playlists or
   * sets changed.
   */
  | {
    event: "store.changed";
    root: string;
    changed: string[];
  };

/** What changed on a drive (see [`WatchEvent::Changed`]). */
export type DriveChange =
  /** Its database files: an export, an edit, a sync, or a player recording history. */
  | "databases"
  /** Its `.locallibrary` state: its identity, name, library or lineage. */
  | "state";

/** A request starting or finishing. */
export type JobEvent = {
  event: "job.started";
  id: RequestId | null;
  method: string;
  started: string;
} | {
  event: "job.finished";
  id: RequestId | null;
  method: string;
  ok: boolean;
  /** The error's `kind`, when it failed. */
  kind?: string | null;
};

/** What watching did about a drive. */
export type WatchAction =
  /** The store holds the drive's state already. */
  | {
    event: "watch.up_to_date";
    root: string;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    /** Its adopted name, else the volume's name. */
    name: string;
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    library: string;
  }
  /** The drive holds a state the store lacks, and it wasn't pulled (watching without `pull`). */
  | {
    event: "watch.pending";
    root: string;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    /** Its adopted name, else the volume's name. */
    name: string;
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    library: string;
    /** History lists the store lacks. */
    history_lists: string[];
    /** It would be saved beside the library's newest snapshot (back from a set). */
    beside: boolean;
  }
  /** The drive's state was pulled into the store as a new snapshot. */
  | {
    event: "watch.pulled";
    root: string;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    /** Its adopted name, else the volume's name. */
    name: string;
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    library: string;
    snapshot: SnapshotInfo;
    /** History lists the store lacked. */
    history_lists: string[];
    /** Saved beside the library's newest snapshot, which stays the newest. */
    beside: boolean;
  }
  /** The library's other mounted drives were synced from this one. */
  | {
    event: "watch.synced";
    root: string;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    /** Its adopted name, else the volume's name. */
    name: string;
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    library: string;
    targets: TargetResult[];
  }
  /**
   * Another operation holds the store or the drive: it's looked at again once that's done.
   * Said once per wait.
   */
  | {
    event: "watch.waiting";
    root: string;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    /** Its adopted name, else the volume's name. */
    name: string;
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    reason: string;
  }
  /** Left alone, and why. */
  | {
    event: "watch.skipped";
    root: string;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    /** Its adopted name, else the volume's name. */
    name: string;
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    reason: string;
  }
  /** Looking at the drive failed (other than waiting on a lock): `kind` is the error's code. */
  | {
    event: "watch.failed";
    root: string;
    /** The identity locallibrary gave it (`.locallibrary/state.json`), if any. */
    id: string | null;
    /** Its adopted name, else the volume's name. */
    name: string;
    /** Whether it holds a rekordbox export (either database). */
    has_export: boolean;
    kind: string;
    message: string;
  };

/** What a plugin did. */
export type PluginEvent = PluginNotice & {
  event: "plugin.notice";
};

/** What a plugin suggests (`host/notify`): an operation's or an action's name, and its params. */
export type Suggested = {
  name: string;
  params: unknown;
};

/**
 * One of the host's own flows a plugin may suggest (`host/notify`'s `action`): the host starts it
 * with these params filled in, and it's planned and confirmed as when the user starts it. Each is
 * on the plugin's own contributions alone, so it never starts another plugin's work.
 */
export type HostAction =
  /** Fetch cover art from its artwork providers (`locallibrary art fetch`). */
  | {
    name: "art.fetch";
    params: FetchAction;
  }
  /** Fetch tags from its metadata providers (`locallibrary track fetch`). */
  | {
    name: "track.fetch";
    params: FetchAction;
  }
  /** Export in one of its formats, to where the user picks. */
  | {
    name: "export";
    params: ExportAction;
  }
  /** Run one of its analyzers on a drive's tracks (`locallibrary analyze`). */
  | {
    name: "analyze";
    params: AnalyzeAction;
  };

/** `art.fetch`'s and `track.fetch`'s params. */
export type FetchAction = {
  /** The drive, by id or name (as events name it). */
  drive: string;
  /** Only these tracks, by id [default: every one]. */
  tracks?: number[];
  /** Only this provider of the plugin's [default: each of its providers of the kind]. */
  provider?: string | null;
};

/**
 * `export`'s params: what its exporter writes out (a drive's library, a set or a mix, as
 * `export/write`'s source names them) and its options. Where to is the user's to pick.
 */
export type ExportAction = {
  /** One of the plugin's exporters, by format. */
  format: string;
  /** The drive, by id or name: a drive exporter's library, else whose tracks to give it. */
  drive?: string | null;
  /** A set exporter's set. */
  set?: string | null;
  /** A mix exporter's mix. */
  mix?: string | null;
  /** As the exporter's `options` schema says. */
  options?: { [key: string]: unknown };
};

/** `analyze`'s params. */
export type AnalyzeAction = {
  /** One of the plugin's analyzers. */
  analyzer: string;
  /** The drive, by id or name. */
  drive: string;
  /** Only these tracks, by id [default: every one]. */
  tracks?: number[];
};

/**
 * A line a plugin tells the user (`host/notify`), and what it suggests, with its params: one of
 * its operations (`operation`) or one of the host's actions (`action`) on its own contributions.
 * The front end offers to start it, and the user confirms it as when they start it themselves.
 */
export type PluginNotice = {
  /** The plugin's id. */
  plugin: string;
  message: string;
  operation?: Suggested;
  action?: HostAction;
};

/** An error response's `error.data`: what a front end can act on without reading the message. */
export type ErrorData = {
  /** Stable: `not_found`, `conflict`, `stale_plan`, `invalid_params`... (see [`crate::Error::code`]). */
  kind: string;
  /** Which decision a `conflict` needs, or what else would resolve the error. */
  reason: Reason | null;
  /**
   * The values it's about: a `path` (for `invalid_params`, where in the request), the space
   * `needed` and `available`, the `drive`, `left` and `right`...
   */
  fields: { [key: string]: unknown };
  /**
   * What a front end can offer: call `method` with `params` (the request that failed, with
   * some parameters added, is one), with administrator rights when `elevate` is set.
   */
  actions: Action[];
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
   * The drive holds an older state of the library than the store: one its latest went
   * through, or an earlier snapshot's (`fields.snapshot`, when a snapshot holds it). Push to
   * update it, or force the pull to record it as the newest.
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
  /**
   * A plugin's guard the user made blocking (`fields.check`, `<plugin>.<check>`) found
   * something about the plan: deal with what it found, or stop the guard blocking.
   */
  | "blocked"
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

export type FileProblemOutput = {
  path: string;
  class: FileClass;
  size: number;
  expected?: Hash | null;
  actual?: Hash | null;
  reference?: ReferenceKind | null;
  /**
   * The modification time the good copy had (restored on repair, so syncs and caches see the
   * file they knew).
   */
  mtime?: number | null;
  /** Why it couldn't be read. */
  error?: string | null;
  /** `None`: no known-good copy is available. */
  repair: RepairSource | null;
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
  "drives.list": { params: ListDrives; result: ListedDrive[] };
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
  "playlists.tree": { params: LibraryRef; result: PlaylistTree };
  /** read method */
  "tracks.get": { params: TracksGet; result: TracksById };
  /** read method */
  "tracks.query": { params: TracksQuery; result: TracksPage };
  /** read method */
  "track.analysis": { params: AnalysisRequest; result: AnalysisView };
  /** read method */
  "tracks.analysis": { params: TracksAnalysisRequest; result: TracksAnalysis };
  /** read method */
  "track.fields": { params: PluginFieldsRequest; result: PluginFields };
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
  /** plan method; paired with `upgrade.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "upgrade.plan": { params: UpgradeRequest & WriteOptions; result: UpgradePlan & PlanFields };
  /** apply method; paired with `upgrade.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "upgrade.apply": { params: ApplyPlan | (UpgradeRequest & WriteOptions); result: EditReport };
  /** plan method; paired with `history.delete.apply`; can delete or overwrite something */
  "history.delete.plan": { params: DeleteHistoryRequest & WriteOptions; result: EditPlan & PlanFields };
  /** apply method; paired with `history.delete.plan`; can delete or overwrite something */
  "history.delete.apply": { params: ApplyPlan | (DeleteHistoryRequest & WriteOptions); result: EditReport };
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
  "art.set.plan": { params: SetArtwork & WriteOptions; result: EditPlan & PlanFields };
  /** apply method; paired with `art.set.plan`; can delete or overwrite something */
  "art.set.apply": { params: ApplyPlan | (SetArtwork & WriteOptions); result: EditReport };
  /** apply method; paired with `art.set.plan`; can delete or overwrite something; an older name for `art.set.apply` */
  "art.set": { params: ApplyPlan | (SetArtwork & WriteOptions); result: EditReport };
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
  /** immediate method; long running: sends `progress`, can be cancelled */
  "sets.lookup": { params: SetPublishRequest & WriteOptions; result: SetLookup };
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
  /** plan method; paired with `collection.export.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "collection.export.plan": { params: CollectionExportRequest & WriteOptions; result: CollectionExportPlan & PlanFields };
  /** apply method; paired with `collection.export.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "collection.export.apply": { params: ApplyPlan | (CollectionExportRequest & WriteOptions); result: CollectionExportReport };
  /** read method */
  "plugins.list": { params: NoParams; result: Listed[] };
  /** read method */
  "plugins.describe": { params: PluginRef; result: Described };
  /** read method */
  "plugins.settings": { params: PluginRef; result: Settings };
  /** immediate method */
  "plugins.settings.set": { params: SetPluginSetting & WriteOptions; result: Settings };
  /** read method */
  "plugins.accounts": { params: PluginRef; result: PluginAccountStatus[] };
  /** immediate method */
  "plugins.account.sign_in.begin": { params: AccountRef & WriteOptions; result: SignIn };
  /** immediate method; long running: sends `progress`, can be cancelled */
  "plugins.account.sign_in.finish": { params: FinishSignIn & WriteOptions; result: PluginAccountStatus };
  /** immediate method */
  "plugins.account.sign_in.paste": { params: PasteSignIn & WriteOptions; result: PluginAccountStatus };
  /** immediate method */
  "plugins.account.sign_in.cancel": { params: CancelSignIn & WriteOptions; result: Cancelled };
  /** immediate method */
  "plugins.account.sign_out": { params: AccountRef & WriteOptions; result: SignedOut };
  /** plan method; paired with `plugins.apply_analysis.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.apply_analysis.plan": { params: ApplyAnalysis & WriteOptions; result: EditPlan & PlanFields };
  /** apply method; paired with `plugins.apply_analysis.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.apply_analysis.apply": { params: ApplyPlan | (ApplyAnalysis & WriteOptions); result: EditReport };
  /** read method; long running: sends `progress`, can be cancelled */
  "plugins.run": { params: RunPluginCommand; result: CommandOutput };
  /** immediate method; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.store.copy": { params: CopyTo & WriteOptions; result: CopyReport };
  /** immediate method; long running: sends `progress`, can be cancelled */
  "plugins.store.restore": { params: RestoreFrom & WriteOptions; result: FetchReport };
  /** read method; long running: sends `progress`, can be cancelled */
  "plugins.store.verify": { params: VerifyFrom; result: ScrubReport };
  /** immediate method; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.store.repair": { params: RepairFrom & WriteOptions; result: RepairReport };
  /** plan method; paired with `plugins.publish.apply`; long running: sends `progress`, can be cancelled */
  "plugins.publish.plan": { params: PublishSetAt & WriteOptions; result: SetPublish & PlanFields };
  /** apply method; paired with `plugins.publish.plan`; long running: sends `progress`, can be cancelled */
  "plugins.publish.apply": { params: ApplyPlan | (PublishSetAt & WriteOptions); result: SetPublished };
  /** plan method; paired with `plugins.op.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.op.plan": { params: PlanPluginOp & WriteOptions; result: OpPlanned };
  /** apply method; paired with `plugins.op.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.op.apply": { params: ApplyPlan | (PlanPluginOp & WriteOptions); result: EditReport };
  /** read method; long running: sends `progress`, can be cancelled */
  "plugins.panel": { params: RenderPluginPanel; result: View };
  /** plan method; paired with `plugins.export.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.export.plan": { params: PlanPluginExport & WriteOptions; result: PluginExport & PlanFields };
  /** apply method; paired with `plugins.export.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.export.apply": { params: ApplyPlan | (PlanPluginExport & WriteOptions); result: Exported };
  /** plan method; paired with `plugins.import.apply`; long running: sends `progress`, can be cancelled */
  "plugins.import.plan": { params: PlanPluginImport & WriteOptions; result: PluginImport & PlanFields };
  /** apply method; paired with `plugins.import.plan`; long running: sends `progress`, can be cancelled */
  "plugins.import.apply": { params: ApplyPlan | (PlanPluginImport & WriteOptions); result: PluginImported };
  /** immediate method; long running: sends `progress`, can be cancelled */
  "plugins.analyze": { params: AnalyzeWithPlugin & WriteOptions; result: Analyzed };
  /** plan method; paired with `plugins.apply_fields.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.apply_fields.plan": { params: ApplyFields & WriteOptions; result: EditPlan & PlanFields };
  /** apply method; paired with `plugins.apply_fields.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.apply_fields.apply": { params: ApplyPlan | (ApplyFields & WriteOptions); result: EditReport };
  /** read method */
  "plugins.checks": { params: NoParams; result: Checks };
  /** immediate method */
  "plugins.checks.set": { params: SetCheck & WriteOptions; result: Checks };
  /** immediate method */
  "plugins.checks.timeout.set": { params: SetTimeout & WriteOptions; result: Checks };
  /** read method; long running: sends `progress`, can be cancelled */
  "plugins.provide": { params: Provide; result: Provided };
  /** plan method; paired with `plugins.provide.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.provide.plan": { params: PlanProvided & WriteOptions; result: EditPlan & PlanFields };
  /** apply method; paired with `plugins.provide.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.provide.apply": { params: ApplyPlan | (PlanProvided & WriteOptions); result: EditReport };
  /** read method */
  "plugins.providers": { params: NoParams; result: Providers };
  /** immediate method */
  "plugins.providers.set": { params: SetAutoPick & WriteOptions; result: Providers };
  /** read method */
  "plugins.roles": { params: NoParams; result: RoleChoice[] };
  /** immediate method */
  "plugins.roles.set": { params: ChooseRole & WriteOptions; result: RoleChoice[] };
  /** plan method; paired with `plugins.source.apply`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.source.plan": { params: SourceExport & WriteOptions; result: CollectionExportPlan & PlanFields };
  /** apply method; paired with `plugins.source.plan`; long running: sends `progress`, can be cancelled; can delete or overwrite something */
  "plugins.source.apply": { params: ApplyPlan | (SourceExport & WriteOptions); result: CollectionExportReport };
}

export type MethodName = keyof Methods;
export type Params<M extends MethodName> = Methods[M]["params"];
export type Result<M extends MethodName> = Methods[M]["result"];

/** Notifications the sidecar sends, by method. */
export interface Notifications {
  progress: ProgressParams;
  event: Notice;
}
