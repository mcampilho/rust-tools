//! The dynamic wallpaper, seen from the settings window: starting and
//! stopping the player, its events, the next video, choosing what plays.

use super::*;

impl Shell {
    /// The dynamic wallpaper is turned on (and can play here).
    pub(super) fn live_on(&self) -> bool {
        cfg!(windows) && self.settings.live.on
    }

    /// Plays what the settings say, or stops when it is turned off.
    pub(super) fn apply_live(&mut self) {
        #[cfg(windows)]
        {
            if !self.settings.live.on {
                if let Some(p) = &self.player {
                    p.stop();
                }
                self.live_state = Some(State::Off);
                return;
            }
            let l = &mut self.settings.live;
            if l.kind == Kind::Folder && l.show().is_none() {
                match live::pick_video(&l.folder, l.subfolders, &l.playing) {
                    Some(v) => {
                        l.playing = v;
                        self.load_live_thumb();
                    }
                    None => return self.live_failed(tr("There are no videos in the chosen folder.")),
                }
            }
            let Some(spec) = self.settings.live.spec() else { return self.live_failed(tr("Nothing chosen to play.")) };
            if self.player.is_none() {
                let proxy = std::sync::Mutex::new(self.proxy.clone());
                self.player = live::player::Player::start(Box::new(move |s| {
                    if let Ok(p) = proxy.lock() {
                        let _ = p.send_event(UserEvent::Live(s));
                    }
                }));
            }
            let Some(player) = &self.player else { return self.live_failed("the player could not start".into()) };
            self.live_error = None;
            self.live_state = None;
            player.play(spec);
            player.pause(self.live_paused);
            self.save();
        }
    }

    /// Turns the dynamic wallpaper off, saying why.
    #[cfg(windows)]
    pub(super) fn live_failed(&mut self, why: String) {
        self.settings.live.on = false;
        self.live_error = Some(tf("The dynamic wallpaper cannot play: {}", &[&why]));
        self.live_state = Some(State::Off);
        if let Some(p) = &self.player {
            p.stop();
        }
        self.save();
        self.redraw();
    }

    #[cfg(windows)]
    pub(super) fn live_event(&mut self, s: State) {
        if let State::Failed(e) = &s {
            let e = e.clone();
            self.live_failed(e);
            return;
        }
        self.live_state = Some(s);
        self.redraw();
    }

    /// Another video from the folder.
    pub(super) fn next_video(&mut self) {
        let l = &mut self.settings.live;
        if let Some(v) = live::pick_video(&l.folder, l.subfolders, &l.playing) {
            l.playing = v;
            self.save();
            self.load_live_thumb();
            if self.live_on() {
                self.apply_live();
            }
        }
        self.redraw();
    }

    pub(super) fn load_live_thumb(&mut self) {
        self.live_thumb = None;
        let Some(show) = self.settings.live.show() else { return };
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let thumb = std::panic::catch_unwind(|| live::preview(&show, THUMB.0, THUMB.1)).ok().flatten();
            let _ = proxy.send_event(UserEvent::LiveThumb(thumb));
        });
    }

    /// Asks for the file (or folder) of `kind`; true when one was chosen.
    pub(super) fn choose_live(&mut self, kind: Kind) -> bool {
        let l = &self.settings.live;
        let current = l.file(kind);
        let dir = if current.is_dir() { Some(current.to_path_buf()) } else { current.parent().map(|p| p.to_path_buf()) };
        let chosen = match kind {
            Kind::Scene => None,
            Kind::Folder => {
                let title = tr("Choose a folder of videos");
                #[cfg(windows)]
                let f = rt_sys::win::dialogs::pick_folder(self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd), &title);
                #[cfg(not(windows))]
                let f = rt_sys::linux::dialogs::pick_folder(&title);
                f
            }
            Kind::Video | Kind::Anim => {
                let (label, exts) = if kind == Kind::Video { (tr("Videos"), &live::VIDEO_EXTENSIONS[..]) } else { (tr("Animated pictures"), &live::ANIM_EXTENSIONS[..]) };
                let pattern: String = exts.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";");
                #[cfg(windows)]
                let f = {
                    let filter = rt_sys::win::dialogs::filter(&[(label, &pattern)]);
                    rt_sys::win::dialogs::open_file(self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd), dir.as_deref(), &filter)
                };
                #[cfg(not(windows))]
                let f = rt_sys::linux::dialogs::open_files(dir.as_deref(), &[(label, &pattern)], false).into_iter().next();
                f
            }
        };
        let Some(path) = chosen else { return false };
        let l = &mut self.settings.live;
        match kind {
            Kind::Video => l.video = path,
            Kind::Anim => l.anim = path,
            Kind::Folder => {
                l.folder = path;
                l.playing = std::path::PathBuf::new();
            }
            Kind::Scene => {}
        }
        l.kind = kind;
        true
    }

    /// After a change to what plays: a new still, and play it if on.
    pub(super) fn live_changed(&mut self) {
        self.live_error = None;
        let l = &mut self.settings.live;
        if l.kind == Kind::Folder && l.show().is_none() {
            if let Some(v) = live::pick_video(&l.folder, l.subfolders, &l.playing) {
                l.playing = v;
            }
        }
        self.load_live_thumb();
        if self.live_on() {
            self.apply_live();
        }
        self.save();
    }

    pub(super) fn live_model(&self) -> (String, String, String, bool) {
        let l = &self.settings.live;
        let title = match l.show() {
            Some(Show::Scene(s)) => tr(s.label()),
            Some(s) => s.file().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            None if l.kind == Kind::Folder => l.folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            None => String::new(),
        };
        let state = if !l.on {
            tr("Off")
        } else {
            match &self.live_state {
                None => tr("Starting…"),
                Some(State::Playing) => tr("Playing behind the desktop icons"),
                Some(State::Paused(Reason::User)) => tr("Paused from the menu of the icon"),
                Some(State::Paused(Reason::When(p))) => tr(match p {
                    PauseWhen::FullScreen => "Paused: a program or a game is in full screen",
                    PauseWhen::Maximized => "Paused: maximized windows cover every screen",
                    PauseWhen::Locked => "Paused: the computer is locked or the screen is off",
                    PauseWhen::Battery => "Paused: the computer runs on battery",
                }),
                Some(State::Off | State::Failed(_)) => tr("Off"),
            }
        };
        let note = if self.tray {
            tr("Icons, files, notifications and widgets stay over it, as always.")
        } else {
            tr("The dynamic wallpaper stops when this window closes; with rwall --tray it keeps playing.")
        };
        let can_show = l.show().and_then(|s| s.file().map(|f| f.exists())).unwrap_or(false);
        (title, state, note, can_show)
    }

    pub(super) fn toggle_live_pause(&mut self) {
        self.live_paused = !self.live_paused;
        #[cfg(windows)]
        if let Some(p) = &self.player {
            p.pause(self.live_paused);
        }
        self.redraw();
    }
}
