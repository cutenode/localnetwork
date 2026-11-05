//! Exporters of sets and mixes: the request names one of what the exporter's `source` lists,
//! found by the host, and the plugin reads it.

use locallibrary::Device;
use locallibrary::mix::report;
use locallibrary::sets::{Set, SetBook, SetEntry};

use super::*;

/// The echo manifest granting `sets:read`, with a set exporter (`sheet`) and a mix exporter
/// (`mixsheet`), each writing a tracklist, and one writing out any of the three (`anysheet`).
fn tracklists() -> String {
    granting(&["sets:read"]).replace(
        "[[contributes.providers]]",
        "[[contributes.exporters]]\nformat = \"sheet\"\ntitle = \"Set sheet\"\nextension = \"txt\"\nsource = \"set\"\n\n\
         [[contributes.exporters]]\nformat = \"mixsheet\"\ntitle = \"Mix sheet\"\nextension = \"txt\"\n\
         source = \"mix\"\n\n[[contributes.exporters]]\nformat = \"anysheet\"\ntitle = \"Any sheet\"\n\
         extension = \"txt\"\nsource = [\"drive\", \"set\", \"mix\"]\n\n[[contributes.providers]]",
    )
}

impl Fixture {
    fn export(&self, exporter: &str, source: ExportSource) -> Result<locallibrary::op::Planned<PluginExport>> {
        let output = self._dir.path().join("out").join(format!("{exporter}.txt"));
        self.plugins.export_plan("echo", exporter, source, &output, Value::Null, Some(&self.store), &Ctx::silent())
    }
}

use locallibrary::plugin::PluginExport;
use loclib_plugin::ExportSource;

/// A set exporter is given the set (by its id, however it was named) and the drive, when there's
/// one; a mix exporter the mix kept in the store. Each writes what it read out.
#[test]
fn a_set_or_a_mix_is_written_out() {
    let f = Fixture::new(|dir| vec![spec(dir, &tracklists(), &[])], Options::default());
    let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
    let set = Set {
        id: "3m2friday2222".into(),
        title: Some("Friday".into()),
        date,
        tracks: vec![SetEntry::added("Floorplan - Never Grow Old")],
        ..Set::default()
    };
    SetBook { sets: vec![set], ..SetBook::default() }.save(&f.store).unwrap();
    let source =
        |set: &str, drive: &str| ExportSource { set: Some(set.into()), drive: drive.into(), ..ExportSource::default() };
    let planned = f.export("sheet", source("Friday", "")).unwrap();
    assert_eq!(planned.notes, ["drive: ", "set: 3m2friday2222"]);
    let id = Device::open(&f.drive).unwrap().id().unwrap().to_string();
    let planned = f.export("sheet", source("last", "USB")).unwrap();
    assert_eq!(planned.notes, [format!("drive: {id}"), "set: 3m2friday2222".into()]);
    assert_eq!(&*planned.operation, "echo.export.sheet");
    planned.apply(Some(&f.store), &Ctx::silent()).unwrap();
    // The test's reads answer `sets.get` with a set of no tracks.
    assert_eq!(std::fs::read_to_string(&planned.output).unwrap(), "");

    let tracks = common::library(&f.drive).tracks;
    let mix = report::save(&f.store, &crate::sets::report(&[(&tracks[0], 0.0, 0.9)], None)).unwrap();
    let planned = f.export("mixsheet", ExportSource { mix: Some(mix.clone()), ..ExportSource::default() }).unwrap();
    assert_eq!(planned.notes, ["drive: ".to_string(), format!("mix: {mix}")]);
    planned.apply(Some(&f.store), &Ctx::silent()).unwrap();
    assert_eq!(std::fs::read_to_string(&planned.output).unwrap(), "A - B\n", "as the test's reads answer");
}

/// What an export names must be what its exporter writes out, and there.
#[test]
fn an_export_names_what_its_exporter_writes_out() {
    let f = Fixture::new(|dir| vec![spec(dir, &tracklists(), &[])], Options::default());
    let e = |exporter: &str, source: ExportSource| f.export(exporter, source).err().unwrap().to_string();
    let set = || Some("Friday".to_string());
    assert_eq!(e("sheet", ExportSource::default()), "Set sheet writes out a set: say which");
    assert_eq!(e("mixsheet", ExportSource::default()), "Mix sheet writes out a mix: say which");
    assert_eq!(e("m3u", ExportSource::default()), "M3U playlist writes out a drive's library: say which");
    let drive = |more: ExportSource| ExportSource { drive: "USB".into(), ..more };
    assert_eq!(
        e("m3u", drive(ExportSource { set: set(), ..ExportSource::default() })),
        "M3U playlist writes out a drive, not a set"
    );
    assert_eq!(
        e("m3u", drive(ExportSource { mix: Some("m".into()), ..ExportSource::default() })),
        "M3U playlist writes out a drive, not a mix"
    );
    let lists = ExportSource { set: set(), playlists: vec!["Sets".into()], ..ExportSource::default() };
    assert_eq!(e("sheet", lists), "Set sheet writes out a set, not a drive's playlists");
    let both = ExportSource { set: set(), mix: Some("m".into()), ..ExportSource::default() };
    assert_eq!(e("sheet", both.clone()), "Set sheet writes out a set, not a mix");
    assert_eq!(e("mixsheet", both.clone()), "Mix sheet writes out a mix, not a set");
    // One writing out any of the three is told which by what the request names, one of them.
    let any = "Any sheet writes out a drive's library, a set or a mix: say which";
    assert_eq!(e("anysheet", ExportSource::default()), any);
    assert_eq!(e("anysheet", both), "name a set or a mix to write out, not both");
    let lists = ExportSource { mix: Some("m".into()), playlists: vec!["Sets".into()], ..ExportSource::default() };
    assert_eq!(e("anysheet", lists), "Any sheet writes out a drive, a set or a mix, not a drive's playlists");
    let none = f.export("sheet", ExportSource { set: set(), ..ExportSource::default() }).err().unwrap();
    assert_eq!(none.code(), "not_found", "{none}");
    let none =
        f.export("mixsheet", ExportSource { mix: Some("nope".into()), ..ExportSource::default() }).err().unwrap();
    assert_eq!(none.code(), "not_found", "{none}");
    let output = f._dir.path().join("out.txt");
    let source = ExportSource { set: set(), ..ExportSource::default() };
    let e = f.plugins.export_plan("echo", "sheet", source, &output, Value::Null, None, &Ctx::silent()).err().unwrap();
    assert_eq!(e.to_string(), "sets and mixes are kept in the store, and there's none");
}

/// For any source an export names (a drive or none, a set by its title, `last` or its id or one
/// that isn't there, a mix kept or not, playlists), with the store or without: an exporter plans
/// only one of what its manifest's `source` lists, named, and found; anything else is
/// refused, saying what's wrong first. A set's and a mix's are written out as the plugin read
/// them.
#[hegel::test(test_cases = 30, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_export_writes_out_only_what_its_exporter_says(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let f = Fixture::new(|dir| vec![spec(dir, &tracklists(), &[])], Options::default());
    let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
    let friday = Set { id: "3m2friday2222".into(), title: Some("Friday".into()), date, ..Set::default() };
    SetBook { sets: vec![friday], ..SetBook::default() }.save(&f.store).unwrap();
    let tracks = common::library(&f.drive).tracks;
    let mix = report::save(&f.store, &crate::sets::report(&[(&tracks[0], 0.0, 0.9)], None)).unwrap();
    let exporter = tc.draw(gs::sampled_from(vec!["m3u", "sheet", "mixsheet", "anysheet"]));
    let source = ExportSource {
        drive: tc.draw(gs::sampled_from(vec!["", "USB"])).into(),
        set: tc.draw(gs::optional(gs::sampled_from(vec!["Friday", "last", "3m2friday2222", "nope"]))).map(String::from),
        mix: tc.draw(gs::optional(gs::sampled_from(vec![mix.clone(), "nope".into()]))),
        playlists: tc.draw(gs::subsequences(vec!["Sets".to_string()])),
        ..ExportSource::default()
    };
    let store = tc.draw(gs::weighted_booleans(0.8)).then_some(&f.store);
    let output = f._dir.path().join("out").join(format!("{exporter}.txt"));
    let planned = f.plugins.export_plan("echo", exporter, source.clone(), &output, Value::Null, store, &Ctx::silent());

    let (title, takes): (&str, &[&str]) = match exporter {
        "m3u" => ("M3U playlist", &["drive"]),
        "sheet" => ("Set sheet", &["set"]),
        "mixsheet" => ("Mix sheet", &["mix"]),
        _ => ("Any sheet", &["drive", "set", "mix"]),
    };
    let (set, mixed) = (source.set.is_some(), source.mix.is_some());
    // What the request names: a set, else a mix, else a drive's library.
    let from = if set {
        "set"
    } else if mixed {
        "mix"
    } else {
        "drive"
    };
    let either = |parts: Vec<String>| match parts.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => unreachable!(),
    };
    let writes = either(takes.iter().map(|t| format!("a {t}")).collect());
    let library = |t: &&str| if *t == "drive" { "a drive's library".to_string() } else { format!("a {t}") };
    let named = Some(format!("{title} writes out {}: say which", either(takes.iter().map(library).collect())));
    let not = |what: &str| Some(format!("{title} writes out {writes}, not {what}"));
    let refused = match () {
        _ if !takes.contains(&from) && from == "drive" => named,
        _ if !takes.contains(&from) => not(&format!("a {from}")),
        _ if from == "drive" && source.drive.is_empty() => named,
        _ if from == "drive" => None,
        _ if !source.playlists.is_empty() => not("a drive's playlists"),
        _ if set && mixed && !takes.contains(&"mix") => not("a mix"),
        _ if set && mixed => Some("name a set or a mix to write out, not both".into()),
        _ if store.is_none() => Some("sets and mixes are kept in the store, and there's none".into()),
        _ => None,
    };
    if let Some(why) = refused {
        assert_eq!(planned.err().unwrap().to_string(), why);
        return;
    }
    if source.set.as_deref() == Some("nope") || source.mix.as_deref() == Some("nope") {
        let e = planned.err().unwrap();
        assert_eq!(e.code(), "not_found", "{e}");
        return;
    }
    let planned = planned.unwrap();
    let id = Device::open(&f.drive).unwrap().id().unwrap().to_string();
    let drive = format!("drive: {}", if source.drive.is_empty() { "" } else { &id });
    match from {
        "set" => assert_eq!(planned.notes, [drive, "set: 3m2friday2222".into()], "by its id"),
        "mix" => assert_eq!(planned.notes, [drive, format!("mix: {mix}")]),
        _ => {}
    }
    planned.apply(store, &Ctx::silent()).unwrap();
    let written = std::fs::read_to_string(&planned.output).unwrap();
    // As the test's reads answer: `sets.get` a set of no tracks, `mix.reports.get` a mix of one.
    match from {
        "set" => assert_eq!(written, ""),
        "mix" => assert_eq!(written, "A - B\n"),
        _ => assert!(written.starts_with("#EXTM3U"), "{written}"),
    }
}

/// For any sizes an exporter says its files are, near `u64::MAX` or not: the plan sums them,
/// and sizes adding up to more than a `u64` holds are the plugin misbehaving, not a panic or a
/// total that wrapped. Regression: they were summed unchecked.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_exports_sizes_that_overflow_are_refused(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let f = Fixture::exporting();
    let size = hegel::one_of!(gs::integers::<u64>().min_value(u64::MAX - 1000), gs::integers::<u64>().max_value(1000));
    let sizes: Vec<u64> = tc.draw(gs::vecs(size).min_size(1).max_size(4));
    let out = f._dir.path().join("out");
    let source = ExportSource { drive: "USB".into(), ..ExportSource::default() };
    let planned = f.plugins.export_plan("echo", "m3u", source, &out, json!({ "sizes": sizes }), None, &Ctx::silent());
    match sizes.iter().try_fold(0u64, |sum, &n| sum.checked_add(n)) {
        Some(total) => {
            let planned = planned.unwrap();
            let size = if total > 0 { format!(" ({})", locallibrary::op::bytes(total)) } else { String::new() };
            let said = format!("write {} file(s){size} to {}", sizes.len(), out.display());
            assert_eq!(planned.summary[0], said);
        }
        None => {
            let said = format!("the Echo plugin failed: its files' sizes add up to more than {} bytes", u64::MAX);
            assert_eq!(failure(planned.err().unwrap()), said);
        }
    }
}

/// What an export meets, as [`prop_an_export_that_cant_be_planned_or_written_fails_saying_why`]
/// draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Exporting {
    /// An exporter the plugin doesn't have.
    Unknown,
    /// An option it doesn't take.
    Option,
    /// A set, without a store.
    SetWithoutStore,
    /// A set, where the store's sets can't be read.
    SetsUnreadable,
    /// A mix, without a store.
    MixWithoutStore,
    /// Applied in a store the output is in.
    InTheStore,
    /// Applied where its folder can't be made.
    NoFolder,
    /// The plugin fails as it writes.
    WriteFails,
    /// The plugin doesn't write a file it said it would.
    Unwritten,
    /// The plugin is gone by the time it's applied.
    Gone,
    /// A drive that isn't known here.
    Elsewhere,
    /// The plugin fails its dry run.
    DryRunFails,
    /// A set and a mix both.
    Both,
    /// A set the store doesn't have.
    NoSet,
    /// A mix the store doesn't have.
    NoMix,
    /// A drive's library, to an exporter of sets.
    DriveToSheet,
    /// A set and a mix, to an exporter of sets.
    BothToSheet,
    /// A set the store has.
    SetFound,
    /// Nothing, to an exporter of a drive's library, a set or a mix.
    Nothing,
    /// A file it writes there already.
    Overwrites,
    /// It writes a file it didn't say it would.
    Unannounced,
}

/// For any export that can't be planned (an exporter or option the plugin doesn't have, a set or
/// a mix without a store, or with sets that can't be read) or written as planned (into a store,
/// where its folder can't be made, the plugin failing, gone or not writing what it said it
/// would), it fails, saying why.
#[hegel::test(test_cases = 1)]
fn prop_an_export_that_cant_be_planned_or_written_fails_saying_why(tc: hegel::TestCase) {
    for exporting in [
        Exporting::Unknown,
        Exporting::Option,
        Exporting::SetWithoutStore,
        Exporting::SetsUnreadable,
        Exporting::MixWithoutStore,
        Exporting::InTheStore,
        Exporting::NoFolder,
        Exporting::WriteFails,
        Exporting::Unwritten,
        Exporting::Gone,
        Exporting::Elsewhere,
        Exporting::DryRunFails,
        Exporting::Both,
        Exporting::NoSet,
        Exporting::NoMix,
        Exporting::DriveToSheet,
        Exporting::BothToSheet,
        Exporting::SetFound,
        Exporting::Nothing,
        Exporting::Overwrites,
        Exporting::Unannounced,
    ] {
        prop_an_export_that_cant_be_planned_or_written_fails_saying_why_case(&tc, exporting);
    }
}

/// [`prop_an_export_that_cant_be_planned_or_written_fails_saying_why`] for one `exporting`.
#[hegel::test_helper]
fn prop_an_export_that_cant_be_planned_or_written_fails_saying_why_case(tc: &hegel::TestCase, exporting: Exporting) {
    use hegel::generators as gs;
    let options = "options = { type = \"object\", properties = { extra = { type = \"string\" }, only_dry = { type = \"string\" } } }";
    let manifest = tracklists().replace("extension = \"m3u\"\n", &format!("extension = \"m3u\"\n{options}\n"));
    let f = Fixture::new(|dir| vec![spec(dir, &manifest, &[])], Options::default());
    let dir = f._dir.path();
    let name = format!("{}.m3u", tc.draw(gs::from_regex("[a-z]{1,6}").fullmatch(true)));
    let (mut exporter, mut output, mut options) = ("m3u", dir.join("out").join(&name), Value::Null);
    let (mut source, mut store) = (ExportSource { drive: "USB".into(), ..ExportSource::default() }, Some(&f.store));
    let other = Store::open_or_init(dir.join("other")).unwrap();
    match exporting {
        Exporting::Unknown => exporter = "nope",
        Exporting::Option => (exporter, options) = ("anysheet", json!({ "other": 1 })),
        Exporting::SetWithoutStore => {
            (exporter, store) = ("sheet", None);
            source = ExportSource { set: Some("Friday".into()), ..ExportSource::default() };
        }
        Exporting::SetsUnreadable => {
            exporter = "sheet";
            source = ExportSource { set: Some("Friday".into()), ..ExportSource::default() };
            std::fs::write(f.store.root().join(locallibrary::sets::SETS_FILE), "damaged").unwrap();
        }
        Exporting::MixWithoutStore => {
            (exporter, store) = ("mixsheet", None);
            source = ExportSource { mix: Some("m".into()), ..ExportSource::default() };
        }
        Exporting::InTheStore => output = other.root().join(&name),
        Exporting::NoFolder => {
            std::fs::write(dir.join("file"), "").unwrap();
            output = dir.join("file").join("out").join(&name);
        }
        Exporting::WriteFails => options = json!({ "extra": dir.join("none").join("extra.txt") }),
        Exporting::Unwritten => options = json!({ "only_dry": dir.join("out").join(&name).join("more.txt") }),
        Exporting::Gone => {}
        Exporting::Elsewhere => source.drive = "ELSEWHERE".into(),
        Exporting::Both => {
            exporter = "anysheet";
            source = ExportSource { set: Some("Friday".into()), mix: Some("m".into()), ..ExportSource::default() };
        }
        Exporting::NoSet => {
            exporter = "sheet";
            source = ExportSource { set: Some("Friday".into()), ..ExportSource::default() };
        }
        Exporting::DriveToSheet => exporter = "sheet",
        Exporting::Nothing => (exporter, source) = ("anysheet", ExportSource::default()),
        Exporting::Unannounced => options = json!({ "extra": dir.join("extra.txt") }),
        Exporting::Overwrites => {
            std::fs::create_dir_all(output.parent().unwrap()).unwrap();
            std::fs::write(&output, "there already").unwrap();
        }
        Exporting::BothToSheet => {
            exporter = "sheet";
            source = ExportSource { set: Some("Friday".into()), mix: Some("m".into()), ..ExportSource::default() };
        }
        Exporting::SetFound => {
            exporter = "sheet";
            let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
            let set = Set { id: "3m2friday2222".into(), title: Some("Friday".into()), date, ..Set::default() };
            SetBook { sets: vec![set], ..SetBook::default() }.save(&f.store).unwrap();
            source = ExportSource { set: Some("Friday".into()), ..ExportSource::default() };
        }
        Exporting::NoMix => {
            exporter = "mixsheet";
            source = ExportSource { mix: Some("m".into()), ..ExportSource::default() };
        }
        Exporting::DryRunFails => std::fs::write(f.drive.join(locallibrary::paths::EXPORT_PDB), "damaged").unwrap(),
    }
    let planned = f.plugins.export_plan("echo", exporter, source, &output, options, store, &Ctx::silent());
    let code = match exporting {
        Exporting::Unknown | Exporting::Elsewhere | Exporting::NoSet | Exporting::NoMix => Some("not_found"),
        Exporting::Both | Exporting::DriveToSheet | Exporting::BothToSheet => Some("invalid"),
        Exporting::Nothing => {
            let e = planned.err().unwrap();
            let said = "Any sheet writes out a drive's library, a set or a mix: say which";
            return assert_eq!((e.code(), e.to_string()), ("invalid", said.to_string()));
        }
        Exporting::DryRunFails => Some("parse"),
        Exporting::Option | Exporting::SetWithoutStore | Exporting::MixWithoutStore => Some("invalid"),
        Exporting::SetsUnreadable => Some("parse"),
        _ => None,
    };
    if let Some(code) = code {
        return assert_eq!(planned.err().unwrap().code(), code, "{exporting:?}");
    }
    let planned = planned.unwrap();
    if exporting == Exporting::Overwrites {
        return assert!(planned.destructive && planned.summary.contains(&"overwrite 1 of them".to_string()));
    }
    if exporting == Exporting::SetFound {
        return assert_eq!(planned.notes, ["drive: ", "set: 3m2friday2222"]);
    }
    if exporting == Exporting::Unannounced {
        let e = failure(planned.apply(None, &Ctx::silent()).err().unwrap());
        let extra = dir.join("extra.txt");
        return assert_eq!(
            e,
            format!("the Echo plugin failed: it wrote what it didn't say it would: {}", extra.display())
        );
    }
    let applied = match exporting {
        Exporting::InTheStore => planned.apply(Some(&other), &Ctx::silent()),
        Exporting::Gone => {
            drop(f.plugins);
            planned.apply(None, &Ctx::silent())
        }
        _ => planned.apply(None, &Ctx::silent()),
    };
    let e = applied.err().unwrap();
    match exporting {
        Exporting::InTheStore => {
            assert!(e.to_string().starts_with("a plugin's export isn't written to the store at "), "{e}")
        }
        Exporting::Gone => assert_eq!(e.code(), "stale_plan", "{e}"),
        Exporting::Unwritten => {
            let said = format!(
                "the Echo plugin failed: it didn't write what it said it would: {}",
                output.join("more.txt").display()
            );
            assert_eq!(failure(e), said);
        }
        _ => assert!(matches!(e.code(), "io" | "external"), "{exporting:?}: {e}"),
    }
}
