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
