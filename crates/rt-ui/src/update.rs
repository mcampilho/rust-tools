//! "Check for Updates…" in every app: the check and the download on a
//! thread of their own, and the questions on the way, in the system's own
//! message boxes (zenity or kdialog on Linux). Nothing goes to the network
//! unless the user asks (see `rt_sys::update`).
//!
//! The app sends the steps back to its event loop and hands each one to
//! [`step`], which asks what to ask and says what the app does next.

use rt_core::i18n::{tf, tr};
use rt_sys::update::{self, Found, Release};

/// What comes back from the threads.
#[derive(Debug)]
pub enum Step {
    Checked(Result<Found, String>),
    /// The version installed, or why not.
    Installed(String, Result<std::path::PathBuf, String>),
}

/// What the app does after a step.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    Nothing,
    /// Ends: it starts again (or Scoop updates it).
    Quit,
}

/// The app's identity, and where its messages go.
pub struct Who {
    pub app: &'static str,
    pub version: &'static str,
    /// The window the questions belong to (Windows).
    #[cfg(windows)]
    pub hwnd: rt_sys::win::HWND,
}

/// Starts a check; its answer comes through `send`.
pub fn check(app: &'static str, version: &'static str, send: Box<dyn Fn(Step) + Send>) {
    std::thread::spawn(move || send(Step::Checked(update::check(app, version))));
}

fn install(who: &Who, rel: Release, send: Box<dyn Fn(Step) + Send>) {
    let (app, version) = (who.app, who.version);
    std::thread::spawn(move || {
        let r = update::install(app, version, &rel);
        send(Step::Installed(rel.version, r));
    });
}

fn info(who: &Who, title: &str, text: &str) {
    #[cfg(windows)]
    {
        use rt_sys::win::dialogs::{message, Buttons, Icon};
        message(who.hwnd, title, text, Buttons::Ok, Icon::Info);
    }
    #[cfg(not(windows))]
    {
        let _ = who;
        rt_sys::linux::dialogs::info(title, text);
    }
}

fn error(who: &Who, title: &str, text: &str) {
    #[cfg(windows)]
    {
        use rt_sys::win::dialogs::{message, Buttons, Icon};
        message(who.hwnd, title, text, Buttons::Ok, Icon::Error);
    }
    #[cfg(not(windows))]
    {
        let _ = who;
        rt_sys::linux::dialogs::error(title, text);
    }
}

fn ask(who: &Who, title: &str, text: &str) -> bool {
    #[cfg(windows)]
    {
        use rt_sys::win::dialogs::{message, Answer, Buttons, Icon};
        message(who.hwnd, title, text, Buttons::OkCancel, Icon::Question) == Answer::Ok
    }
    #[cfg(not(windows))]
    {
        let _ = who;
        rt_sys::linux::dialogs::confirm(title, text)
    }
}

/// Handles one step: tells, asks, and starts the next one through `send`.
pub fn step(who: &Who, s: Step, send: Box<dyn Fn(Step) + Send>) -> Next {
    let title = tr("Check for updates");
    match s {
        Step::Checked(Ok(Found::UpToDate)) => {
            info(who, title, &tf("You have the latest version of {} ({}).", &[who.app, who.version]));
            Next::Nothing
        }
        Step::Checked(Ok(Found::Newer(rel))) => {
            let news = tf("{} {} is available (you have {}).", &[who.app, &rel.version, who.version]);
            if update::installed_by_scoop(who.app) {
                let text = format!(
                    "{news}\n\n{}",
                    tf(
                        "{} was installed with Scoop. Update it with Scoop now? {} closes, and Scoop updates it in a console window.",
                        &[who.app, who.app]
                    )
                );
                if ask(who, title, &text) {
                    match update::scoop_update(who.app) {
                        Ok(()) => return Next::Quit,
                        Err(e) => error(who, title, &tf("Scoop could not be started: {}", &[&e])),
                    }
                }
                return Next::Nothing;
            }
            let text = format!(
                "{news}\n\n{}\n{}",
                tr("Download and install it now? The download is checked against its published SHA-256 sum."),
                tf("What is new: {}", &[&rel.page])
            );
            if ask(who, title, &text) {
                install(who, rel, send);
            }
            Next::Nothing
        }
        Step::Checked(Err(e)) => {
            error(who, title, &tf("Could not check for updates: {}", &[&e]));
            Next::Nothing
        }
        Step::Installed(v, Ok(exe)) => {
            if ask(who, title, &tf("{} was updated to {}. Start the new version now?", &[who.app, &v])) {
                match update::restart(&exe) {
                    Ok(()) => return Next::Quit,
                    Err(e) => error(who, title, &tf("It could not be started: {}", &[&e])),
                }
            }
            Next::Nothing
        }
        Step::Installed(_, Err(e)) => {
            error(who, title, &tf("The update failed, and nothing was changed: {}", &[&e]));
            Next::Nothing
        }
    }
}
