mod api;

use crate::common;

use indexmap::IndexMap;
use std::io::Cursor;

use binrw::BinWrite;
use locallibrary::settings::{
    self, DriveSettings, FlatSettings, SettingValue, SettingsBlob, SettingsFile, SettingsProfile,
};
use uuid::Uuid;

/// Fixture directory name -> (setting key, value id) it differs in.
fn expectation(file: SettingsFile, dir: &str) -> (&'static str, &'static str) {
    use SettingsFile::*;
    match (file, dir) {
        (MySetting, "autocue_off") => ("auto_cue", "off"),
        (MySetting, "autocuelevel_36") => ("auto_cue_level", "-36db"),
        (MySetting, "autocuelevel_42") => ("auto_cue_level", "-42db"),
        (MySetting, "autocuelevel_72") => ("auto_cue_level", "-72db"),
        (MySetting, "autocuelevel_78") => ("auto_cue_level", "-78db"),
        (MySetting, "discslotillumination_dark") => ("disc_slot_illumination", "dark"),
        (MySetting, "discslotillumination_off") => ("disc_slot_illumination", "off"),
        (MySetting, "hotcueautoload_off") => ("hot_cue_auto_load", "off"),
        (MySetting, "hotcueautoload_rekordboxsetting") => ("hot_cue_auto_load", "rekordbox"),
        (MySetting, "hotcuecolor_on") => ("hot_cue_color", "on"),
        (MySetting, "jogmode_cdj") => ("jog_mode", "cdj"),
        (MySetting, "jogringbrightness_dark") => ("jog_ring_brightness", "dark"),
        (MySetting, "jogringbrightness_off") => ("jog_ring_brightness", "off"),
        (MySetting, "jogringindicator_off") => ("jog_ring_indicator", "off"),
        (MySetting, "language_french") => ("language", "french"),
        (MySetting, "language_greek") => ("language", "greek"),
        (MySetting, "language_turkish") => ("language", "turkish"),
        (MySetting, "lcdbrightness_1") => ("lcd_brightness", "1"),
        (MySetting, "lcdbrightness_5") => ("lcd_brightness", "5"),
        (MySetting, "mastertempo_on") => ("master_tempo", "on"),
        (MySetting, "onairdisplay_off") => ("on_air_display", "off"),
        (MySetting, "phasemeter_type2") => ("phase_meter", "type2"),
        (MySetting, "quantize_eighth") => ("quantize_beat_value", "1/8"),
        (MySetting, "quantize_half") => ("quantize_beat_value", "1/2"),
        (MySetting, "quantize_quarter") => ("quantize_beat_value", "1/4"),
        (MySetting, "quantize_off") => ("quantize", "off"),
        (MySetting, "slipflashing_off") => ("slip_flashing", "off"),
        (MySetting, "sync_on") => ("sync", "on"),
        (MySetting, "temporange_16") => ("tempo_range", "16"),
        (MySetting, "temporange_6") => ("tempo_range", "6"),
        (MySetting, "temporange_wide") => ("tempo_range", "wide"),
        (MySetting, "timemode_elapsed") => ("time_mode", "elapsed"),
        (MySetting2, "beatjumpbeatvalue_64") => ("beat_jump_beat_value", "64"),
        (MySetting2, "beatjumpbeatvalue_half") => ("beat_jump_beat_value", "1/2"),
        (MySetting2, "beatjumpbeatvalue_one") => ("beat_jump_beat_value", "1"),
        (MySetting2, "jogdisplaymode_artwork") => ("jog_display_mode", "artwork"),
        (MySetting2, "jogdisplaymode_info") => ("jog_display_mode", "info"),
        (MySetting2, "jogdisplaymode_simple") => ("jog_display_mode", "simple"),
        (MySetting2, "joglcdbrightness_1") => ("jog_lcd_brightness", "1"),
        (MySetting2, "joglcdbrightness_5") => ("jog_lcd_brightness", "5"),
        (MySetting2, "padbuttonbrightness_1") => ("pad_button_brightness", "1"),
        (MySetting2, "padbuttonbrightness_4") => ("pad_button_brightness", "4"),
        (MySetting2, "vinylspeedadjust_release") => ("vinyl_speed_adjust", "release"),
        (MySetting2, "vinylspeedadjust_touchrelease") => ("vinyl_speed_adjust", "touch-release"),
        (MySetting2, "waveform_phasemeter") => ("waveform_display", "phase-meter"),
        (MySetting2, "waveformdivisions_timescale") => ("waveform_divisions", "time-scale"),
        (DjmMySetting, "beatfxquantize_off") => ("beat_fx_quantize", "off"),
        (DjmMySetting, "chfadercurve_steepbottom") => ("channel_fader_curve", "steep-bottom"),
        (DjmMySetting, "chfadercurve_steeptop") => ("channel_fader_curve", "steep-top"),
        (DjmMySetting, "chfadercurvelong_exp2") => ("channel_fader_curve_long", "smooth"),
        (DjmMySetting, "chfadercurvelong_linear") => ("channel_fader_curve_long", "linear"),
        (DjmMySetting, "crossfadercurve_constantpower") => ("crossfader_curve", "constant-power"),
        (DjmMySetting, "crossfadercurve_slowcut") => ("crossfader_curve", "slow-cut"),
        (DjmMySetting, "headphones_monosplit_monosplit") => ("headphones_mono_split", "mono-split"),
        (DjmMySetting, "headphones_preeq_preeq") => ("headphones_pre_eq", "pre-eq"),
        (DjmMySetting, "miclowcut_off") => ("mic_low_cut", "off"),
        (DjmMySetting, "midi_buttontype_trigger") => ("midi_button_type", "trigger"),
        (DjmMySetting, "midi_ch_15") => ("midi_channel", "15"),
        (DjmMySetting, "midi_ch_16") => ("midi_channel", "16"),
        (DjmMySetting, "midi_ch_2") => ("midi_channel", "2"),
        (DjmMySetting, "mixer_brightness_display_1") => ("mixer_display_brightness", "1"),
        (DjmMySetting, "mixer_brightness_display_2") => ("mixer_display_brightness", "2"),
        (DjmMySetting, "mixer_brightness_display_white") => ("mixer_display_brightness", "white"),
        (DjmMySetting, "mixer_brightness_indicator_1") => ("mixer_indicator_brightness", "1"),
        (DjmMySetting, "mixer_brightness_indicator_2") => ("mixer_indicator_brightness", "2"),
        (DjmMySetting, "talkover_level_12") => ("talk_over_level", "-12db"),
        (DjmMySetting, "talkover_level_24") => ("talk_over_level", "-24db"),
        (DjmMySetting, "talkover_level_6") => ("talk_over_level", "-6db"),
        (DjmMySetting, "talkover_mode_normal") => ("talk_over_mode", "normal"),
        (DevSetting, "keydisplayformat-alphanumeric") => ("key_display_format", "alphanumeric"),
        (DevSetting, "overviewwaveformtype-full") => ("overview_waveform_type", "full"),
        (DevSetting, "waveformcolor-3band") => ("waveform_color", "3band"),
        (DevSetting, "waveformcolor-rgb") => ("waveform_color", "rgb"),
        (DevSetting, "waveformcurrentposition-left") => ("waveform_current_position", "left"),
        other => panic!("no expectation for fixture {other:?}"),
    }
}

#[test]
fn every_fixture_differs_from_defaults_in_exactly_its_setting() {
    let base = common::fixtures().join("rekordcrate/settings");
    let mut checked = 0;
    for (file, dir) in [
        (SettingsFile::MySetting, "mysetting"),
        (SettingsFile::MySetting2, "mysetting2"),
        (SettingsFile::DjmMySetting, "djmmysetting"),
        (SettingsFile::DevSetting, "devsetting"),
    ] {
        for entry in std::fs::read_dir(base.join(dir)).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let (key, value) = expectation(file, &name);
            let blob = settings::read_blob(file, &entry.path().join(file.file_name())).unwrap();
            assert!(blob.checksum_ok(), "{name}: checksum");
            for def in settings::SCHEMA.iter().filter(|d| d.file == file) {
                let want = if def.key == key { value } else { def.default };
                assert_eq!(blob.get(def), SettingValue::Known(want.into()), "{name}: {}", def.key);
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 74);
}

#[test]
fn defaults_match_rekordcrate_and_real_exports() {
    use rekordcrate::setting::Setting;
    for (file, theirs) in [
        (SettingsFile::MySetting, Setting::default_mysetting()),
        (SettingsFile::MySetting2, Setting::default_mysetting2()),
        (SettingsFile::DjmMySetting, Setting::default_djmmysetting()),
        (SettingsFile::DevSetting, Setting::default_devsetting()),
    ] {
        let mut out = Cursor::new(Vec::new());
        theirs.write_options(&mut out, binrw::Endian::Little, (false,)).unwrap();
        let theirs = out.into_inner();
        let ours = SettingsBlob::default_for(file);
        // Identical data sections. (Checksums can differ: the header's version string differs
        // by design, and DJMMYSETTING's checksum covers the header.)
        let end = theirs.len() - 4;
        assert_eq!(&ours.as_bytes()[100..end], &theirs[100..end], "{file:?}");
        // Same checksum where it covers only the data.
        if file != SettingsFile::DjmMySetting {
            assert_eq!(&ours.as_bytes()[end..], &theirs[end..], "{file:?} checksum");
        }
    }
    // The demo export's files are all defaults, except DEVSETTING (exported with RGB waveforms).
    let demo = common::fixtures().join("rekordcrate/demo_tracks");
    let snap = settings::read_all(&DriveSettings(&demo));
    assert!(snap.problems.is_empty() && snap.missing_files.is_empty());
    for def in settings::SCHEMA {
        assert!(matches!(snap.values[def.key], SettingValue::Known(_)), "{}", def.key);
    }
}

#[test]
fn profile_applies_base_and_per_drive_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let a = common::demo_drive(dir.path(), "A");
    let b = dir.path().join("B");
    std::fs::create_dir(&b).unwrap();

    let a_id = Uuid::new_v4();
    let b_id = Uuid::new_v4();
    let mut profile = SettingsProfile::default();
    profile.set(None, "quantize", "off").unwrap();
    profile.set(None, "jog-mode", "CDJ").unwrap();
    profile.set(None, "crossfader_curve", "constant-power").unwrap();
    profile.set(Some((b_id, "B")), "language", "japanese").unwrap();

    let before = std::fs::read(a.join("PIONEER/MYSETTING.DAT")).unwrap();
    let ra = settings::apply(&DriveSettings(&a), &profile.effective(Some(a_id)), false, false).unwrap();
    assert_eq!(ra.changes.len(), 3);
    let after = std::fs::read(a.join("PIONEER/MYSETTING.DAT")).unwrap();
    // Two setting bytes and the checksum changed; nothing else.
    let changed = before.iter().zip(&after).filter(|(x, y)| x != y).count();
    assert!((2..=4).contains(&changed), "{changed} bytes changed");

    // A blank drive gets complete files.
    settings::apply(&DriveSettings(&b), &profile.effective(Some(b_id)), true, false).unwrap();
    let snap = settings::read_all(&DriveSettings(&b));
    assert!(snap.missing_files.is_empty());
    assert_eq!(snap.values["language"], SettingValue::Known("japanese".into()));
    assert_eq!(snap.values["quantize"], SettingValue::Known("off".into()));
    let snap_a = settings::read_all(&DriveSettings(&a));
    assert_eq!(snap_a.values["language"], SettingValue::Known("english".into()));

    assert!(settings::drift(&profile, Some(a_id), &snap_a).is_empty());
    assert!(settings::drift(&profile, Some(b_id), &snap).is_empty());
    // Drive A doesn't have B's override.
    let d = settings::drift(&profile, Some(b_id), &snap_a);
    assert_eq!(d.len(), 1);
    assert!(d[0].overridden);

    // Dry run writes nothing.
    let mut want = IndexMap::new();
    want.insert("sync".to_string(), "on".to_string());
    let before = std::fs::read(a.join("PIONEER/MYSETTING.DAT")).unwrap();
    let r = settings::apply(&DriveSettings(&a), &want, false, true).unwrap();
    assert_eq!(r.changes.len(), 1);
    assert_eq!(std::fs::read(a.join("PIONEER/MYSETTING.DAT")).unwrap(), before);
}

#[test]
fn import_from_flat_folder() {
    let dir = tempfile::tempdir().unwrap();
    for f in SettingsFile::ALL {
        let src = common::fixtures().join("rekordcrate/settings/mysetting/jogmode_cdj").join(f.file_name());
        let bytes =
            if src.exists() { std::fs::read(src).unwrap() } else { SettingsBlob::default_for(f).as_bytes().to_vec() };
        std::fs::write(dir.path().join(f.file_name()), bytes).unwrap();
    }
    let snap = settings::read_all(&FlatSettings(dir.path()));
    let mut p = SettingsProfile::default();
    assert_eq!(p.import_base(&snap), settings::SCHEMA.len());
    assert_eq!(p.base["jog_mode"], "cdj");
}

#[test]
fn reports_setting_bytes_from_newer_rekordbox_versions() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    // rekordbox 6 fixtures: everything decoded.
    assert!(settings::read_all(&DriveSettings(&root)).undecoded.is_empty());
    // rekordbox 7.2 writes mixer settings past the ones known (DJMMYSETTING.DAT data bytes 25..).
    let path = root.join("PIONEER/DJMMYSETTING.DAT");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[104 + 25] = 0x82;
    std::fs::write(&path, bytes).unwrap();
    let snap = settings::read_all(&DriveSettings(&root));
    assert_eq!(snap.undecoded.len(), 1);
    assert_eq!((snap.undecoded[0].offset, snap.undecoded[0].value, snap.undecoded[0].expected), (25, 0x82, 0));
}
