//! Panels (`panel/render`, the `panels` feature): a plugin's view of a track, and the views the
//! `views` feature brings, read as hosts take them: an image's file read and given on as a PNG,
//! the new kinds only from a plugin with the feature, and no question where one can't be.

use base64::prelude::*;
use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::Column;

use super::*;

/// The echo manifest, with a panel (`said`) answering as its `panel` setting says (see the test
/// plugin's `render_panel`).
fn panelled() -> String {
    let more = "\n[[contributes.panels]]\nname = \"said\"\ntitle = \"As it says\"\n\n[[contributes.settings]]\nkey = \"panel\"\ntitle = \"What it shows\"\ntype = \"string\"\n";
    format!("{ECHO}{more}")
}

impl Fixture {
    /// The echo plugin with its `said` panel, run with `args`.
    fn panelled(args: &'static [&'static str]) -> Fixture {
        Fixture::new(|dir| vec![spec(dir, &panelled(), args)], Options::default())
    }

    /// The first track on the drive, by id.
    fn first_track(&self) -> u32 {
        common::library(&self.drive).tracks[0].id
    }

    /// The `said` panel's view of the first track, once its `panel` setting is `answer`.
    fn said(&self, answer: Value) -> Result<View> {
        self.plugins.set_setting("echo", &self.store, "panel", Some(answer.to_string().into())).unwrap();
        self.plugins.render_panel("echo", "said", "USB", self.first_track(), Some(&self.store), &Ctx::silent())
    }
}

#[test]
fn a_panel_shows_the_track_its_given() {
    let f = Fixture::panelled(&[]);
    let names: Vec<(&str, &str)> =
        f.plugins.panels().into_iter().map(|(plugin, p)| (plugin, p.name.as_str())).collect();
    assert_eq!(names, [("echo", "about"), ("echo", "said")]);
    let track = &common::library(&f.drive).tracks[0];
    let View::Kv { pairs, .. } =
        f.plugins.render_panel("echo", "about", "USB", track.id, Some(&f.store), &Ctx::silent()).unwrap()
    else {
        panic!("its id and title")
    };
    let shown: Vec<(&str, &Value)> = pairs.iter().map(|p| (p.label.as_str(), &p.value)).collect();
    assert_eq!(shown, [("id", &json!(track.id)), ("title", &json!(track.title))]);
    let e = f.plugins.render_panel("echo", "nope", "USB", track.id, None, &Ctx::silent()).unwrap_err();
    assert_eq!((e.code(), e.to_string()), ("not_found", "not found: the echo plugin's panel \"nope\"".into()));
    let e = f.said(json!({ "fail": "not_found" })).unwrap_err();
    assert_eq!(e.to_string(), "not found: it failed (the Echo plugin)");
}

/// An image's file is read from the scratch folder and given on as a PNG `data:` URL, whatever
/// the plugin said it was; a file that isn't an image is refused.
#[test]
fn a_panels_image_is_read_from_its_file() {
    let f = Fixture::panelled(&[]);
    let image = json!({ "kind": "image", "blob": "cover.jpg", "alt": "the cover", "data": "data:text/plain,nope" });
    let view = f.said(json!({ "blobs": { "cover.jpg": jpeg(16) }, "view": image })).unwrap();
    let View::Image { alt, data: Some(url), .. } = view else { panic!("an image") };
    let png = BASE64_STANDARD.decode(url.strip_prefix("data:image/png;base64,").unwrap()).unwrap();
    assert_eq!((alt.as_str(), image::guess_format(&png).unwrap()), ("the cover", image::ImageFormat::Png));
    let e = f.said(json!({ "blobs": { "cover.jpg": [1, 2, 3] }, "view": image })).unwrap_err();
    assert!(failure(e).starts_with("the Echo plugin failed: its image cover.jpg: "));
    let e = f.said(json!({ "view": image })).unwrap_err();
    assert!(failure(e).starts_with("the Echo plugin failed: its image: its file \"cover.jpg\" isn't there"));
    // In a section, too.
    let sections = json!({ "kind": "sections", "items": [{ "title": "Cover", "view": image }] });
    let View::Sections { items, .. } = f.said(json!({ "blobs": { "cover.jpg": jpeg(16) }, "view": sections })).unwrap()
    else {
        panic!("sections")
    };
    assert!(matches!(&items[0].view, View::Image { data: Some(url), .. } if url.starts_with("data:image/png")));
}

/// A big picture comes as a preview, 512 pixels on its longest side; one over 8192 pixels on a
/// side is refused unread.
#[test]
fn a_panels_image_is_kept_small() {
    let png = |w: u32, h: u32| {
        let mut out = Vec::new();
        let image = image::RgbImage::from_pixel(w, h, image::Rgb([200, 40, 90]));
        image.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    };
    let f = Fixture::panelled(&[]);
    let image = json!({ "kind": "image", "blob": "a.png", "alt": "a" });
    let View::Image { data: Some(url), .. } =
        f.said(json!({ "blobs": { "a.png": png(2000, 1000) }, "view": image })).unwrap()
    else {
        panic!("an image")
    };
    let preview = BASE64_STANDARD.decode(url.strip_prefix("data:image/png;base64,").unwrap()).unwrap();
    let preview = image::load_from_memory(&preview).unwrap();
    assert_eq!((preview.width(), preview.height()), (512, 256));
    let e = f.said(json!({ "blobs": { "a.png": png(9000, 1) }, "view": image })).unwrap_err();
    assert!(failure(e).starts_with("the Echo plugin failed: its image a.png: "));
}

/// A tree, a chart, an image, sections or a typed column come only from a plugin with the
/// `views` feature; a panel never asks a question, nor does a section of a command's view.
#[test]
fn what_a_view_may_be_is_checked() {
    let tree = json!({ "kind": "tree", "items": [{ "label": "a" }] });
    let typed = json!({ "kind": "table", "columns": [{ "name": "Time", "kind": "duration" }], "rows": [[187]] });
    let question = json!({ "kind": "choose", "title": "Which?", "param": "x", "items": [] });
    let f = Fixture::panelled(&["--without", "views"]);
    for view in [&tree, &typed] {
        let e = f.said(json!({ "view": view })).unwrap_err();
        assert_eq!(failure(e), "the Echo plugin failed: its view needs the views feature, which it doesn't have");
    }
    let plain = json!({ "kind": "table", "columns": ["Title"], "rows": [] });
    assert!(matches!(f.said(json!({ "view": plain })).unwrap(), View::Table { .. }), "plain columns need nothing");
    let f = Fixture::panelled(&[]);
    let View::Table { columns, .. } = f.said(json!({ "view": typed })).unwrap() else { panic!("a table") };
    assert!(matches!(&columns[0], Column::Typed(_)));
    let e = f.said(json!({ "view": question })).unwrap_err();
    assert_eq!(failure(e), "the Echo plugin failed: its panel said asks a question, which a panel can't");
    let in_a_section = json!({ "kind": "sections", "items": [{ "title": "A", "view": question }] });
    let e = f.command("echo", json!({ "view": in_a_section }), &Ctx::silent());
    let CommandOutput::Headless(answered) = e.unwrap() else { panic!("a headless command answers it as it is") };
    assert_eq!(answered, in_a_section);
    let e = f.command("titles", json!({ "view": in_a_section }), &Ctx::silent()).unwrap_err();
    assert_eq!(failure(e), "the Echo plugin failed: a section of its view asks a question");
    // A view command's image is read as a panel's is.
    let image = json!({ "kind": "image", "blob": "a.jpg", "alt": "a" });
    let CommandOutput::View(View::Image { data: Some(_), .. }) =
        f.command("titles", json!({ "view": image, "blobs": { "a.jpg": jpeg(16) } }), &Ctx::silent()).unwrap()
    else {
        panic!("an image")
    };
}

/// A panel stops once it's cancelled (the player asks for another track's).
#[test]
fn a_panel_stops_when_cancelled() {
    let f = Fixture::panelled(&[]);
    let cancel = locallibrary::CancelToken::new();
    let stop = cancel.clone();
    let cancelling = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        stop.cancel();
    });
    f.plugins.set_setting("echo", &f.store, "panel", Some(json!({ "sleep": 10_000 }).to_string().into())).unwrap();
    let ctx = Ctx::new(&locallibrary::progress::NoProgress, cancel);
    let at = Instant::now();
    let e = f.plugins.render_panel("echo", "said", "USB", f.first_track(), Some(&f.store), &ctx).unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert!(at.elapsed() < Duration::from_secs(5));
    cancelling.join().unwrap();
}

/// For any answers a panel gives, from a plugin with the `views` feature or without, the host
/// takes the view answered (an image given on as a PNG), or says how the plugin failed: a kind
/// the feature brings from a plugin without it, a question (or one in a section), an image whose
/// file isn't there or isn't a picture, or the plugin's own failure.
#[hegel::test(test_cases = 12, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_panels_answer_is_its_view_or_why_it_isnt(tc: TestCase) {
    let views = tc.draw(gs::booleans());
    let f = Fixture::panelled(if views { &[] } else { &["--without", "views"] });
    let tree = json!({ "kind": "tree", "items": [{ "label": "a", "children": [{ "label": "b", "detail": "c" }] }] });
    let image = json!({ "kind": "image", "blob": "a.jpg", "alt": "a" });
    let question = json!({ "kind": "choose", "title": "Which?", "param": "x", "items": [] });
    let needs = "the Echo plugin failed: its view needs the views feature, which it doesn't have";
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        // The answer, and how the plugin failed (`None`: it's the view answered).
        let (answer, why) = match tc.draw(gs::integers::<u8>().max_value(8)) {
            0 => (json!({ "view": { "kind": "kv", "pairs": [{ "label": "id", "value": 1 }] } }), None),
            1 => (json!({ "view": tree }), Some(needs).filter(|_| !views)),
            2 => (
                json!({ "view": question }),
                Some("the Echo plugin failed: its panel said asks a question, which a panel can't"),
            ),
            3 => {
                let sections = json!({ "kind": "sections", "items": [{ "title": "A", "view": question }] });
                let why = if views { "the Echo plugin failed: a section of its view asks a question" } else { needs };
                (json!({ "view": sections }), Some(why))
            }
            4 => (json!({ "view": image, "blobs": { "a.jpg": jpeg(16) } }), Some(needs).filter(|_| !views)),
            5 => {
                let why =
                    if views { "the Echo plugin failed: its image: its file \"a.jpg\" isn't there" } else { needs };
                (json!({ "view": image }), Some(why))
            }
            6 => {
                let why = if views { "the Echo plugin failed: its image a.jpg: " } else { needs };
                (json!({ "view": image, "blobs": { "a.jpg": [1, 2, 3] } }), Some(why))
            }
            // The plugin's own failure, or what isn't a view (which the test plugin won't send).
            n => {
                let answer = if n == 7 { json!({ "view": { "kind": "chart" } }) } else { json!({ "fail": "invalid" }) };
                assert_eq!(f.said(answer).unwrap_err().code(), "invalid");
                continue;
            }
        };
        match (f.said(answer.clone()), why) {
            (Ok(view), None) => {
                let mut view = serde_json::to_value(view).unwrap();
                // An image is given on as a PNG.
                if let Some(data) = view.as_object_mut().and_then(|v| v.remove("data")) {
                    assert!(data.as_str().unwrap().starts_with("data:image/png;base64,"), "{data}");
                }
                assert_eq!(view, answer["view"]);
            }
            (Err(e), Some(why)) => {
                let said = failure(e);
                assert!(said.starts_with(why), "{answer}: {said}");
            }
            (got, why) => panic!("{answer}: {got:?}, not {why:?}"),
        }
    }
}
