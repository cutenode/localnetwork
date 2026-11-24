use hegel::TestCase;
use hegel::generators as gs;
use serde_json::{Value, json};

use super::*;
use crate::store::{PLAYER_FILE, Store, player_fallback};

#[hegel::test_helper]
fn key(tc: &TestCase) -> Key {
    let named = || gs::sampled_from(Named::ALL.to_vec());
    match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => Key::Char(tc.draw(gs::characters().exclude_categories(&["Cc", "Cs", "Zs", "Zl", "Zp"]))),
        1 => Key::Named(tc.draw(named())),
        2 => Key::Shift(tc.draw(named())),
        3 => Key::Ctrl(tc.draw(gs::sampled_from(('a'..='z').collect::<Vec<_>>()))),
        _ => Key::F(tc.draw(gs::integers::<u8>().min_value(1).max_value(12))),
    }
}

/// Every key reads back from how it's written, unless no action can have it, which says why; and
/// written in another case, or `ctrl+` as plugins' settings had it, or with blanks around, it's the
/// same key.
#[hegel::test(test_cases = 500)]
fn prop_key_spellings_round_trip(tc: TestCase) {
    let key = key(&tc);
    let written = key.to_string();
    match key.reserved() {
        None => assert_eq!(Key::parse(&written), Ok(key), "{written:?}"),
        Some(why) => assert_eq!(Key::parse(&written), Err(why.to_string()), "{written:?}"),
    }
    let loose = match key {
        Key::Char(_) => format!(" {written}\t"),
        Key::Ctrl(c) => format!("CTRL+{}", c.to_ascii_uppercase()),
        _ => written.to_uppercase(),
    };
    assert_eq!(Key::parse(&loose), Key::parse(&written), "{loose:?}");
}

/// What isn't one key is refused, saying why: alt- keys, esc, sequences, f keys past f12, shift-
/// with a letter or an f key, ctrl- with anything but a letter, and nothing at all.
#[hegel::test(test_cases = 200)]
fn prop_what_isnt_a_key_is_refused(tc: TestCase) {
    let alt = format!("alt-{}", tc.draw(gs::sampled_from(vec!["a", "x", "enter", "f1"])));
    let other = tc.draw(gs::sampled_from(vec![
        "", "  ", "g g", "ab", "f0", "f13", "f+5", "f05", "shift-a", "shift-f5", "ctrl-1", "ctrl-é", "ctrl-ab", "pgup",
        "\u{7}",
    ]));
    let spelled = tc.draw(gs::sampled_from(vec![alt.clone(), other.to_string(), "esc".into(), "Escape".into()]));
    let why = Key::parse(&spelled).unwrap_err();
    if spelled == alt {
        assert!(why.ends_with("the player doesn't take alt- keys"), "{why}");
    } else if spelled.eq_ignore_ascii_case("esc") || spelled.eq_ignore_ascii_case("escape") {
        assert_eq!(why, "esc always closes");
    } else {
        assert!(why.contains("isn't a key: one character"), "{spelled:?}: {why}");
    }
}

#[test]
fn the_fixed_keys_are_refused_with_why() {
    let refused = |s: &str| Key::parse(s).unwrap_err();
    assert_eq!(refused("ctrl-c"), "ctrl-c always quits");
    assert_eq!(refused("?"), "? always opens the help");
    assert_eq!(refused(":"), ": always opens the commands");
    assert_eq!(refused("ctrl-h"), "terminals send ctrl-h as backspace");
    assert_eq!(refused("ctrl-i"), "terminals send ctrl-i as tab");
    assert_eq!(refused("ctrl-m"), "terminals send ctrl-m as enter");
    assert_eq!(Key::parse("shift-tab"), Ok(Key::Shift(Named::Tab)));
    assert_eq!(Key::parse("PageDown").map(|k| k.to_string()), Ok("pagedown".into()));
}

/// Before there's a store the file is kept beside its folder; once there is one, in it, and the one
/// kept beside it is still read until then. A missing file is no keys; one that isn't the file is
/// an error, and isn't saved over by loading.
#[test]
fn the_file_lives_where_the_players_does() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    assert_eq!(load(&root).unwrap(), Keybinds::default());

    let mut binds = Keybinds { from: Some("did:plc:abc".into()), ..Default::default() };
    binds.keys.insert("playback.next".into(), vec!["]".into()]);
    binds.keys.insert("deck.hot-cue".into(), vec!["&".into(), "é".into()]);
    binds.keys.insert("player.jobs".into(), vec![]);
    save(&root, &binds).unwrap();
    let beside = player_fallback(&root).unwrap().join(KEYBINDS_FILE);
    let text = std::fs::read_to_string(&beside).unwrap();
    assert!(!root.exists(), "the player doesn't make the store's folder");
    // Sorted by action, as written.
    let order = ["deck.hot-cue", "playback.next", "player.jobs"];
    let at = |id: &str| text.find(&format!("\"{id}\"")).unwrap();
    assert!(order.windows(2).all(|w| at(w[0]) < at(w[1])), "{text}");
    assert!(!text.contains("updatedAt"), "{text}");
    assert_eq!(load(&root).unwrap(), binds);

    Store::open_or_init(&root).unwrap();
    assert_eq!(load(&root).unwrap(), binds, "read from beside the store until it's written in it");
    binds.keys.shift_remove("player.jobs");
    save(&root, &binds).unwrap();
    assert!(root.join(KEYBINDS_FILE).is_file());
    assert_eq!(std::fs::read_to_string(&beside).unwrap(), text, "the one beside it is left");
    assert_eq!(load(&root).unwrap(), binds);

    std::fs::write(root.join(KEYBINDS_FILE), "{ not json").unwrap();
    assert!(matches!(load(&root), Err(Error::Parse { .. })));
    std::fs::write(root.join(KEYBINDS_FILE), json!({ "keys": ["a"] }).to_string()).unwrap();
    assert!(matches!(load(&root), Err(Error::Parse { .. })));
    // A folder with no name has nowhere beside it.
    let nowhere = Path::new("/");
    assert_eq!(load(nowhere).unwrap(), Keybinds::default());
    assert!(save(nowhere, &binds).unwrap_err().to_string().contains("nowhere beside / to keep keys"));
}

/// A store can still be made in a folder that holds only what locallibrary keeps beside one, the
/// keys among them.
#[test]
fn the_store_can_be_made_beside_keybinds_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(KEYBINDS_FILE), "{}").unwrap();
    std::fs::write(root.join(PLAYER_FILE), "{}").unwrap();
    Store::open_or_init(&root).unwrap();
    let other = dir.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join(KEYBINDS_FILE), "{}").unwrap();
    std::fs::write(other.join("notes.txt"), "").unwrap();
    assert!(Store::open_or_init(&other).is_err(), "anything else, and it isn't made");
}

fn plugin_settings(store: &Store, id: &str, settings: Value) {
    let dir = store.root().join("plugins").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("settings.json"), settings.to_string()).unwrap();
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// Plugins' `keys.<name>` settings move into the file once, spelled as keys are, with what can't
/// move said: a key that isn't one, one another action has, and an action the file has already.
/// The plugins keep their other settings. A second run, with nothing to move, reads and writes
/// nothing: not even a file that can't be read.
#[test]
fn plugin_keys_move_to_the_keybinds_file_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    assert_eq!(migrate::plugin_keys(&store).unwrap(), Vec::<String>::new());
    assert!(!store.root().join(KEYBINDS_FILE).exists(), "nothing to move, nothing written");

    let mut binds = Keybinds::default();
    binds.keys.insert("plugin.sound.tag".into(), vec!["f9".into()]);
    binds.keys.insert("library.search".into(), vec!["F".into()]);
    save(store.root(), &binds).unwrap();
    plugin_settings(
        &store,
        "sound",
        json!({ "greeting": "hi", "keys.analyze": " F5 ", "keys.tag": "f2", "keys.blank": " ", "services.watch": true }),
    );
    plugin_settings(&store, "lyrics", json!({ "keys.fetch": "Ctrl+J", "keys.show": "alt-x", "keys.count": 5 }));
    plugin_settings(&store, "zines", json!({ "keys.open": "F", "keys.next": "]", "keys.again": "ctrl-j" }));
    plugin_settings(&store, "quiet", json!({ "colour": "red" }));

    let dropped = migrate::plugin_keys(&store).unwrap();
    assert_eq!(
        dropped,
        [
            "plugin.lyrics.show: \"alt-x\": the player doesn't take alt- keys",
            "plugin.lyrics.count: 5 isn't a key",
            "plugin.sound.tag: keybinds.json has its keys already",
            "plugin.zines.open: F is another action's",
            "plugin.zines.again: ctrl-j is another action's",
        ]
    );
    let moved = load(store.root()).unwrap();
    let keys = |id: &str| moved.keys.get(id).cloned();
    assert_eq!(keys("plugin.lyrics.fetch"), Some(vec!["ctrl-j".to_string()]));
    assert_eq!(keys("plugin.sound.analyze"), Some(vec!["f5".to_string()]));
    assert_eq!(keys("plugin.zines.next"), Some(vec!["]".to_string()]));
    assert_eq!(keys("plugin.sound.tag"), Some(vec!["f9".to_string()]), "the file's stays");
    assert_eq!((keys("plugin.sound.blank"), keys("plugin.lyrics.show")), (None, None));
    let plugin = |id: &str| read(&store.root().join("plugins").join(id).join("settings.json"));
    assert_eq!(plugin("sound"), json!({ "greeting": "hi", "services.watch": true }));
    assert_eq!(plugin("lyrics"), json!({}));
    assert_eq!(plugin("quiet"), json!({ "colour": "red" }));

    std::fs::write(store.root().join(KEYBINDS_FILE), "{ not json").unwrap();
    assert_eq!(migrate::plugin_keys(&store).unwrap(), Vec::<String>::new());
    assert_eq!(std::fs::read_to_string(store.root().join(KEYBINDS_FILE)).unwrap(), "{ not json");
}

/// A file that can't be read stops the move before anything's written: the plugins keep their keys.
#[test]
fn plugin_keys_stay_when_the_file_cant_be_read() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    plugin_settings(&store, "sound", json!({ "keys.analyze": "a" }));
    std::fs::write(store.root().join(KEYBINDS_FILE), "{ not json").unwrap();
    assert!(migrate::plugin_keys(&store).is_err());
    assert_eq!(read(&store.root().join("plugins/sound/settings.json")), json!({ "keys.analyze": "a" }));
}

#[cfg(feature = "account")]
mod record {
    use super::*;
    use crate::atmosphere::lexicon::{self, KEYBINDS};

    /// An action id, a few of them known, some of them too long for the Lexicon.
    #[hegel::test_helper]
    fn action(tc: &TestCase) -> String {
        match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => tc.draw(gs::sampled_from(vec!["playback.next", "deck.hot-cue", "player.jobs"])).to_string(),
            1 => "x".repeat(tc.draw(gs::integers::<usize>().min_value(120).max_value(140))),
            _ => tc.draw(gs::from_regex(r"[a-z]{1,6}\.[a-z-]{1,8}")),
        }
    }

    /// A file as written by hand: keys that are keys, and some that aren't.
    #[hegel::test_helper]
    fn keybinds(tc: &TestCase) -> Keybinds {
        let mut out = Keybinds {
            from: tc.draw(gs::optional(gs::sampled_from(vec!["did:plc:abc".to_string(), "not a did".into()]))),
            updated_at: tc.draw(gs::optional(gs::sampled_from(vec!["2026-11-20T18:00:00Z".to_string()]))),
            ..Default::default()
        };
        // More than a record holds, now and then.
        if tc.draw(gs::weighted_booleans(0.1)) {
            out.keys.extend((0..520).map(|i| (format!("bulk.action-{i:03}"), Vec::new())));
        }
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
            let n = tc.draw(gs::integers::<usize>().max_value(10));
            let spelled: Vec<String> = (0..n)
                .map(|_| match tc.draw(gs::booleans()) {
                    true => key(tc).to_string(),
                    false => tc.draw(gs::sampled_from(vec!["alt-x", "esc", "nonsense key", "?"])).to_string(),
                })
                .collect();
            out.keys.insert(action(tc), spelled);
        }
        out
    }

    /// Whatever the file holds, its record fits the Lexicon, and reads back as the keys it could
    /// say: every key spelled as keys are, none it can't read, at most 8 to an action and 512
    /// actions, and `from` only while it's a DID.
    #[hegel::test(test_cases = 200)]
    fn prop_the_keybinds_record_fits_its_lexicon(tc: TestCase) {
        let binds = keybinds(&tc);
        let now = chrono::Utc::now();
        let record = binds.to_record(now);
        lexicon::validate(KEYBINDS, &record).unwrap();
        let (back, skipped) = Keybinds::from_record(&record).unwrap();
        assert_eq!(skipped, Vec::<String>::new());
        assert_eq!(back.keys, binds.readable());
        assert!(back.keys.len() <= 512 && back.keys.values().all(|k| k.len() <= 8));
        assert!(back.keys.keys().all(|a| a.len() <= 128 && binds.keys.contains_key(a)));
        for (action, keys) in &binds.keys {
            let read: Vec<String> = keys.iter().filter_map(|k| Key::parse(k).ok()).map(|k| k.to_string()).collect();
            let kept = back.keys.get(action);
            if action.len() <= 128 && back.keys.len() < 512 {
                let want = (!read.is_empty() || keys.is_empty()).then(|| read.into_iter().take(8).collect());
                assert_eq!(kept, want.as_ref(), "{action}");
            }
        }
        assert_eq!(back.from.as_deref(), binds.from.as_deref().filter(|d| d.starts_with("did:")));
        assert_eq!(back.updated_at, Some(crate::profile::timestamp(now)));
    }

    #[test]
    fn the_keybinds_record_fits_its_lexicon() {
        let mut binds = Keybinds::default();
        binds.keys.insert("playback.next".into(), vec!["]".into()]);
        binds.keys.insert("player.jobs".into(), vec![]);
        binds.keys.insert("deck.hot-cue".into(), ["&", "é", "\"", "'", "(", "-", "è", "_"].map(String::from).to_vec());
        let record = binds.to_record(chrono::Utc::now());
        lexicon::validate(KEYBINDS, &record).unwrap();
        assert_eq!(record["$type"], KEYBINDS);
        assert_eq!(record["bindings"][0], json!({ "action": "deck.hot-cue", "keys": binds.keys["deck.hot-cue"] }));
        assert_eq!(record["bindings"][1], json!({ "action": "playback.next", "keys": ["]"] }));
        assert_eq!(record["bindings"][2], json!({ "action": "player.jobs", "keys": [] }));
        assert!(record.get("from").is_none());
    }

    /// A record from another version keeps the actions this one doesn't know, and skips, saying
    /// so, what isn't a binding, an action listed again and keys it can't read (an action with
    /// only those, whole). Something that isn't a keybinds record is refused.
    #[test]
    fn a_record_with_unknown_ids_keeps_them() {
        let record = json!({
            "$type": KEYBINDS,
            "bindings": [
                { "action": "visuals.sparkles", "keys": ["shift-f3", "x"] },
                { "action": "playback.next", "keys": ["]"] },
                { "action": "playback.next", "keys": ["n"] },
                { "action": "player.jobs", "keys": [] },
                { "action": "deck.loop", "keys": ["hyper-l"] },
                { "action": "deck.single", "keys": [7] },
                { "keys": ["a"] },
                "loose"
            ],
            "from": "did:plc:abc",
            "updatedAt": "2026-11-20T18:00:00.000Z"
        });
        let (binds, skipped) = Keybinds::from_record(&record).unwrap();
        let mut want = IndexMap::new();
        want.insert("player.jobs".to_string(), Vec::<String>::new());
        want.insert("playback.next".into(), vec!["]".into()]);
        want.insert("visuals.sparkles".into(), vec!["x".into()]);
        assert_eq!(binds.keys, want);
        assert_eq!(binds.keys.keys().collect::<Vec<_>>(), ["playback.next", "player.jobs", "visuals.sparkles"]);
        assert_eq!(
            (binds.from.as_deref(), binds.updated_at.as_deref()),
            (Some("did:plc:abc"), Some("2026-11-20T18:00:00.000Z"))
        );
        assert_eq!(
            skipped,
            [
                "visuals.sparkles: \"shift-f3\"",
                "playback.next, listed again",
                "deck.loop: \"hyper-l\"",
                "deck.single: 7",
                "{\"keys\":[\"a\"]}",
                "\"loose\"",
            ]
        );
        let settings = json!({ "$type": lexicon::SETTINGS, "updatedAt": "2026-11-20T18:00:00.000Z" });
        let e = Keybinds::from_record(&settings).unwrap_err().to_string();
        assert!(
            e.contains("isn't a keybinds record (its $type is \"computer.localnetwork.locallibrary.settings\")"),
            "{e}"
        );
    }
}
