use serde_json::json;

use super::*;
use crate::masterdb::files::tests::{SECRET, has, library, plain, registry};
use crate::masterdb::{DEFAULT_KEY, PATH_ENV};
use crate::plugin::blobs::Scratch;

/// A process's state with one request under way, its scratch folder, and rekordbox's `master.db`
/// (the sample library) where [`PATH_ENV`] says, outside the folder.
fn under_way() -> (tempfile::TempDir, Live, Scratch, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let master = dir.path().join("rekordbox/master.db");
    std::fs::create_dir(dir.path().join("rekordbox")).unwrap();
    std::fs::write(&master, library().encrypted(DEFAULT_KEY)).unwrap();
    // SAFETY: nextest runs each test in a process of its own.
    unsafe { std::env::set_var(PATH_ENV, &master) };
    let live = Live::new(None);
    let scratch = live.scratches.make(&dir.path().join("scratch"), &[]).unwrap();
    let folder = scratch.dir().unwrap().to_path_buf();
    (dir, live, scratch, folder)
}

fn call(live: &Live, method: &str, params: Value) -> Result<Value, RpcFailure> {
    answer(live, 512 << 20, method, params)
}

#[test]
fn only_its_own_reads_are_answered_here() {
    for method in METHODS {
        assert!(answers(method), "{method}");
    }
    assert!(!answers("collection.info") && !answers("tracks.query"));
}

/// Where the files are is said as it is; and whether rekordbox is running.
#[test]
fn locations_and_status_are_answered() {
    let (dir, live, _scratch, _) = under_way();
    let at = call(&live, "collection.locations", json!({})).unwrap();
    assert_eq!(at["master_db"], json!(dir.path().join("rekordbox/master.db")));
    assert_eq!(at["master_db_exists"], true);
    let status = call(&live, "collection.status", json!({})).unwrap();
    assert!(status["running"].is_boolean(), "{status}");
}

/// A copy goes into the request's scratch folder, without the cloud agent's sign-in unless
/// it's asked for, and reads back there.
#[test]
fn a_copy_goes_into_the_scratch_folder() {
    let (_dir, live, _scratch, folder) = under_way();
    let into = folder.join("master.db");
    let made = call(&live, "collection.copy", json!({ "into": into })).unwrap();
    assert_eq!((made["credentials_removed"].clone(), made["db_id"].clone()), (json!(1), json!("1234567890")));
    assert_eq!(made["bytes"], std::fs::metadata(&into).unwrap().len());
    assert!(!has(&plain(&into), SECRET));
    let kept = folder.join("with.db");
    let made = call(&live, "collection.copy", json!({ "into": kept, "with_credentials": true })).unwrap();
    assert_eq!(made["credentials_removed"], 0);
    assert_eq!(registry(&kept)[0], "agentCredentials");
    // Its tracks, from the copy.
    let read = call(&live, "collection.tracks", json!({ "db": into, "limit": 2 })).unwrap();
    assert_eq!((read["total"].clone(), read["tracks"].as_array().unwrap().len()), (json!(4), 2));
    assert_eq!(read["tracks"][1]["path"], "/Users/dj/Music/Downloads/ngo.mp3");
    // And rekordbox's own, without a copy named.
    assert_eq!(call(&live, "collection.tracks", json!({ "offset": 3, "limit": 500 })).unwrap()["tracks"][0]["id"], "4");
}

/// A copy is written only into the scratch folder of a request under way, as a new file, and
/// as far as the folder has room; tracks are read only from a file there, itself.
#[test]
fn files_outside_the_scratch_folder_are_refused() {
    let (dir, live, _scratch, folder) = under_way();
    let master = dir.path().join("rekordbox/master.db");
    let before = std::fs::read(&master).unwrap();
    for into in [dir.path().join("outside.db"), master.clone(), PathBuf::from("master.db")] {
        let e = call(&live, "collection.copy", json!({ "into": into })).unwrap_err();
        assert_eq!(e.kind, "invalid", "{}", e.message);
        assert!(e.message.ends_with("isn't in the scratch folder of a request under way"), "{}", e.message);
    }
    assert!(!dir.path().join("outside.db").exists());
    assert_eq!(std::fs::read(&master).unwrap(), before, "rekordbox's own is never written");
    let e = call(&live, "collection.copy", json!({ "into": folder.join("../escape.db") })).unwrap_err();
    assert_eq!(e.kind, "invalid", "{}", e.message);
    // Not over a file there already, nor past the folder's room.
    std::fs::write(folder.join("mine"), "mine").unwrap();
    assert!(call(&live, "collection.copy", json!({ "into": folder.join("mine") })).is_err());
    assert_eq!(std::fs::read_to_string(folder.join("mine")).unwrap(), "mine");
    let e = answer(&live, 4096, "collection.copy", json!({ "into": folder.join("big.db") })).unwrap_err();
    assert_eq!(e.kind, "too_big");
    assert!(!folder.join("big.db").exists());
    // Tracks of a file outside, or of a link in the folder to one outside.
    let e = call(&live, "collection.tracks", json!({ "db": master, "limit": 1 })).unwrap_err();
    assert_eq!(e.kind, "invalid");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&master, folder.join("link.db")).unwrap();
        let e = call(&live, "collection.tracks", json!({ "db": folder.join("link.db"), "limit": 1 })).unwrap_err();
        assert_eq!(e.kind, "invalid", "{}", e.message);
    }
    let e = call(&live, "collection.copy", json!({ "with_credentials": true })).unwrap_err();
    assert_eq!(e.kind, "invalid_params");
}
