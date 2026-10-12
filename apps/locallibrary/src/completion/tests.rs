use super::*;

#[test]
fn arguments_complete_to_what_they_take() {
    use clap::CommandFactory;
    let cmd = crate::Cli::command();
    let find = |path: &[&str], id: &str| {
        let mut c = &cmd;
        for p in path {
            c = c.find_subcommand(p).unwrap_or_else(|| panic!("no {p}"));
        }
        let a = c.get_arguments().find(|a| a.get_id() == id).unwrap_or_else(|| panic!("no {id} in {path:?}"));
        takes(a)
    };
    assert_eq!(find(&["sync"], "drives"), Some(Takes::Drive));
    assert_eq!(find(&["sync"], "from"), Some(Takes::Drive));
    assert_eq!(find(&["playlist", "add"], "target"), Some(Takes::Playlist));
    assert_eq!(find(&["playlist", "add"], "drive"), Some(Takes::Drive));
    assert_eq!(find(&["pack"], "target"), Some(Takes::Drive));
    assert_eq!(find(&["pack"], "playlists"), Some(Takes::Playlist));
    assert_eq!(find(&["tags", "add"], "tag"), Some(Takes::Tag));
    assert_eq!(find(&["plays", "export"], "list"), Some(Takes::HistoryList));
    assert_eq!(find(&["merge"], "history"), None, "a flag takes nothing");
    assert_eq!(find(&["refresh"], "drive"), Some(Takes::Drive));
    assert_eq!(find(&["refresh"], "skip"), Some(Takes::Step));
    assert_eq!(find(&["prep"], "drives"), Some(Takes::Drive));
    assert_eq!(find(&["prep"], "library"), Some(Takes::Library));
    assert_eq!(find(&["prep"], "players"), Some(Takes::Model));
    assert_eq!(find(&["sets", "edit"], "set"), Some(Takes::Set));
    assert_eq!(find(&["sets", "new"], "sessions"), Some(Takes::Session));
    assert_eq!(find(&["sets", "playlist"], "drive"), Some(Takes::Drive));
    assert_eq!(find(&["setup"], "primary"), Some(Takes::Drive));
    assert_eq!(find(&["pull"], "library"), Some(Takes::Library));
    assert_eq!(find(&["players", "set"], "models"), Some(Takes::Model));
    assert_eq!(find(&["drive", "format"], "erase"), None);
    // About one drive: a library's name isn't taken.
    assert_eq!(find(&["drive", "rename"], "drive"), Some(Takes::OneDrive));
    assert_eq!(find(&["drive", "format"], "drive"), Some(Takes::OneDrive));
    assert_eq!(find(&["drive", "verify"], "against"), Some(Takes::OneDrive));
    assert_eq!(find(&["library", "push"], "drive"), Some(Takes::OneDrive));
    assert_eq!(find(&["settings", "set"], "drive"), Some(Takes::OneDrive));
    assert_eq!(find(&["provision"], "target"), Some(Takes::OneDrive));
    assert_eq!(find(&["provision"], "like"), Some(Takes::Drive));
}
