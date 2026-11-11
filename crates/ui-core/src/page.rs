//! What fills the window under the app bar: one page at a time, each a tab of the app bar —
//! the start page and the project always, the settings while they are open. Which tab shows,
//! which are there and where closing the settings goes back to all follow from [`Pages`]; one
//! action changes it (the window's `Action::Show`, `Action::OpenSettings`,
//! `Action::CloseSettings`).

emcraft_macros::named_enum! {
    /// A section of the settings page.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Section {
        #[default]
        Appearance = "appearance" => "Appearance",
        Language = "language" => "Language",
        Units = "units" => "Units",
        View3d = "3d" => "3D view",
        Dialogs = "dialogs" => "Dialogs",
        Files = "files" => "Files",
    }
}
emcraft_macros::named_serde!(Section);

emcraft_macros::named_enum! {
    /// A page of the window, a tab of the app bar.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub enum Tab {
        /// The start page: new projects, recent ones, templates.
        Start = "start" => "Start",
        /// The open project's workspace (its tab reads the project's name).
        #[default]
        Project = "project" => "Project",
        /// The settings, while they are open.
        Settings = "settings" => "Settings",
    }
}
emcraft_macros::named_serde!(Tab);

/// The window's pages: the tab shown, the settings' section while they are open, and the tab
/// they were opened from (closing them goes back to it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pages {
    shown: Tab,
    settings: Option<Section>,
    back: Tab,
}

impl Pages {
    /// The pages showing `tab` (the settings, at their first section, when `Settings`).
    ///
    /// ```
    /// use emcraft_ui_core::page::{Pages, Section, Tab};
    ///     ///
    /// let mut p = Pages::at(Tab::Start);
    /// assert_eq!(p.tabs(), [Tab::Start, Tab::Project]);
    /// p.open_settings(Some(Section::Language));
    /// assert_eq!((p.shown(), p.tabs()), (Tab::Settings, vec![Tab::Start, Tab::Project, Tab::Settings]));
    /// // Another tab while the settings are open: they stay a tab.
    /// p.show(Tab::Project);
    /// assert_eq!((p.shown(), p.settings()), (Tab::Project, Some(Section::Language)));
    /// p.show(Tab::Settings);
    /// p.close_settings();
    /// assert_eq!((p.shown(), p.tabs()), (Tab::Project, vec![Tab::Start, Tab::Project]));
    /// ```
    pub fn at(tab: Tab) -> Pages {
        let mut p = Pages { shown: Tab::Project, settings: None, back: Tab::Project };
        p.show(tab);
        p
    }

    /// The tab shown.
    pub const fn shown(self) -> Tab {
        self.shown
    }

    /// Whether the start page shows.
    pub fn start(self) -> bool {
        self.shown == Tab::Start
    }

    /// The settings' section while they show.
    pub fn settings_shown(self) -> Option<Section> {
        self.settings.filter(|_| self.shown == Tab::Settings)
    }

    /// The settings' section while they are open (shown or not).
    pub const fn settings(self) -> Option<Section> {
        self.settings
    }

    /// The tabs there are, in the bar's order.
    pub fn tabs(self) -> Vec<Tab> {
        let mut out = vec![Tab::Start, Tab::Project];
        if self.settings.is_some() {
            out.push(Tab::Settings);
        }
        out
    }

    /// Shows `tab` (the settings open at their last or first section).
    pub fn show(&mut self, tab: Tab) {
        if tab == Tab::Settings {
            self.open_settings(None);
            return;
        }
        self.shown = tab;
        self.back = tab;
    }

    /// Opens the settings at `section` (their last, else their first, when none) and shows them.
    pub fn open_settings(&mut self, section: Option<Section>) {
        self.settings = Some(section.or(self.settings).unwrap_or_default());
        self.shown = Tab::Settings;
    }

    /// The project's workspace instead of the start page (a project opened or begun there);
    /// the settings, when they show, stay, closing back to it.
    pub fn leave_start(&mut self) {
        match self.shown {
            Tab::Start => self.show(Tab::Project),
            Tab::Settings => self.back = Tab::Project,
            Tab::Project => {}
        }
    }

    /// Closes the settings; their tab showing, the tab they were opened from shows.
    pub fn close_settings(&mut self) {
        self.settings = None;
        if self.shown == Tab::Settings {
            self.shown = self.back;
        }
    }
}
