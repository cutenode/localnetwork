//! The tracklist plugin locallibrary ships reading rekordbox XML collections: `import` adds what
//! a collection says of a drive's tracks (keys, tempos, cues), and `collection export --from
//! tracklist:rekordbox-xml` builds a drive from one.

use std::path::Path;

use serde_json::Value;

use crate::support::{Env, path, recording};

/// `path` as rekordbox writes a `Location`.
fn location(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/").replace(' ', "%20");
    let p = if p.starts_with('/') { p } else { format!("/{p}") };
    format!("file://localhost{p}")
}

fn xml(tracks: &str, playlists: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<DJ_PLAYLISTS Version=\"1.0.0\">\n<COLLECTION>\n{tracks}</COLLECTION>\n<PLAYLISTS>{playlists}</PLAYLISTS>\n</DJ_PLAYLISTS>\n"
    )
}

/// A collection's cues land on the drive's tracks it names by file (from another computer's
/// folders), a hot cue loop in beats of the drive's grid; one track it names isn't on the drive,
/// and a cue out of range is left out, each said. Imported again, there's nothing left to add.
#[test]
fn a_rekordbox_xml_collection_adds_cues_to_the_drives_tracks() {
    let env = Env::ascii();
    let drive = env.demo("Demo");
    let d = path(&drive);
    let tracks = env.json(&["tracks", d]);
    let first = &tracks["tracks"][0];
    let file = first["file_path"].as_str().unwrap();
    let id = first["id"].to_string();
    let elsewhere = location(&Path::new("/Users/me/Music").join(file.trim_start_matches('/')));
    let collection = xml(
        &format!(
            r#"<TRACK TrackID="1" Name="whatever" Location="{elsewhere}" AverageBpm="128.00">
<TEMPO Inizio="0.025" Bpm="128.00" Metro="4/4" Battito="1"/>
<POSITION_MARK Name="Drop" Type="4" Start="60.025" End="61.900" Num="1"/>
<POSITION_MARK Name="" Type="0" Start="30.000" Num="-1"/>
<POSITION_MARK Name="" Type="0" Start="-5" Num="-1"/>
</TRACK>
<TRACK TrackID="2" Name="Gone" Artist="Nobody" Location="file://localhost/gone/x.mp3"/>
"#
        ),
        "",
    );
    let xml_file = env.outside("rekordbox.xml");
    std::fs::write(&xml_file, collection).unwrap();
    let import = ["import", path(&xml_file), d];
    let planned = env.json(&[&import[..], &["--dry-run"]].concat());
    let plan = &planned["plan"];
    assert_eq!((plan["tracks"]["entries"].as_u64(), plan["tracks"]["found"].as_u64()), (Some(2), Some(1)), "{plan}");
    let warnings: Vec<&str> = plan["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    let left_out = "rekordbox XML: keys, tempos, hot cues, memory cues and loops: 1 cue(s) (POSITION_MARK) left out";
    assert!(warnings.iter().any(|w| w.starts_with(left_out)), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("1 aren't on the drive")), "{warnings:?}");

    let out = env.ok(&[&["--yes"][..], &import].concat());
    assert!(out.contains("change(s) written"), "{out}");
    let cues = env.ok(&["cues", d, "--track", &id]);
    assert!(cues.contains("B  1:00.025  beat 129  loop 4 beats to 1:01.900  \"Drop\""), "{cues}");
    assert!(cues.contains("<>  0:30.000"), "{cues}");
    let again = env.json(&[&import[..], &["--dry-run"]].concat());
    assert_eq!(again["plan"]["edits"]["changes"], serde_json::json!([]), "{again}");
}

/// A drive is built from a collection: its tracks in the folder granted with their tags, key,
/// tempo, cues and beat grid, and its playlists; a track elsewhere is left out, and what the
/// file holds out of range is said.
#[test]
fn a_drive_is_built_from_a_rekordbox_xml_collection() {
    let env = Env::ascii();
    let music = env.outside("music");
    std::fs::create_dir_all(&music).unwrap();
    std::fs::write(music.join("glue.wav"), recording(1, 4.0, 44_100, 16, 0)).unwrap();
    std::fs::write(music.join("opus.wav"), recording(2, 4.0, 44_100, 16, 0)).unwrap();
    let (glue, opus) = (location(&music.join("glue.wav")), location(&music.join("opus.wav")));
    let collection = xml(
        &format!(
            r#"<TRACK TrackID="1" Name="Glue" Artist="Bicep" Genre="Electronica" Tonality="Fm" AverageBpm="120.00" TotalTime="4" Location="{glue}">
<TEMPO Inizio="0.000" Bpm="120.00" Metro="4/4" Battito="1"/>
<POSITION_MARK Name="Drop" Type="0" Start="1.000" Num="0" Red="255" Green="0" Blue="0"/>
<POSITION_MARK Name="" Type="4" Start="2.000" End="3.000" Num="-1"/>
<TEMPO Inizio="nope" Bpm="120"/>
</TRACK>
<TRACK TrackID="2" Name="Opus" Artist="Eric Prydz" Location="{opus}"/>
<TRACK TrackID="3" Name="Elsewhere" Location="file://localhost/elsewhere/x.wav"/>
"#
        ),
        r#"<NODE Type="0" Name="ROOT"><NODE Type="0" Name="Sets"><NODE Type="1" Name="Friday" KeyType="0"><TRACK Key="2"/><TRACK Key="1"/></NODE></NODE></NODE>"#,
    );
    let xml_file = env.outside("rekordbox.xml");
    std::fs::write(&xml_file, collection).unwrap();
    let stick = env.outside("stick");
    let option = format!("file={}", xml_file.display());
    let build = ["collection", "export", path(&stick), "--from", "tracklist:rekordbox-xml", "--grant", path(&music)];
    let build = [&build[..], &["--option", &option]].concat();
    let dry = env.run(&[&build[..], &["--dry-run"]].concat());
    let said = format!("{}{}", dry.out, dry.err);
    assert_eq!(dry.code, 0, "{said}");
    assert!(said.contains("2 track(s), 1 playlist(s)"), "{said}");
    let tempo = "rekordbox XML collection: 1 beat grid tempo(s) (TEMPO) left out: a time, tempo or beat out of range";
    assert!(said.contains(tempo), "{said}");
    assert!(said.contains("1 track(s) left out, e.g. ") && said.contains("x.wav"), "{said}");
    assert!(!said.contains("beat grid(s) left out") && !said.contains("cue(s) left out"), "{said}");

    let done = env.run(&[&["--yes"][..], &build].concat());
    assert_eq!(done.code, 0, "{}{}", done.out, done.err);
    let s = path(&stick);
    let tracks = env.json(&["tracks", s]);
    let glue = tracks["tracks"].as_array().unwrap().iter().find(|t| t["title"] == "Glue").unwrap();
    let fields = (glue["artist"].as_str(), glue["genre"].as_str(), glue["bpm"].as_f64());
    assert_eq!(fields, (Some("Bicep"), Some("Electronica"), Some(120.0)), "{glue}");
    assert!(!glue["key"].as_str().unwrap_or_default().is_empty(), "{glue}");
    let cues = env.ok(&["cues", s, "--track", &glue["id"].to_string()]);
    // On the grid the file's tempo makes: 120 BPM from 0, so 1 s is beat 3 and 2 s beat 5.
    assert!(cues.contains("A  0:01.000  beat 3  \"Drop\"  #e02823"), "{cues}");
    assert!(cues.contains("<>  0:02.000  beat 5  loop to 0:03.000"), "{cues}");
    let friday = env.json(&["tracks", s, "-p", "Sets/Friday"]);
    let order: Vec<&str> = friday["tracks"].as_array().unwrap().iter().filter_map(|t| t["title"].as_str()).collect();
    assert_eq!(order, ["Opus", "Glue"]);
}
