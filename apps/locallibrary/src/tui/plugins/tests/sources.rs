//! Library sources in the player: "build from …" asks for the folders the plugin may read and the
//! source's options, then the plan is confirmed and written as any change is.

use std::path::{Path, PathBuf};

use hegel::TestCase;
use hegel::generators as gs;
use ratatui::crossterm::event::KeyCode;

use super::super::super::app::App;
use super::super::super::app::testing::{
    Plugged, app_on, confirming, demo_drive, demo_drive_in, echo_plugin, key, last_message, mounted, press, say_yes,
    settle, wait,
};
use super::super::super::edits::Modal;
use super::super::super::ui::screen;
use super::{Screen, entry, move_to, picker};

/// The echo plugin (in `dir`) granted `fs:read`, with the library source `folder`; and a folder
/// of music beside the drives, with one WAV file.
fn sourcing(dir: &Path) -> PathBuf {
    echo_plugin(dir);
    let file = dir.join("echo.toml");
    let manifest = std::fs::read_to_string(&file).unwrap();
    let granted = manifest.replace(r#""library:plan"]"#, r#""library:plan", "fs:read"]"#);
    let source = "\n[[contributes.sources]]\nname = \"folder\"\ntitle = \"Audio files\"\n\
                  options = { type = \"object\", properties = { per_page = { type = \"integer\" } } }\n";
    std::fs::write(&file, granted + source).unwrap();
    let music = dir.join("music");
    std::fs::create_dir_all(&music).unwrap();
    let pcm = loclib_mixid::test_util::synth(1.0, 1).into_iter().map(|s| (s * 12_000.0) as i32);
    std::fs::write(music.join("a.wav"), loclib_mixid::test_util::wav_of(44_100, 16, pcm)).unwrap();
    music
}

/// Fills the form open with `music` as the folder granted and `replace`, and submits it.
fn build(app: &mut App, music: &Path, replace: bool) {
    press(app, ":build from audio files\n");
    let Some(Modal::Form(f)) = &mut app.modal else { panic!("no form: {}", last_message(app)) };
    assert_eq!(f.title, "Audio files");
    let names: Vec<&str> = f.fields.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["read_from", "replace", "per_page"]);
    assert_eq!(f.fields[0].text, "~/Music", "it starts at the music folder");
    f.fields[0].text = music.display().to_string();
    f.fields[1].text = replace.to_string();
    press(app, "\n");
}

/// On a drive plugged in, a library source builds it: a library already there is replaced only
/// when the form says so, and the plan says which folders the plugin read before it's written.
#[test]
fn a_drive_is_built_from_a_library_source() {
    let (dir, root) = demo_drive();
    let music = sourcing(dir.path());
    let (mut blank, _store) = super::super::super::app::testing::blank(super::Screen::Library);
    press(&mut blank, ":");
    assert_eq!(entry(&blank, "build from audio files…").1, Some("open a drive first"));

    mounted(dir.path());
    let mut app = app_on(&root);
    build(&mut app, &music, false);
    wait(&mut app, |a| a.job.is_none());
    let said = last_message(&app).to_string();
    assert!(said.contains("wasn't built from echo:folder; replace it"), "{said}");

    build(&mut app, &music, true);
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Confirm(c)) = &app.modal else { panic!("no confirmation: {}", last_message(&app)) };
    assert_eq!(c.plan.operation(), "echo.source.folder");
    let s = screen(&mut app, 120, 40);
    for needle in
        ["Replace the library on", "with the library source echo:folder?", "read the files Audio files", "y build"]
    {
        assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
    }
    // Not echoed on failure: CodeQL takes a toast written after a plan's apply for something
    // secret.
    say_yes(&mut app);
    let library = locallibrary::Device::open(&root).unwrap().library().unwrap();
    let titles: Vec<&str> = library.tracks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["a"], "built from the folder");
}

/// The folder a property test's drives are mounted in, with [`sourcing`]'s plugin, and its
/// music folder: set up as the first case starts, for every case after it (see `plugins_once`).
fn sourcing_once() -> &'static (tempfile::TempDir, PathBuf) {
    static DIR: std::sync::OnceLock<(tempfile::TempDir, PathBuf)> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let music = sourcing(dir.path());
        mounted(dir.path());
        (dir, music)
    })
}

/// The folders a drawn build grants, as the form takes them.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Granted {
    Music,
    /// The music folder, and again spelled another way.
    MusicTwice,
    /// A relative path, which the form takes from where the player runs: no folder there.
    Relative,
    Nowhere,
    Nothing,
}

/// From the palette or the drive's menu, with drawn folders, replacing or not and any page size:
/// with no folder the form stays open; a folder that isn't there, or a library on the drive not
/// replaced, is refused, saying so; esc while it's planned stops it. Otherwise the
/// confirmation names the source and the folders read, a no writes nothing, and a yes builds the
/// drive from the music folder.
#[hegel::test(test_cases = 12, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_drive_is_built_from_a_source_once_confirmed(tc: TestCase) {
    static CASES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let (dir, music) = sourcing_once();
    let n = CASES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let drive = Plugged(demo_drive_in(dir.path(), &format!("SRC{n}")));
    let titles = || -> Vec<String> {
        let library = locallibrary::Device::open(&drive.0).unwrap().library().unwrap();
        library.tracks.into_iter().map(|t| t.title).collect()
    };
    let before = titles();
    let mut app = app_on(&drive.0);
    if tc.draw(gs::booleans()) {
        press(&mut app, ":build from audio files\n");
    } else {
        let info = locallibrary::api::drive_info(&locallibrary::api::DriveRef { drive: drive.0.display().to_string() });
        app.drives = vec![info.unwrap()];
        app.drive_state.select(Some(0));
        app.view = Screen::Drives;
        press(&mut app, "e");
        let at = picker(&app).items.iter().position(|i| i.label == "build from audio files…").unwrap();
        move_to(&mut app, at);
        key(&mut app, KeyCode::Enter);
    }
    let granted = tc.draw(gs::sampled_from(vec![
        Granted::Music,
        Granted::MusicTwice,
        Granted::Relative,
        Granted::Nowhere,
        Granted::Nothing,
    ]));
    let replace = tc.draw(gs::booleans());
    let per_page = tc.draw(gs::sampled_from(vec!["", "1", "2"]));
    let Some(Modal::Form(f)) = &mut app.modal else { panic!("no form: {}", last_message(&app)) };
    f.fields[0].text = match granted {
        Granted::Music => music.display().to_string(),
        Granted::MusicTwice => format!("{}, {}", music.display(), music.join(".").display()),
        Granted::Relative => "music".into(),
        Granted::Nowhere => dir.path().join("nowhere").display().to_string(),
        Granted::Nothing => String::new(),
    };
    f.fields[1].text = replace.to_string();
    f.fields[2].text = per_page.into();
    press(&mut app, "\n");
    if granted == Granted::Nothing {
        assert!(matches!(app.modal, Some(Modal::Form(_))) && app.job.is_none(), "the form stays open");
        return;
    }
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Planning"));
    if tc.draw(gs::integers::<u8>().max_value(4)) == 0 {
        press(&mut app, "\u{1b}");
        assert_eq!((app.job.is_none(), app.modal.is_none(), last_message(&app)), (true, true, "cancelled"));
        assert_eq!(titles(), before);
        return;
    }
    wait(&mut app, |a| a.job.is_none());
    let refused = match granted {
        Granted::Relative | Granted::Nowhere => Some("not found: the folder"),
        _ if !replace => Some("wasn't built from echo:folder; replace it"),
        _ => None,
    };
    if let Some(said) = refused {
        assert!(app.modal.is_none() && last_message(&app).contains(said), "{said:?}: {}", last_message(&app));
        assert_eq!(titles(), before);
        return;
    }
    let (operation, _) = confirming(&mut app);
    assert_eq!(operation, "echo.source.folder");
    let Some(Modal::Confirm(c)) = &app.modal else { panic!() };
    let real = locallibrary::device::without_verbatim_prefix(music.canonicalize().unwrap());
    let read = format!("read the files Audio files finds in {}", real.display());
    assert!(c.plan.summary().contains(&read), "{:?}", c.plan.summary());
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("with the library source echo:folder?"), "{s}");
    settle(&mut app);
    if tc.draw(gs::booleans()) {
        press(&mut app, tc.draw(gs::sampled_from(vec!["n", "\u{1b}", "q"])));
        assert_eq!((last_message(&app), titles()), ("nothing written", before));
        return;
    }
    // Not echoed on failure, as above.
    say_yes(&mut app);
    assert_eq!(titles(), ["a"], "built from the music folder");
}
