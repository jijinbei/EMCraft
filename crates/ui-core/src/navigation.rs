//! The 3D view's navigation choices: what an orbit turns about, the orientation control.

emcraft_macros::named_enum! {
    /// What an orbit turns about.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum OrbitAround {
        /// The view's centre (its target): what Pan moves and Fit frames.
        #[default]
        ViewCentre = "view_centre" => "View centre",
        /// The model's origin, kept still on screen.
        Origin = "origin" => "Origin",
        /// The model point under the cursor when the drag began (over empty space, the middle of
        /// what is shown).
        Cursor = "cursor" => "Under the cursor",
    }
}
emcraft_macros::named_serde!(OrbitAround);

emcraft_macros::named_enum! {
    /// The view's orientation control in its corner.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Navigator {
        /// A cube: its faces and corners turn to that view, a drag on it orbits.
        #[default]
        Cube = "cube" => "Cube",
        /// The six signed axes.
        Axes = "axes" => "Axes",
    }
}
emcraft_macros::named_serde!(Navigator);
