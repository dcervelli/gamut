use super::*;

fn list(count: usize) -> Files {
    let paths = (0..count)
        .map(|i| PathBuf::from(format!("{i}.png")))
        .collect();
    Files::new(paths, 0, decode::Overrides::default())
}

/// Holding `]` through a directory asks for each file in turn without
/// waiting for the last, and only the file the user stopped on is shown.
#[test]
fn a_reply_the_user_has_stepped_past_is_not_accepted() {
    let mut files = list(3);
    let first = files.step(true).expect("three files to step through");
    let second = files.step(true).expect("three files to step through");
    assert_eq!((first.index, second.index), (1, 2));

    assert!(files.accept(first.generation).is_none());
    assert!(!files.is_idle(), "the newer request is still owed a reply");
    assert_eq!(files.accept(second.generation).map(|p| p.index), Some(2));
    assert!(files.is_idle());
}

/// The target is the file last asked for, and the file on screen again
/// once the read is answered or given up; a read said to be slow from
/// the start is said at once and only once.
#[test]
fn the_target_is_the_file_last_asked_for() {
    let mut files = list(3);
    files.shown(0);
    assert_eq!(files.target(), 0);
    let request = files.step(true).expect("three files to step through");
    assert_eq!(files.target(), 1);
    assert_eq!(files.target_path(), Some(Path::new("1.png")));
    let now = Instant::now();
    files.announce_now(now);
    files.announce_now(now + SLOW_READ);
    assert_eq!(
        files.pending().and_then(|pending| pending.announced),
        Some(now)
    );
    assert_eq!(files.announce_slow_read(now + SLOW_READ), Announce::Nothing);
    let answered = files
        .accept(request.generation)
        .expect("the read in flight");
    assert!(
        files.failed(answered.index, None).is_none(),
        "no walk to go on with"
    );
    assert_eq!(files.target(), 0);
}

/// The files a step goes to from the one on screen, the next first.
#[test]
fn the_neighbors_are_where_a_step_goes() {
    let names = |files: &Files| -> Vec<String> {
        files
            .neighbors()
            .iter()
            .map(|path| path.display().to_string())
            .collect()
    };
    assert!(names(&list(1)).is_empty());
    assert_eq!(names(&list(2)), ["1.png"]);
    let mut files = list(3);
    assert_eq!(names(&files), ["1.png", "2.png"]);
    files.shown(2);
    assert_eq!(names(&files), ["0.png", "1.png"]);
}

#[test]
fn stepping_wraps_and_is_not_offered_with_one_file() {
    let mut files = list(2);
    assert_eq!(files.step(false).map(|r| r.index), Some(1));
    assert_eq!(files.step(false).map(|r| r.index), Some(0));
    assert!(list(1).step(true).is_none());
}

/// A file that will not decode must not trap navigation, and the walk has
/// to stop asking once it has been all the way round.
#[test]
fn a_walk_carries_on_past_a_failure_and_gives_up_after_the_last() {
    let mut files = list(3);
    let request = files.step(true).expect("three files");
    let pending = files
        .accept(request.generation)
        .expect("the reply we waited for");
    assert_eq!(pending.index, 1);

    let again = files.failed(1, pending.step).expect("one more file to try");
    assert_eq!(again.index, 2);
    let pending = files
        .accept(again.generation)
        .expect("the reply we waited for");
    assert!(
        files.failed(2, pending.step).is_none(),
        "every other file has been tried"
    );
    assert_eq!(files.index(), 0, "nothing new ever reached the screen");

    assert!(files.failed(0, None).is_none(), "a reload is not a walk");
}

fn named(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

/// A program opened on nothing has an empty list: nothing to step to,
/// nothing to reload, nothing on screen to name — and the first thing
/// handed to it, whether chosen or pasted, goes at the head.
#[test]
fn an_empty_list_names_nothing_and_takes_the_first_thing_it_is_given() {
    let mut files = list(0);
    assert_eq!(files.len(), 0);
    assert_eq!(files.shown_path(), None);
    assert!(files.step(true).is_none());
    assert!(files.reload().is_none());
    assert!(files.page(1).is_none());
    assert!(!files.relist(Vec::new()));
    assert!(files.is_idle());

    let pasted = files.adopt(PathBuf::from("pasted.png"), Source::Disk);
    assert_eq!(pasted.index, 0);
    assert_eq!(files.shown_path(), Some(Path::new("pasted.png")));
    assert!(files.is_adopted(Path::new("pasted.png")));

    // What the dialog chose joins the end of the list and is asked for
    // as a walk over the newcomers; the reply to the paste, still on
    // its way, is not a reply to it.
    let chosen = files
        .append(named(&["a.png", "b.png"]))
        .expect("something new");
    assert_eq!(chosen.index, 1);
    assert_ne!(chosen.generation, pasted.generation);
    assert!(files.accept(pasted.generation).is_none());
    let pending = files
        .accept(chosen.generation)
        .expect("the walk's own reply");
    assert!(
        pending.step.is_some(),
        "a walk, so a bad file is stepped over"
    );
    assert_eq!(files.len(), 3);
    assert!(files.is_adopted(Path::new("pasted.png")));
    let again = files.failed(1, pending.step).expect("on to b.png");
    assert_eq!(again.index, 2);
    let pending = files.accept(again.generation).expect("its reply");
    assert!(
        files.failed(2, pending.step).is_none(),
        "the walk is over the newcomers alone"
    );
    files.shown(2);

    // A file already on the list is not added again, and one that is
    // the only thing chosen is gone to rather than added.
    let again = files
        .append(named(&["a.png", "c.png"]))
        .expect("c.png is new");
    assert_eq!(again.index, 3);
    assert_eq!(files.len(), 4);
    files.accept(again.generation);
    let back = files.append(named(&["a.png"])).expect("a.png, by name");
    assert_eq!(back.index, 1);
    let pending = files.accept(back.generation).expect("its reply");
    assert!(pending.step.is_none(), "not a walk");
    files.shown(1);
    assert!(
        files.append(named(&["a.png"])).is_none(),
        "the file on screen is nothing to ask for"
    );
    assert_eq!(files.len(), 4);
}

/// A directory read again is a list read again: a file written into it
/// joins the walk, and the one on screen goes on being the one on screen.
#[test]
fn a_file_that_has_appeared_joins_the_list() {
    let mut files = list(2);
    files.shown(1);
    assert!(files.relist(named(&["0.png", "1.png", "2.png"])));
    assert_eq!(files.len(), 3);
    assert_eq!(files.shown_path(), Some(Path::new("1.png")));
    assert_eq!(files.index(), 1);
    assert_eq!(files.step(true).map(|r| r.index), Some(2));

    let mut files = list(2);
    files.shown(1);
    assert!(files.relist(named(&["0.png", "0a.png", "1.png"])));
    assert_eq!(files.index(), 2, "a file arriving before it moves it along");
    assert!(
        !files.relist(named(&["0.png", "0a.png", "1.png"])),
        "a directory that has not changed changes nothing"
    );
}

/// The file on screen is never dropped from the list, whatever has become
/// of it: its pixels are up, and everything the interface says about them
/// is read off the path they came from. It keeps its neighbors, so that
/// `]` goes on to what has taken its place rather than back over a file
/// already seen.
#[test]
fn the_file_on_screen_survives_being_deleted_under_us() {
    let mut files = list(4);
    files.shown(1);
    assert!(files.relist(named(&["0.png", "2.png"])));
    assert_eq!(files.shown_path(), Some(Path::new("1.png")));
    assert_eq!(files.len(), 3, "the file on screen, and what is left");
    assert_eq!(files.step(true).map(|r| r.index), Some(2));
    assert_eq!(files.path(2), Path::new("2.png"));
}

/// The file on screen and the neighbors it would have stepped to, all
/// gone at once. It keeps its place among whatever is left, so that `]`
/// reaches the next survivor and `[` the last one before it — the walk
/// carries on from where the user actually is, not from where the list
/// happens to have closed up.
#[test]
fn a_file_deleted_with_its_neighbors_keeps_its_place_among_the_survivors() {
    let mut files = list(5);
    files.shown(2);
    assert!(files.relist(named(&["0.png", "4.png"])));
    assert_eq!(files.shown_path(), Some(Path::new("2.png")));
    assert_eq!(files.index(), 1);
    assert_eq!(files.path(0), Path::new("0.png"));
    assert_eq!(files.path(2), Path::new("4.png"));
    assert_eq!(files.step(true).map(|r| r.index), Some(2));

    // And with everything before it gone, it leads what is left.
    let mut files = list(3);
    files.shown(1);
    assert!(files.relist(named(&["2.png"])));
    assert_eq!(files.index(), 0);
    assert_eq!(files.path(1), Path::new("2.png"));
}

/// The list is read again every time the directory changes, so a file
/// already deleted is passed over the missing path again and again. It
/// stays where it was put, and the rebuilds around it go on as normal.
#[test]
fn a_file_already_gone_keeps_its_place_through_later_rebuilds() {
    let mut files = list(3);
    files.shown(1);
    assert!(
        !files.relist(named(&["0.png", "2.png"])),
        "the file on screen going back in leaves the list as it was, and \
         nothing in the bar reads any differently for it"
    );

    // A file arrives after it: the one on screen has not moved.
    assert!(files.relist(named(&["0.png", "2.png", "3.png"])));
    assert_eq!(files.index(), 1);
    assert_eq!(files.shown_path(), Some(Path::new("1.png")));
    assert_eq!(files.len(), 4);

    // One arrives before it, and it moves along with the rest.
    assert!(files.relist(named(&["0.png", "0a.png", "2.png", "3.png"])));
    assert_eq!(files.index(), 2);
    assert_eq!(files.shown_path(), Some(Path::new("1.png")));

    // And the file itself comes back: it is an ordinary member again,
    // in the place the directory gives it rather than the place we kept.
    assert!(files.relist(named(&["0.png", "0a.png", "1.png", "2.png"])));
    assert_eq!(files.index(), 2);
    assert_eq!(files.len(), 4, "no phantom left behind beside it");
}

/// A pasted picture goes in beside the file on screen and is asked for
/// straight away, so that `[` goes back to where the user was and `]`
/// carries on where they were going.
#[test]
fn a_pasted_file_joins_the_list_beside_the_one_on_screen() {
    let mut files = list(3);
    files.shown(1);
    let request = files.adopt(
        PathBuf::from("/pictures/pasted.png"),
        Source::Clipboard("image/png".into()),
    );
    assert_eq!(request.index, 2);
    assert!(matches!(request.source, Source::Clipboard(_)));
    assert_eq!(files.len(), 4);
    assert_eq!(files.path(2), Path::new("/pictures/pasted.png"));
    assert_eq!(files.index(), 1, "nothing is on screen until it is read");

    let pending = files
        .accept(request.generation)
        .expect("the reply we waited for");
    assert!(
        files.failed(2, pending.step).is_none(),
        "a paste asks for one picture rather than walking off to another"
    );
}

/// No directory named on the command line lists a pasted file — it was
/// written where pictures are kept — so a rebuild would drop every one of
/// them the moment the user stepped off it.
#[test]
fn a_pasted_file_survives_the_list_being_read_again() {
    let mut files = list(3);
    files.shown(1);
    let request = files.adopt(
        PathBuf::from("pasted.png"),
        Source::Clipboard("image/png".into()),
    );
    files.accept(request.generation);
    files.shown(request.index);
    assert_eq!(files.shown_path(), Some(Path::new("pasted.png")));

    // Stepped off it, and the directory changes underneath.
    let request = files.step(true).expect("somewhere to step");
    files.accept(request.generation);
    files.shown(request.index);
    assert!(files.relist(named(&["0.png", "1.png", "2.png", "3.png"])));
    assert_eq!(files.len(), 5);
    assert_eq!(
        files.path(2),
        Path::new("pasted.png"),
        "still between the file it was pasted beside and the next one"
    );
    assert_eq!(files.shown_path(), Some(Path::new("2.png")));
}

/// `--paste` puts the clipboard's picture at the head of the list, and it
/// is a paste like any other from then on: fetched by the loader, kept
/// through a relist that no named directory would put it back into, and
/// walked past — since the rest of the list was asked for too — if it
/// never arrives.
#[test]
fn a_paste_at_the_head_of_the_list_is_kept_like_any_other() {
    let mut files = Files::new(
        named(&["pasted.png", "0.png", "1.png"]),
        0,
        decode::Overrides::default(),
    );
    let request = files.open_first(Source::Clipboard("image/png".into()));
    assert_eq!(request.index, 0);
    assert!(matches!(request.source, Source::Clipboard(_)));

    let pending = files
        .accept(request.generation)
        .expect("the reply we waited for");
    let next = files
        .failed(0, pending.step)
        .expect("a paste that never arrived is walked past");
    assert_eq!(next.index, 1);
    assert!(matches!(next.source, Source::Disk));
    files.accept(next.generation);
    files.shown(1);

    assert!(files.relist(named(&["0.png", "1.png", "2.png"])));
    assert_eq!(files.path(0), Path::new("pasted.png"), "still at the head");
    assert_eq!(files.shown_path(), Some(Path::new("0.png")));
    assert_eq!(files.len(), 4);
}

/// Emptying the directory altogether leaves the picture that is up, with
/// nowhere to step to.
#[test]
fn an_emptied_directory_leaves_the_one_file_on_screen() {
    let mut files = list(2);
    assert!(files.relist(Vec::new()));
    assert_eq!(files.len(), 1);
    assert_eq!(files.shown_path(), Some(Path::new("0.png")));
    assert!(files.step(true).is_none());
}

#[test]
fn a_reload_waits_for_the_read_already_in_flight() {
    let mut files = list(2);
    assert!(files.reload().is_some());
    assert!(files.reload().is_none());
}

/// A file that opens between two frames must not flicker a word into the
/// interface and out again; one that keeps the user waiting has to say so,
/// and say it once.
#[test]
fn only_a_read_that_keeps_the_user_waiting_is_announced() {
    let mut files = list(2);
    assert_eq!(files.announce_slow_read(Instant::now()), Announce::Nothing);

    let request = files.step(true).expect("two files");
    let since = files.pending().expect("a request is in flight").since;
    assert_eq!(
        files.announce_slow_read(since),
        Announce::Waiting(since + SLOW_READ),
        "a read that has just started is not worth mentioning yet"
    );
    assert_eq!(files.announce_slow_read(since + SLOW_READ), Announce::Now);
    assert_eq!(
        files.announce_slow_read(since + SLOW_READ * 2),
        Announce::Nothing,
        "and having been said once it is not said again"
    );

    files.accept(request.generation);
    assert_eq!(
        files.announce_slow_read(since + SLOW_READ * 2),
        Announce::Nothing
    );
}

/// A step taken while a slow read is being announced keeps the
/// announcement, so that the toast saying so stays up through a walk
/// rather than going and coming back on every file.
#[test]
fn a_step_past_an_announced_read_keeps_it_announced() {
    let mut files = list(3);
    files.step(true).expect("three files");
    let since = files.pending().expect("a request is in flight").since;
    assert_eq!(files.announce_slow_read(since + SLOW_READ), Announce::Now);

    files.step(true).expect("three files");
    let pending = files.pending().expect("the next read is in flight");
    assert_eq!(pending.announced, Some(since + SLOW_READ));
    assert_eq!(
        files.announce_slow_read(pending.since),
        Announce::Nothing,
        "already said, so neither waited for nor said again"
    );
}

/// A file moved to the trash stays on the list, and on screen, until
/// its neighbor has taken the screen from it, and leaves the list then
/// — with the file on screen and a read in flight aimed where they
/// were. From the last of the list the step away is back rather than
/// round to the first: the walk goes on the way it was going.
#[test]
fn a_trashed_file_leaves_the_list_once_another_has_the_screen() {
    let mut files = list(4);
    files.shown(1);
    files.condemn();
    assert!(files.is_condemned(Path::new("1.png")));
    let request = files.step_away().expect("somewhere to go");
    assert_eq!(request.index, 2);
    assert_eq!(files.len(), 4, "still on the list while it is on screen");

    files.accept(request.generation);
    files.shown(2);
    assert_eq!(files.len(), 3);
    assert_eq!(files.shown_path(), Some(Path::new("2.png")));
    assert_eq!(files.index(), 1, "the file on screen moved up with it");
    assert!(!files.is_condemned(Path::new("1.png")));
    assert_eq!(
        files.paths(),
        &[
            PathBuf::from("0.png"),
            PathBuf::from("2.png"),
            PathBuf::from("3.png")
        ]
    );

    // From the end of the list, back.
    files.shown(2);
    files.condemn();
    let request = files.step_away().expect("somewhere to go");
    assert_eq!(request.index, 1);
    files.accept(request.generation);
    files.shown(1);
    assert_eq!(
        files.paths(),
        &[PathBuf::from("0.png"), PathBuf::from("2.png")]
    );
    assert_eq!(files.index(), 1);

    // The only file has nowhere to go: it leaves the list, which is
    // then empty, and comes back by being put back at the head.
    let mut alone = list(1);
    assert!(alone.step_away().is_none());
    alone.remove_shown();
    assert_eq!(alone.len(), 0);
    assert_eq!(alone.shown_path(), None);
    assert!(alone.is_idle());
    let back = alone.reinstate(PathBuf::from("0.png"), 0, false);
    assert_eq!(back.index, 0);
    assert_eq!(alone.len(), 1);
    alone.accept(back.generation);
    alone.shown(0);
    assert_eq!(alone.shown_path(), Some(Path::new("0.png")));
}

/// A neighbor that will not decode leaves the trashed file on screen,
/// and on the list: what is on screen is still what the list has to
/// name.
#[test]
fn a_trashed_file_stays_while_nothing_takes_the_screen() {
    let mut files = list(2);
    files.condemn();
    let request = files.step_away().expect("a neighbor");
    let pending = files.accept(request.generation).expect("ours");
    assert!(files.failed(pending.index, pending.step).is_none());
    files.shown(0);
    assert_eq!(files.len(), 2);
    assert!(files.is_condemned(Path::new("0.png")));
}

/// A file put back from the trash goes back where it stood — or at the
/// end of a list that has grown shorter — and is asked for; the file on
/// screen and a read in flight keep their places.
#[test]
fn a_reinstated_file_goes_back_where_it_was() {
    let mut files = list(3);
    files.shown(2);
    let request = files.reinstate(PathBuf::from("1b.png"), 1, false);
    assert_eq!(request.index, 1);
    assert_eq!(files.path(1), Path::new("1b.png"));
    assert_eq!(files.index(), 3, "the file on screen moved along");
    assert_eq!(files.pending().map(|pending| pending.index), Some(1));

    let request = files.reinstate(PathBuf::from("9.png"), 10, true);
    assert_eq!(request.index, 4, "past the end goes at the end");
    assert!(files.is_adopted(Path::new("9.png")));
    assert!(!files.is_adopted(Path::new("1b.png")));
    // A rebuild that does not list it keeps it, as it keeps a paste.
    files.accept(request.generation);
    files.shown(4);
    assert!(files.relist(named(&["0.png", "1.png", "2.png"])));
    assert_eq!(files.len(), 4);
    assert_eq!(files.shown_path(), Some(Path::new("9.png")));
}

/// A renamed file keeps its place on the list under its new name, and
/// stays kept through a rebuild if it was a paste.
#[test]
fn a_renamed_file_keeps_its_place() {
    let mut files = list(3);
    let request = files.adopt(PathBuf::from("pasted.png"), Source::Disk);
    files.accept(request.generation);
    files.shown(1);
    files.rename(1, PathBuf::from("kept.png"));
    assert_eq!(files.shown_path(), Some(Path::new("kept.png")));
    assert_eq!(files.position(Path::new("pasted.png")), None);
    assert!(files.is_adopted(Path::new("kept.png")));
    files.shown(0);
    assert!(
        !files.relist(named(&["0.png", "1.png", "2.png"])),
        "put back where it was, the list is as it was"
    );
    assert_eq!(files.position(Path::new("kept.png")), Some(1));
}

/// A read in flight of the very file that has left the list is a read
/// of nothing: its reply is dropped rather than shown under another
/// file's index.
#[test]
fn a_read_of_a_file_that_left_the_list_is_dropped() {
    let mut files = list(3);
    files.shown(1);
    files.condemn();
    let reload = files.reload().expect("idle");
    // Another file arrives first — a pick from the chooser, say.
    let pick = files.go_to(2);
    files.accept(pick.generation);
    files.shown(2);
    assert_eq!(files.len(), 2);
    assert!(files.accept(reload.generation).is_none());
}

/// A file taken off the list goes out the way a trashed one does —
/// once its neighbor has the screen — and stays out: the directory
/// listing it again does not bring it back, since the user just said
/// they did not want it there. Opening it by name again is what does.
#[test]
fn a_hidden_file_leaves_the_list_and_stays_out_of_a_rebuild() {
    let mut files = list(3);
    files.shown(1);
    files.hide();
    assert!(files.is_hidden(Path::new("1.png")));
    assert!(
        files.is_condemned(Path::new("1.png")),
        "on its way out like a trashed file"
    );
    let request = files.step_away().expect("somewhere to go");
    files.accept(request.generation);
    files.shown(2);
    assert_eq!(files.paths(), &named(&["0.png", "2.png"]));
    assert!(
        files.is_hidden(Path::new("1.png")),
        "remembered after it has left"
    );

    assert!(
        !files.relist(named(&["0.png", "1.png", "2.png"])),
        "a rebuild that lists the hidden file changes nothing"
    );
    assert_eq!(files.paths(), &named(&["0.png", "2.png"]));

    let request = files
        .append(named(&["1.png"]))
        .expect("something to ask for");
    assert_eq!(request.index, 2, "opened by name, it is back, at the end");
    assert!(!files.is_hidden(Path::new("1.png")));
    files.accept(request.generation);
    files.shown(2);
    assert!(
        !files.relist(named(&["0.png", "1.png", "2.png"])),
        "listed again, it is kept where it went in"
    );
    assert_eq!(files.paths(), &named(&["0.png", "2.png", "1.png"]));
}

/// Undo before the neighbor arrives is a reprieve, and undo after is a
/// reinstatement: either way the file is no longer hidden. And a file
/// renamed while hidden stays hidden under its new name.
#[test]
fn a_hidden_file_put_back_is_hidden_no_longer() {
    let mut files = list(3);
    files.shown(1);
    files.hide();
    let request = files.step_away().expect("somewhere to go");
    files.reprieve();
    assert!(!files.is_hidden(Path::new("1.png")));
    assert!(!files.is_condemned(Path::new("1.png")));
    assert!(files.accept(request.generation).is_some());
    files.shown(2);
    assert_eq!(files.len(), 3, "reprieved, it stays");

    files.hide();
    let request = files.step_away().expect("somewhere to go");
    files.accept(request.generation);
    files.shown(1);
    assert_eq!(files.paths(), &named(&["0.png", "1.png"]));
    let request = files.reinstate(PathBuf::from("2.png"), 2, false);
    assert_eq!(request.index, 2);
    assert!(!files.is_hidden(Path::new("2.png")));

    let mut files = list(2);
    files.shown(0);
    files.hide();
    files.rename(0, PathBuf::from("renamed.png"));
    assert!(files.is_hidden(Path::new("renamed.png")));
    assert!(!files.is_hidden(Path::new("0.png")));
}

/// Put in another order, the list keeps the file on screen the file on
/// screen, wherever it has gone; and a rebuild keeps that order among
/// the files it still lists, putting a newcomer in beside the file the
/// directory has before it rather than starting over from name order.
#[test]
fn a_reordered_list_keeps_its_order_through_a_rebuild() {
    let mut files = list(4);
    files.shown(1);
    assert!(!files.reorder(&[0, 1, 2, 3]), "nothing moved");
    assert!(files.reorder(&[3, 1, 0, 2]));
    assert_eq!(files.paths(), &named(&["3.png", "1.png", "0.png", "2.png"]));
    assert_eq!(files.shown_path(), Some(Path::new("1.png")));
    assert_eq!(files.index(), 1);
    assert!(files.reorder(&[2, 0, 1, 3]));
    assert_eq!(files.index(), 2, "moved along with its file");

    // A rebuild in name order that lists a newcomer.
    assert!(files.relist(named(&["0.png", "1.png", "1a.png", "2.png", "3.png"])));
    assert_eq!(
        files.paths(),
        &named(&["0.png", "3.png", "1.png", "1a.png", "2.png"]),
        "the order stands, and the newcomer follows the file listed before it"
    );
    assert_eq!(files.shown_path(), Some(Path::new("1.png")));
    assert!(!files.relist(named(&["0.png", "1.png", "1a.png", "2.png", "3.png"])));

    // One that lists only newcomers goes in ahead of everything, in
    // its own order, as a name-ordered list would have them.
    let mut files = list(1);
    files.shown(0);
    assert!(files.relist(named(&["a.png", "b.png"])));
    assert_eq!(files.paths(), &named(&["0.png", "a.png", "b.png"]));
}
