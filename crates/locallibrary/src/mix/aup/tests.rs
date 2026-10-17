use super::*;
use crate::masterdb::tests::scratch_db;

/// A project's binary XML the way Audacity writes it (UTF-32 names), titled REC001, with
/// `clips` of four samples on one track, each `(offset, trimLeft, block id)`.
fn doc(rate: f64, clips: &[(f64, f64, i64)]) -> Xml {
    let mut x = Xml::new(4);
    x.start("project").double("rate", rate);
    x.start("tags").start("tag").string("name", "TITLE").string("value", "REC001").end("tag").end("tags");
    x.start("wavetrack").double("gain", 1.0);
    for &(offset, trim, block) in clips {
        x.start("waveclip").double("offset", offset).double("trimLeft", trim).start("sequence");
        x.int(FT_LONG_LONG, "numsamples", 4);
        x.start("waveblock").int(FT_LONG_LONG, "start", 0).int(FT_LONG_LONG, "blockid", block).end("waveblock");
        x.end("sequence").end("waveclip");
    }
    x.end("wavetrack").end("project");
    x
}

#[test]
fn renders_clips_on_the_timeline() {
    let dir = tempfile::tempdir().unwrap();
    // Clip 1 at 0 s; clip 2 at 6 s (one sample trimmed off its front). Rate 2 Hz keeps it tiny.
    let x = doc(2.0, &[(0.0, 0.0, 1), (5.5, 0.5, 2)]);
    let ints: Vec<u8> = [1000i16, 2000, 3000, 4000].iter().flat_map(|f| f.to_le_bytes()).collect();
    let blocks = [(1, FLOAT, floats(&[0.1, 0.2, 0.3, 0.4])), (2, INT16, ints)];
    let path = project(dir.path(), "project", Some(&x), &blocks);

    let p = Project::open(&path).unwrap();
    assert_eq!(p.tag("title"), Some("REC001"));
    assert_eq!(p.duration_secs(), 7.5);
    let mut out = Vec::new();
    p.render(2, &Ctx::silent(), &mut |s| out.extend_from_slice(s)).unwrap();
    let mut expected = vec![0.0f32; 15];
    expected[..4].copy_from_slice(&[0.1, 0.2, 0.3, 0.4]);
    expected[12..].copy_from_slice(&[2000.0 / 32768.0, 3000.0 / 32768.0, 4000.0 / 32768.0]);
    assert_eq!(out.len(), expected.len());
    for (i, (o, e)) in out.iter().zip(&expected).enumerate() {
        assert!((o - e).abs() < 1e-6, "sample {i}: {o} vs {e}");
    }
}

/// A rate under 1 Hz made the resampler step by 0 and render for ever; a document nesting
/// 300,000 elements overflowed the stack; a view could stand in for the project table.
#[test]
fn odd_rates_deep_nesting_and_views_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let open = |name: &str, x: Xml| {
        let path = dir.path().join(name);
        let db = scratch_db(&path);
        let table = if name == "view.aup3" { "stored" } else { "project" };
        db.execute_batch(&format!("CREATE TABLE {table}(id INTEGER PRIMARY KEY, dict BLOB, doc BLOB);")).unwrap();
        db.execute(&format!("INSERT INTO {table} VALUES (1, ?1, ?2)"), (x.dict, x.doc)).unwrap();
        if table == "stored" {
            db.execute_batch("CREATE VIEW project AS SELECT * FROM stored").unwrap();
        }
        drop(db);
        Project::open(&path).map(|_| ()).unwrap_err().to_string()
    };
    assert!(open("slow.aup3", doc(0.3, &[])).contains("sample rate of 0.3 Hz"));
    assert!(open("fast.aup3", doc(f64::INFINITY, &[])).contains("sample rate of inf Hz"));
    let mut deep = doc(44_100.0, &[]);
    for _ in 0..=MAX_DEPTH {
        // Name 0: `project`.
        deep.doc.extend([FT_START_TAG, 0, 0]);
    }
    assert!(open("deep.aup3", deep).contains("nested more than 64 deep"));
    assert!(open("view.aup3", doc(44_100.0, &[])).contains("it has a view"));
}

/// Binary XML written token by token, in any of the character sizes Audacity uses: each name
/// goes into the dictionary the first time it's used.
pub(crate) struct Xml {
    char_size: u8,
    dict: Vec<u8>,
    doc: Vec<u8>,
    names: Vec<String>,
}

impl Xml {
    pub(crate) fn new(char_size: u8) -> Xml {
        Xml { char_size, dict: vec![FT_CHAR_SIZE, char_size], doc: Vec::new(), names: Vec::new() }
    }

    fn text(&self, s: &str) -> Vec<u8> {
        match self.char_size {
            4 => s.chars().flat_map(|c| (c as u32).to_le_bytes()).collect(),
            2 => s.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            _ => s.as_bytes().to_vec(),
        }
    }

    fn id(&mut self, name: &str) -> [u8; 2] {
        let id = match self.names.iter().position(|n| n == name) {
            Some(i) => i as u16,
            None => {
                let id = self.names.len() as u16;
                let text = self.text(name);
                self.dict.push(FT_NAME);
                self.dict.extend(id.to_le_bytes());
                self.dict.extend((text.len() as u16).to_le_bytes());
                self.dict.extend(text);
                self.names.push(name.into());
                id
            }
        };
        id.to_le_bytes()
    }

    /// A token naming `name`, then `value`'s bytes.
    fn token(&mut self, op: u8, name: &str, value: &[u8]) -> &mut Xml {
        let id = self.id(name);
        self.doc.push(op);
        self.doc.extend(id);
        self.doc.extend(value);
        self
    }

    pub(crate) fn start(&mut self, name: &str) -> &mut Xml {
        self.token(FT_START_TAG, name, &[])
    }

    pub(crate) fn end(&mut self, name: &str) -> &mut Xml {
        self.token(FT_END_TAG, name, &[])
    }

    pub(crate) fn string(&mut self, name: &str, v: &str) -> &mut Xml {
        let text = self.text(v);
        self.token(FT_STRING, name, &[&(text.len() as i32).to_le_bytes()[..], &text].concat())
    }

    /// `name` as a whole number, in whichever of the integer tokens `op` is.
    fn int(&mut self, op: u8, name: &str, v: i64) -> &mut Xml {
        let bytes = match op {
            FT_BOOL => vec![v as u8],
            FT_LONG_LONG => v.to_le_bytes().to_vec(),
            FT_SIZE_T => (v as u32).to_le_bytes().to_vec(),
            _ => (v as i32).to_le_bytes().to_vec(),
        };
        self.token(op, name, &bytes)
    }

    fn float(&mut self, name: &str, v: f32) -> &mut Xml {
        self.token(FT_FLOAT, name, &[&v.to_le_bytes()[..], &7i32.to_le_bytes()].concat())
    }

    pub(crate) fn double(&mut self, name: &str, v: f64) -> &mut Xml {
        self.token(FT_DOUBLE, name, &[&v.to_le_bytes()[..], &15i32.to_le_bytes()].concat())
    }

    /// A clip at `offset` seconds of `blocks`, each `(first sample, block id)`.
    pub(crate) fn clip(&mut self, offset: f64, samples: i64, blocks: &[(i64, i64)]) -> &mut Xml {
        self.start("waveclip").double("offset", offset).start("sequence");
        self.int(FT_LONG_LONG, "numsamples", samples);
        for &(start, id) in blocks {
            self.start("waveblock").int(FT_LONG_LONG, "start", start).int(FT_LONG_LONG, "blockid", id);
            self.end("waveblock");
        }
        self.end("sequence")
    }
}

/// A project file, in Audacity's tables, holding `xml` (in `table`, `project` or
/// `autosave`) and the sample blocks, each `(id, format, samples)`.
pub(crate) fn project(dir: &Path, table: &str, xml: Option<&Xml>, blocks: &[(i64, i64, Vec<u8>)]) -> PathBuf {
    let path = dir.join(format!("{}.aup3", std::fs::read_dir(dir).unwrap().count()));
    let db = scratch_db(&path);
    db.execute_batch(
        "CREATE TABLE project(id INTEGER PRIMARY KEY, dict BLOB, doc BLOB);
             CREATE TABLE autosave(id INTEGER PRIMARY KEY, dict BLOB, doc BLOB);
             CREATE TABLE sampleblocks(blockid INTEGER PRIMARY KEY AUTOINCREMENT, sampleformat INTEGER,
               summin REAL, summax REAL, sumrms REAL, summary256 BLOB, summary64k BLOB, samples BLOB);",
    )
    .unwrap();
    if let Some(x) = xml {
        db.execute(&format!("INSERT INTO {table} VALUES (1, ?1, ?2)"), (&x.dict, &x.doc)).unwrap();
    }
    for (id, format, samples) in blocks {
        db.execute(
            "INSERT INTO sampleblocks (blockid, sampleformat, samples) VALUES (?1, ?2, ?3)",
            (id, format, samples),
        )
        .unwrap();
    }
    path
}

pub(crate) fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn rendered(path: &Path, rate: u32) -> Vec<f32> {
    let mut out = Vec::new();
    Project::open(path).unwrap().render(rate, &Ctx::silent(), &mut |s| out.extend_from_slice(s)).unwrap();
    out
}

/// Audacity writes a value with whichever token fits it (an int, a size, a string...), and
/// the reader takes any of them for any attribute, in UTF-16 as in UTF-32. Tokens it has no
/// use for (raw data, pushes and pops) are skipped.
#[test]
fn every_token_type_reads_as_audacity_wrote_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut x = Xml::new(2);
    x.start("project").int(FT_SIZE_T, "rate", 4);
    x.start("tags").start("tag").string("name", "YEAR").int(FT_INT, "value", 2024).end("tag");
    x.start("tag").string("name", "ARTIST").string("value", "Ärm").end("tag");
    x.start("tag").string("name", "BPM").double("value", 128.5).end("tag").end("tags");
    x.start("labeltrack").start("label").double("t", 0.5).string("title", "intro").end("label").end("labeltrack");
    x.doc.extend([FT_PUSH, FT_DATA, 3, 0, 0, 0, 1, 2, 3, FT_RAW, 1, 0, 0, 0, 9, FT_POP]);
    x.start("wavetrack").float("gain", 0.5).int(FT_BOOL, "mute", 0).string("solo", "0").string("rate", "4");
    x.start("waveclip").int(FT_INT, "offset", 1).string("trimLeft", "0").start("sequence");
    x.float("numsamples", 6.0);
    // A block a sample, so more blocks are read than are kept at once.
    for i in 0..6 {
        x.start("waveblock").int(FT_SIZE_T, "start", i).int(FT_LONG, "blockid", i + 1).end("waveblock");
    }
    x.end("sequence").end("waveclip").end("wavetrack").end("project");
    // 24-bit samples, stored in four bytes: k/8 each.
    let blocks: Vec<(i64, i64, Vec<u8>)> =
        (1..=6).map(|k| (k, INT24, ((k as i32) << 20).to_le_bytes().to_vec())).collect();
    let path = project(dir.path(), "project", Some(&x), &blocks);

    let p = Project::open(&path).unwrap();
    let tags: Vec<(&str, &str)> = p.tags.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
    assert_eq!(tags, [("YEAR", "2024"), ("ARTIST", "Ärm"), ("BPM", "128.5")]);
    assert_eq!(p.labels, [Label { start: 0.5, end: 0.5, title: "intro".into() }]);
    assert_eq!(p.duration_secs(), 1.0 + 6.0 / 4.0);
    let expected = [0.0, 0.0, 0.0, 0.0, 0.0625, 0.125, 0.1875, 0.25, 0.3125, 0.375];
    assert_eq!(rendered(&path, 4), expected);
}

/// A project Audacity never saved has only its autosave, and one cut short (Audacity
/// stopped mid-write) still opens with what's there. Negative block ids are silence.
#[test]
fn autosaved_and_cut_short_projects_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut x = Xml::new(1);
    x.start("project").double("rate", 2.0).start("wavetrack").clip(0.0, 4, &[(0, -2), (2, 1)]);
    let path = project(dir.path(), "autosave", Some(&x), &[(1, FLOAT, floats(&[0.25, 0.5]))]);
    assert_eq!(rendered(&path, 2), [0.0, 0.0, 0.25, 0.5]);

    let nothing = project(dir.path(), "project", None, &[]);
    assert!(Project::open(&nothing).unwrap_err().to_string().contains("the project is empty"));
}

/// Muted tracks are left out, and while any track is soloed only soloed ones are heard. A
/// stereo clip's second channel, nested in the first since Audacity 3.4, is heard too.
#[test]
fn mute_solo_and_nested_channels_are_honoured() {
    let dir = tempfile::tempdir().unwrap();
    let mut x = Xml::new(4);
    x.start("project").double("rate", 2.0);
    x.start("wavetrack").int(FT_BOOL, "mute", 1).int(FT_BOOL, "solo", 1).clip(0.0, 2, &[(0, 1)]);
    x.end("waveclip").end("wavetrack");
    x.start("wavetrack").int(FT_BOOL, "solo", 1).clip(0.0, 2, &[(0, -2)]).clip(0.0, 2, &[(0, 2)]);
    x.end("waveclip").end("waveclip").end("wavetrack");
    x.start("wavetrack").clip(0.0, 2, &[(0, 1)]).end("waveclip").end("wavetrack").end("project");
    let loud = floats(&[0.9, 0.9]);
    let path = project(dir.path(), "project", Some(&x), &[(1, FLOAT, loud), (2, FLOAT, floats(&[0.25, 0.5]))]);
    assert_eq!(rendered(&path, 2), [0.25, 0.5]);
}

#[test]
fn damaged_projects_say_what_is_wrong() {
    let dir = tempfile::tempdir().unwrap();
    let error = |x: &Xml, blocks: &[(i64, i64, Vec<u8>)]| {
        let path = project(dir.path(), "project", Some(x), blocks);
        match Project::open(&path) {
            Ok(p) => p.render(2, &Ctx::silent(), &mut |_| {}).unwrap_err().to_string(),
            Err(e) => e.to_string(),
        }
    };
    let mut unknown = Xml::new(1);
    unknown.start("project").doc.push(200);
    assert!(error(&unknown, &[]).contains("project document: unknown token 200 at byte 3"));
    let mut silent = Xml::new(1);
    silent.start("project").double("rate", 2.0).start("labeltrack").end("labeltrack").end("project");
    assert!(error(&silent, &[]).contains("the project has no audible audio"));
    let mut odd = Xml::new(1);
    odd.start("project").double("rate", 2.0).start("wavetrack").clip(0.0, 2, &[(0, 1)]);
    assert!(error(&odd, &[(1, 0x99, vec![0; 8])]).contains("unknown sample format 0x99"));
    assert!(error(&odd, &[]).contains("sample block 1"));
}

use hegel::generators as gs;
use hegel::{Generator, TestCase};

/// A clip as the property places it: everything in whole samples at the project's rate.
#[derive(Debug)]
struct ClipModel {
    offset: u64,
    trim_left: u64,
    trim_right: u64,
    stretch: u64,
    /// The sequence, block by block: (samples, whether it's stored or silence).
    blocks: Vec<(Vec<f32>, bool)>,
}

impl ClipModel {
    fn numsamples(&self) -> u64 {
        self.blocks.iter().map(|b| b.0.len() as u64).sum()
    }

    /// The sequence's samples (silence as zeros).
    fn samples(&self) -> Vec<f32> {
        self.blocks.iter().flat_map(|(s, stored)| s.iter().map(move |v| if *stored { *v } else { 0.0 })).collect()
    }

    /// The stretch of the sequence that plays: first sample and how many.
    fn played(&self) -> (u64, u64) {
        let n = self.numsamples();
        let first = self.trim_left.min(n);
        (first, n.saturating_sub(self.trim_right).max(first) - first)
    }
}

#[hegel::test_helper]
fn clip_model(tc: &TestCase) -> ClipModel {
    // Exact in every sample format.
    let level = gs::sampled_from(vec![0.0f32, 0.25, -0.5, 0.125, 0.5]);
    let blocks = tc.draw(gs::vecs(gs::tuples!(gs::vecs(level).min_size(1).max_size(5), gs::booleans())).max_size(4));
    ClipModel {
        offset: tc.draw(gs::integers::<u64>().max_value(12)),
        trim_left: tc.draw(gs::integers::<u64>().max_value(3)),
        trim_right: tc.draw(gs::integers::<u64>().max_value(3)),
        stretch: tc.draw(gs::integers::<u64>().min_value(1).max_value(2)),
        blocks,
    }
}

/// `v` written with any of the tokens Audacity might use for it.
#[hegel::test_helper]
fn number(tc: &TestCase, x: &mut Xml, name: &str, v: f64) {
    match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => x.float(name, v as f32),
        1 => x.string(name, &v.to_string()),
        2 if v.fract() == 0.0 => x.int(FT_INT, name, v as i64),
        3 if v.fract() == 0.0 && v >= 0.0 => x.int(FT_SIZE_T, name, v as i64),
        4 if v == 0.0 || v == 1.0 => x.int(FT_BOOL, name, v as i64),
        _ => x.double(name, v),
    };
    // Tokens a reader has no use for, which Audacity writes between others.
    match tc.draw(gs::integers::<u8>().max_value(6)) {
        0 => x.doc.extend([FT_PUSH, FT_POP]),
        1 => x.doc.extend([FT_DATA, 2, 0, 0, 0, 7, 7]),
        2 => x.doc.extend([FT_RAW, 1, 0, 0, 0, 9]),
        _ => {}
    }
}

#[hegel::test_helper]
fn flag(tc: &TestCase, x: &mut Xml, name: &str, on: bool) {
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => x.int(FT_BOOL, name, i64::from(on)),
        1 => x.string(name, if on { "1" } else { "0" }),
        _ => x.int(FT_INT, name, i64::from(on)),
    };
}

/// A clip's element up to the end of its sequence, its stored blocks added to `blocks` in
/// any of the sample formats.
#[hegel::test_helper]
fn write_clip(tc: &TestCase, x: &mut Xml, c: &ClipModel, rate: f64, blocks: &mut Vec<(i64, i64, Vec<u8>)>) {
    x.start("waveclip");
    number(tc, x, "offset", c.offset as f64 / rate);
    number(tc, x, "trimLeft", c.trim_left as f64 / rate);
    number(tc, x, "trimRight", c.trim_right as f64 / rate);
    number(tc, x, "clipStretchRatio", c.stretch as f64);
    x.start("sequence");
    number(tc, x, "numsamples", c.numsamples() as f64);
    let mut at = 0;
    for (samples, stored) in &c.blocks {
        let id = if *stored {
            let (format, bytes): (i64, Vec<u8>) = match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => (INT16, samples.iter().flat_map(|v| ((v * 32_768.0) as i16).to_le_bytes()).collect()),
                1 => (INT24, samples.iter().flat_map(|v| ((v * 8_388_608.0) as i32).to_le_bytes()).collect()),
                _ => (FLOAT, floats(samples)),
            };
            blocks.push((blocks.len() as i64 + 1, format, bytes));
            blocks.len() as i64
        } else {
            -(samples.len() as i64)
        };
        x.start("waveblock");
        number(tc, x, "start", at as f64);
        x.int(FT_LONG_LONG, "blockid", id).end("waveblock");
        at += samples.len();
    }
    x.end("sequence");
}

/// Any project, in any of the character sizes, value tokens and sample formats, saved or
/// only autosaved, renders to what its clips hold where they sit on the timeline: trimmed,
/// stretched, at each track's gain, silent blocks silent, a nested channel heard like any
/// other, every audible track mixed down equally, muted ones (and with a solo, unsoloed ones)
/// left out. It reads back its tags and labels, and is as long as its last clip.
#[hegel::test(test_cases = 40)]
fn projects_render_their_clips_where_they_sit(tc: TestCase) {
    let rate = tc.draw(gs::sampled_from(vec![2.0f64, 4.0, 8.0, 16.0]));
    let mut x = Xml::new(tc.draw(gs::sampled_from(vec![1u8, 2, 4])));
    x.start("project").double("rate", rate);
    let tags: Vec<(String, String)> = tc.draw(
        gs::vecs(gs::tuples!(
            gs::sampled_from(vec!["TITLE".to_string(), "ARTIST".into(), "GENRE".into()]),
            gs::text().alphabet("Live ät 2024").min_size(1).max_size(8)
        ))
        .max_size(3),
    );
    x.start("tags");
    for (name, value) in &tags {
        x.start("tag").string("name", name).string("value", value).end("tag");
    }
    // A number, read back as its text whichever token it's written as.
    let bpm = tc.draw(gs::optional(gs::sampled_from(vec![120.0, 128.5])));
    if let Some(bpm) = bpm {
        x.start("tag").string("name", "BPM");
        number(&tc, &mut x, "value", bpm);
        x.end("tag");
    }
    x.end("tags");
    let tags: Vec<(String, String)> = tags.into_iter().chain(bpm.map(|b| ("BPM".to_string(), b.to_string()))).collect();
    let labels =
        tc.draw(gs::vecs(gs::tuples!(gs::integers::<u8>(), gs::text().alphabet("drop ïn").max_size(6))).max_size(2));
    x.start("labeltrack");
    for (t, title) in &labels {
        x.start("label")
            .double("t", f64::from(*t))
            .double("t1", f64::from(*t) + 0.5)
            .string("title", title)
            .end("label");
    }
    x.end("labeltrack");

    let tracks: Vec<(f64, bool, bool, Vec<ClipModel>)> = (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|_| {
            let gain = tc.draw(gs::sampled_from(vec![0.5, 1.0, 2.0]));
            let (mute, solo) = (tc.draw(gs::weighted_booleans(0.2)), tc.draw(gs::weighted_booleans(0.2)));
            let clips = (0..tc.draw(gs::integers::<usize>().max_value(3))).map(|_| clip_model(&tc)).collect();
            (gain, mute, solo, clips)
        })
        .collect();
    let mut blocks: Vec<(i64, i64, Vec<u8>)> = Vec::new();
    for (gain, mute, solo, clips) in &tracks {
        x.start("wavetrack");
        number(&tc, &mut x, "gain", *gain);
        flag(&tc, &mut x, "mute", *mute);
        flag(&tc, &mut x, "solo", *solo);
        let mut clips = clips.iter().peekable();
        while let Some(c) = clips.next() {
            write_clip(&tc, &mut x, c, rate, &mut blocks);
            // Audacity 3.4+ nests a stereo clip's second channel in the first.
            if clips.peek().is_some() && tc.draw(gs::booleans()) {
                write_clip(&tc, &mut x, clips.next().unwrap(), rate, &mut blocks);
                x.end("waveclip");
            }
            x.end("waveclip");
        }
        x.end("wavetrack");
    }
    x.end("project");
    let dir = tempfile::tempdir().unwrap();
    let table = tc.draw(gs::sampled_from(vec!["project", "autosave"]));
    let path = project(dir.path(), table, Some(&x), &blocks);

    // What it should sound like, worked out sample by sample.
    let any_solo = tracks.iter().any(|t| t.2);
    let heard: Vec<&(f64, bool, bool, Vec<ClipModel>)> = tracks.iter().filter(|t| !t.1 && (!any_solo || t.2)).collect();
    let audible = heard.iter().filter(|t| !t.3.is_empty()).count();
    let opened = Project::open(&path);
    if audible == 0 {
        let err = opened.unwrap_err().to_string();
        return assert!(err.contains("no audible audio"), "{err}");
    }
    let p = opened.unwrap();
    assert_eq!(p.tags, tags);
    for (name, _) in &tags {
        let first = tags.iter().find(|t| &t.0 == name).map(|t| t.1.as_str());
        assert_eq!(p.tag(&name.to_lowercase()), first);
    }
    let read: Vec<(u8, String)> = p.labels.iter().map(|l| (l.start as u8, l.title.clone())).collect();
    assert_eq!(read, labels);
    assert!(p.labels.iter().all(|l| l.end == l.start + 0.5));
    let ends = heard.iter().flat_map(|t| &t.3).map(|c| c.offset + c.trim_left * c.stretch + c.played().1 * c.stretch);
    let total = ends.max().unwrap_or(0) as usize;
    assert_eq!(p.duration_secs(), total as f64 / rate);
    let mut expected = vec![0.0f32; total];
    for (gain, _, _, clips) in &heard {
        for c in clips {
            let (first, len) = c.played();
            let samples = c.samples();
            let start = c.offset + c.trim_left * c.stretch;
            for t in 0..len * c.stretch {
                expected[(start + t) as usize] +=
                    samples[(first + t / c.stretch) as usize] * (*gain as f32 / audible as f32);
            }
        }
    }
    let mut out = Vec::new();
    let format = p.render(rate as u32, &Ctx::silent(), &mut |s| out.extend_from_slice(s)).unwrap();
    assert_eq!((format.sample_rate, format.channels as usize), (rate as u32, heard.len()));
    assert_eq!(format.duration_secs, total as f64 / rate);
    assert_eq!(out.len(), expected.len());
    for (i, (o, e)) in out.iter().zip(&expected).enumerate() {
        assert!((o - e).abs() < 1e-6, "sample {i}: {o} for {e}");
    }
}

/// Bytes from anywhere as the project's dictionary and document never panic the reader:
/// it opens what it can make sense of, and otherwise says what's wrong with that file.
#[hegel::test(test_cases = 50)]
fn damaged_documents_are_errors_not_crashes(tc: TestCase) {
    let mut x = Xml::new(tc.draw(gs::sampled_from(vec![1u8, 2, 4])));
    x.start("project").double("rate", 4.0).start("wavetrack").clip(0.0, 4, &[(0, 1)]);
    x.end("waveclip").end("wavetrack").end("project");
    let (mut dict, mut doc) = (x.dict.clone(), x.doc.clone());
    for bytes in [&mut dict, &mut doc] {
        let flips = tc.draw(gs::vecs(gs::tuples!(gs::integers::<usize>(), gs::integers::<u8>())).max_size(4));
        for (at, v) in flips {
            let n = bytes.len();
            bytes[at % n] = v;
        }
        let keep = tc.draw(gs::integers::<usize>().max_value(bytes.len()));
        bytes.truncate(keep);
    }
    let dir = tempfile::tempdir().unwrap();
    let damaged = Xml { dict, doc, ..x };
    let path = project(dir.path(), "project", Some(&damaged), &[(1, FLOAT, floats(&[0.5; 4]))]);
    match Project::open(&path) {
        Ok(p) => {
            assert!(p.duration_secs() >= 0.0 && RATES.contains(&p.rate), "{p:?}");
            if let Err(e) = p.render(4, &Ctx::silent(), &mut |_| {}) {
                assert!(matches!(&e, Error::Parse { path: at, .. } if *at == path), "{e}");
            }
        }
        Err(e) => assert!(matches!(&e, Error::Parse { path: at, .. } if *at == path), "{e}"),
    }
}

/// A project's rate is its own when it's one Audacity offers (up to 1 MHz), 44.1 kHz when it
/// gives none, and anything else (under 1 Hz, which would never render, or past 1 MHz) is
/// refused.
#[hegel::test(test_cases = 40)]
fn only_real_rates_are_taken(tc: TestCase) {
    let rate = tc.draw(hegel::one_of!(
        gs::floats::<f64>().min_value(-10.0).max_value(10.0),
        gs::floats::<f64>().min_value(1.0).max_value(2e6),
        gs::sampled_from(vec![f64::INFINITY, 0.0])
    ));
    let mut x = Xml::new(1);
    x.start("project").double("rate", rate).start("wavetrack").clip(0.0, 2, &[(0, 1)]);
    x.end("waveclip").end("wavetrack").end("project");
    let dir = tempfile::tempdir().unwrap();
    let path = project(dir.path(), "project", Some(&x), &[(1, FLOAT, floats(&[0.5, 0.5]))]);
    let taken = if rate > 0.0 { rate } else { 44_100.0 };
    match Project::open(&path) {
        Ok(p) => assert!(RATES.contains(&taken) && p.rate == taken, "{rate}: {}", p.rate),
        Err(e) => assert!(!RATES.contains(&taken) && e.to_string().contains("sample rate of"), "{rate}: {e}"),
    }
}

/// A project Audacity stopped writing after its last clip, missing any number of its closing
/// tags, renders exactly as the whole one does: each track's blocks one after another (more of
/// them than are kept loaded at once), the tracks mixed down equally.
#[hegel::test(test_cases = 30)]
fn projects_missing_their_closing_tags_render_whole(tc: TestCase) {
    let level = gs::sampled_from(vec![0.0f32, 0.25, -0.5, 0.125, 0.5]);
    let tracks: Vec<Vec<f32>> = tc.draw(gs::vecs(gs::vecs(level).min_size(1).max_size(16)).min_size(1).max_size(2));
    let mut x = Xml::new(tc.draw(gs::sampled_from(vec![1u8, 2, 4])));
    x.start("project").double("rate", 4.0);
    let mut blocks = Vec::new();
    for samples in &tracks {
        // A block a sample.
        let ids: Vec<(i64, i64)> = (0..samples.len() as i64).map(|i| (i, blocks.len() as i64 + i + 1)).collect();
        x.start("wavetrack").clip(0.0, samples.len() as i64, &ids);
        blocks.extend(ids.iter().zip(samples).map(|(&(_, id), &s)| (id, FLOAT, floats(&[s]))));
        x.end("waveclip").end("wavetrack");
    }
    x.end("project");
    let dir = tempfile::tempdir().unwrap();
    let whole = project(dir.path(), "project", Some(&x), &blocks);
    // The closing tags of the last clip, its track and the project.
    let cut = tc.draw(gs::integers::<usize>().max_value(3));
    x.doc.truncate(x.doc.len() - 3 * cut);
    let short = project(dir.path(), "project", Some(&x), &blocks);

    let total = tracks.iter().map(Vec::len).max().unwrap();
    let mut expected = vec![0.0f32; total];
    for samples in &tracks {
        for (e, s) in expected.iter_mut().zip(samples) {
            *e += s / tracks.len() as f32;
        }
    }
    assert_eq!(rendered(&short, 4), expected);
    assert_eq!(rendered(&whole, 4), expected);
}

/// A flag, a whole number or a fraction written as a tag's value with its own token reads back
/// as the same text as when written as a string.
#[hegel::test(test_cases = 60)]
fn tag_values_read_the_same_in_any_token(tc: TestCase) {
    let (op, v, text): (u8, f64, String) = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => {
            let b = tc.draw(gs::booleans());
            (FT_BOOL, f64::from(u8::from(b)), b.to_string())
        }
        1 => {
            let i = tc.draw(gs::integers::<i32>());
            (FT_INT, f64::from(i), i.to_string())
        }
        _ => {
            let f = tc.draw(gs::sampled_from(vec![0.5, 128.25, -3.0, 1e-3]));
            (FT_DOUBLE, f, f.to_string())
        }
    };
    let tag_of = |typed: bool| {
        let mut x = Xml::new(1);
        x.start("project").double("rate", 2.0).start("tags").start("tag").string("name", "MOOD");
        match (typed, op) {
            (false, _) => x.string("value", &text),
            (true, FT_DOUBLE) => x.double("value", v),
            (true, op) => x.int(op, "value", v as i64),
        };
        x.end("tag").end("tags").start("wavetrack").clip(0.0, 2, &[(0, 1)]).end("waveclip").end("wavetrack");
        x.end("project");
        let dir = tempfile::tempdir().unwrap();
        let path = project(dir.path(), "project", Some(&x), &[(1, FLOAT, floats(&[0.5, 0.5]))]);
        Project::open(&path).unwrap().tag("mood").map(String::from)
    };
    assert_eq!(tag_of(true), tag_of(false));
    assert_eq!(tag_of(false).as_deref(), Some(text.as_str()));
}

/// What can't be a project Audacity wrote is refused for exactly that: elements nested deeper
/// than [`MAX_DEPTH`], a view standing in for the project's table, a sample block in a format
/// Audacity has no name for. Nesting within the limit is no reason to refuse.
#[hegel::test(test_cases = 60)]
fn odd_projects_are_refused_for_what_is_odd(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut x = Xml::new(1);
    x.start("project").double("rate", 2.0).start("wavetrack").clip(0.0, 2, &[(0, 1)]);
    x.end("waveclip").end("wavetrack");
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => {
            let depth = tc.draw(hegel::one_of!(
                gs::integers::<usize>().min_value(1).max_value(MAX_DEPTH + 3),
                gs::integers::<usize>().min_value(MAX_DEPTH - 2).max_value(MAX_DEPTH + 3),
            ));
            // `project` and `depth` elements in it, none of them closed.
            for _ in 1..depth {
                x.start("labeltrack");
            }
            let path = project(dir.path(), "project", Some(&x), &[(1, FLOAT, floats(&[0.5, 0.5]))]);
            let opened = Project::open(&path).map(|_| ()).map_err(|e| e.to_string());
            match opened {
                Err(e) if e.contains("nested more than 64 deep") => assert!(depth > MAX_DEPTH, "{depth}"),
                other => assert!(depth <= MAX_DEPTH, "{depth}: {other:?}"),
            }
        }
        1 => {
            let table = tc.draw(gs::sampled_from(vec!["project", "autosave"]));
            let path = dir.path().join("view.aup3");
            let db = scratch_db(&path);
            db.execute_batch("CREATE TABLE stored(id INTEGER PRIMARY KEY, dict BLOB, doc BLOB);").unwrap();
            db.execute("INSERT INTO stored VALUES (1, ?1, ?2)", (&x.dict, &x.doc)).unwrap();
            db.execute_batch(&format!("CREATE VIEW {table} AS SELECT * FROM stored")).unwrap();
            drop(db);
            let e = Project::open(&path).unwrap_err();
            assert!(
                matches!(&e, Error::Parse { message, .. } if message.contains(&format!("it has a view {table}"))),
                "{e}"
            );
        }
        _ => {
            let format = tc.draw(gs::integers::<i64>().filter(|f| ![INT16, INT24, FLOAT].contains(f)));
            let path = project(dir.path(), "project", Some(&x), &[(1, format, floats(&[0.5, 0.5]))]);
            let e = Project::open(&path).unwrap().render(2, &Ctx::silent(), &mut |_| {}).unwrap_err();
            assert!(e.to_string().contains(&format!("unknown sample format {format:#x}")), "{e}");
        }
    }
}
