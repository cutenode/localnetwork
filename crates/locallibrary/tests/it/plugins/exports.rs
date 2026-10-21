//! Exporters of sets and mixes: the request names what the exporter's `source` says, found by
//! the host, and the plugin reads it.

use locallibrary::Device;
use locallibrary::mix::report;
use locallibrary::sets::{Set, SetBook, SetEntry};

use super::*;

/// The echo manifest granting `sets:read`, with a set exporter (`sheet`) and a mix exporter
/// (`mixsheet`), each writing a tracklist.
fn tracklists() -> String {
    granting(&["sets:read"]).replace(
        "[[contributes.providers]]",
        "[[contributes.exporters]]\nformat = \"sheet\"\ntitle = \"Set sheet\"\nextension = \"txt\"\nsource = \"set\"\n\n\
         [[contributes.exporters]]\nformat = \"mixsheet\"\ntitle = \"Mix sheet\"\nextension = \"txt\"\n\
         source = \"mix\"\n\n[[contributes.providers]]",
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
    assert_eq!(e("mixsheet", both), "Mix sheet writes out a mix, not a set");
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
/// only what its manifest's `source` says it writes out, named, and found; anything else is
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
    let exporter = tc.draw(gs::sampled_from(vec!["m3u", "sheet", "mixsheet"]));
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

    let (title, writes) = match exporter {
        "m3u" => ("M3U playlist", "drive"),
        "sheet" => ("Set sheet", "set"),
        _ => ("Mix sheet", "mix"),
    };
    let (set, mixed) = (source.set.is_some(), source.mix.is_some());
    let not = |what: &str| Some(format!("{title} writes out a {writes}, not {what}"));
    let refused = match exporter {
        "m3u" if source.drive.is_empty() => Some(format!("{title} writes out a drive's library: say which")),
        "m3u" if set => not("a set"),
        "m3u" if mixed => not("a mix"),
        "m3u" => None,
        _ if !source.playlists.is_empty() => not("a drive's playlists"),
        "sheet" if mixed => not("a mix"),
        "mixsheet" if set => not("a set"),
        _ if !set && !mixed => Some(format!("{title} writes out a {writes}: say which")),
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
    match exporter {
        "sheet" => assert_eq!(planned.notes, [drive, "set: 3m2friday2222".into()], "by its id"),
        "mixsheet" => assert_eq!(planned.notes, [drive, format!("mix: {mix}")]),
        _ => {}
    }
    planned.apply(store, &Ctx::silent()).unwrap();
    let written = std::fs::read_to_string(&planned.output).unwrap();
    // As the test's reads answer: `sets.get` a set of no tracks, `mix.reports.get` a mix of one.
    match exporter {
        "sheet" => assert_eq!(written, ""),
        "mixsheet" => assert_eq!(written, "A - B\n"),
        _ => assert!(written.starts_with("#EXTM3U"), "{written}"),
    }
}
