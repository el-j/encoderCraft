//! Software updates: the check at start, Help ▸ Check for Updates and the update prompt, shown as craft-update's own
//! dialogs on the app's craft-ui [`DialogHost`](craft_ui::dialog::DialogHost), in the interface language, as the
//! other Craft apps show them. An update installs, and closes the app to finish, only while no job is encoding, so it
//! never cuts a batch short.

use craft_ui::dialog::DialogResult;
use craft_update::stack_dialogs::{CheckChoice, UpdateCheckDialog};
use craft_update::updater::{Action, Status};
use encodercraft_core::JobStatus;

use crate::EncoderCraftApp;

/// Whether an update may install and restart the app now: the queue is not working through its jobs and no job is
/// encoding or paused part-way.
pub(crate) fn can_restart<'a>(worker_running: bool, mut jobs: impl Iterator<Item = &'a JobStatus>) -> bool {
    !worker_running && !jobs.any(|status| matches!(status, JobStatus::Processing | JobStatus::Paused))
}

/// Whether an update dialog holds the keyboard: the prompt, and the result window for every status but the notice
/// that an update installed, which leaves the app usable behind it.
pub(crate) fn blocks_input(app: &EncoderCraftApp) -> bool {
    app.updater.prompt_open() || !matches!(app.updater.status(), Status::Idle | Status::Updated(_))
}

/// Whether the result window shows: for every status but idle (a requested check, and the first start after an
/// update, which says that it installed or why it failed), and never over the prompt.
fn result_window_shows(status: &Status, prompt_open: bool) -> bool {
    !prompt_open && !matches!(status, Status::Idle)
}

/// Starts a requested check, else the check at start (once, when the desktop app turned it on), and picks up what
/// finished. Call once per frame, before [`show`].
pub(crate) fn tick(app: &mut EncoderCraftApp, ctx: &egui::Context) {
    if std::mem::take(&mut app.check_updates_requested) {
        app.updater.check_now();
    } else if app.startup_update_check {
        app.updater.check_at_start();
    }
    app.updater.tick(ctx);
}

/// One pass of the update flow, before the dialog host runs: answers the result window closed in the last pass,
/// drives the prompt, and keeps the result window open and current while the updater has something to report.
pub(crate) fn show(app: &mut EncoderCraftApp, ctx: &egui::Context) {
    let jobs = app.queue_mgr.list_jobs();
    let safe = can_restart(app.queue_mgr.is_worker_running(), jobs.iter().map(|job| &job.status));
    // Answered before the window is kept open below, so a dismissed outcome does not open again at once.
    match app.dialogs.take::<UpdateCheckDialog>() {
        Some(DialogResult::Ok(CheckChoice::CheckNow | CheckChoice::CheckAgain)) => app.check_updates_requested = true,
        Some(DialogResult::Ok(CheckChoice::RestartToUpdate)) => app.updater.restart_to_update(),
        Some(DialogResult::Ok(CheckChoice::Update)) => {
            app.updater.request_install(safe);
        }
        Some(DialogResult::Ok(CheckChoice::Download)) => ctx.open_url(egui::OpenUrl::new_tab(crate::app::ENCODERCRAFT_APP.releases_url())),
        // Close, Esc or the window's close button dismisses the outcome it showed.
        Some(_) => app.updater.dismiss_status(),
        None => {}
    }
    match app.updater.run(&mut app.dialogs, safe) {
        Action::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
        // Through the normal quit path, whose `on_exit` starts the install helper.
        Action::Close => app.quit_requested = true,
        _ => {}
    }

    let updater = &app.updater;
    let shows = result_window_shows(updater.status(), updater.prompt_open());
    let (status, installing, message) = (updater.status().clone(), updater.is_installing(), updater.status_message());
    let dialogs = &mut app.dialogs;
    if shows && !dialogs.is_open::<UpdateCheckDialog>() {
        dialogs.open(UpdateCheckDialog::new(env!("CARGO_PKG_VERSION")));
    } else if !shows && dialogs.is_open::<UpdateCheckDialog>() {
        dialogs.cancel::<UpdateCheckDialog>(ctx);
        // Hidden for the prompt, not dismissed: the outcome shows again after it.
        let _ = dialogs.take::<UpdateCheckDialog>();
    }
    if let Some(dialog) = dialogs.get_mut::<UpdateCheckDialog>() {
        dialog.set_status(status);
        dialog.set_installing(installing);
        dialog.set_message(message);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{Receiver, channel};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use craft_update::updater::{CheckSource, Updater};
    use encodercraft_engine::QueueManager;
    use encodercraft_presets::PresetRegistry;

    use super::*;

    /// Stands in for GitHub: every check sends one message and finds nothing newer.
    fn source() -> (CheckSource, Receiver<()>) {
        let (sent, asked) = channel();
        let ask: CheckSource = Arc::new(move || {
            let _ = sent.send(());
            Ok(None)
        });
        (ask, asked)
    }

    /// The app with an updater that asks `ask` and keeps its records in `root`, out of the user's folders.
    fn app(ask: CheckSource, root: &std::path::Path) -> EncoderCraftApp {
        let mut app = EncoderCraftApp::new(Arc::new(QueueManager::new_in_memory()), Arc::new(Mutex::new(PresetRegistry::default())));
        app.updater = Updater::new(&crate::app::ENCODERCRAFT_APP, "0.0.1")
            .with_source(ask)
            .with_paths(craft_update::Paths::under(root))
            .with_pins_path(root.join("pins.json"));
        app
    }

    /// One frame of the app as the desktop runner draws it, with craft-ui's host; returns the text it drew.
    fn frame(app: &mut EncoderCraftApp, ctx: &egui::Context) -> Vec<String> {
        fn walk(shape: &egui::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, texts)),
                egui::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
                _ => {}
            }
        }
        app.prepare_frame(ctx);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| app.render(ui));
        out.textures_delta.clear();
        let mut texts = Vec::new();
        out.shapes.iter().for_each(|clipped| walk(&clipped.shape, &mut texts));
        texts
    }

    /// Draws frames until `done`, for at most five seconds (the check and the offer run on threads).
    fn frames_until(app: &mut EncoderCraftApp, ctx: &egui::Context, done: impl Fn(&EncoderCraftApp) -> bool) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut texts = frame(app, ctx);
        while !done(app) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            texts = frame(app, ctx);
        }
        texts
    }

    #[test]
    fn the_check_at_start_runs_only_when_the_desktop_app_turns_it_on() {
        for on in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (ask, asked) = source();
            let mut app = app(ask, root.path());
            app.set_startup_update_check(on);
            let ctx = egui::Context::default();
            crate::install_ui_runtime(&ctx);
            frame(&mut app, &ctx);
            frame(&mut app, &ctx);
            // Dropping the app drops the last sender once no check holds it, so this cannot hang.
            drop(app);
            assert_eq!(asked.recv().is_ok(), on, "check at start turned on: {on}");
        }
    }

    #[test]
    fn a_new_release_shows_the_shared_prompt_named_for_the_app() {
        let root = tempfile::tempdir().unwrap();
        let (ask, _asked) = source();
        let mut app = app(ask, root.path());
        let ctx = egui::Context::default();
        crate::install_ui_runtime(&ctx);
        frame(&mut app, &ctx);
        app.updater.offer(craft_update::Release {
            tag: "v99.0.0".into(),
            version: craft_update::parse_version("99.0.0"),
            name: "encoderCraft 99.0.0".into(),
            html_url: "https://github.com/el-j/encoderCraft/releases/tag/v99.0.0".into(),
            ..Default::default()
        });
        frames_until(&mut app, &ctx, |app| app.updater.prompt_open());
        assert!(app.updater.prompt_open(), "the offer opens craft-update's prompt");
        let texts = frame(&mut app, &ctx);
        assert!(texts.iter().any(|text| text.contains("encoderCraft")), "the prompt names the app from craft-ui's host: {texts:?}");
        assert!(blocks_input(&app), "the prompt holds the keyboard");
    }

    #[test]
    fn an_update_waits_while_the_queue_is_encoding() {
        let can = |worker_running, jobs: &[JobStatus]| can_restart(worker_running, jobs.iter());
        assert!(can(false, &[]), "an empty, idle queue");
        assert!(can(false, &[JobStatus::Queued, JobStatus::Completed, JobStatus::Failed("x".into()), JobStatus::Cancelled]), "nothing running");
        assert!(!can(true, &[JobStatus::Queued]), "the queue is working through its jobs");
        assert!(!can(false, &[JobStatus::Processing]), "a job is encoding");
        assert!(!can(false, &[JobStatus::Paused]), "a job is paused part-way");
    }
}
