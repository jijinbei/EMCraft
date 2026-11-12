//! The window's glyphs by name: what each command, view and tool shows beside its label. Each
//! is drawn from its shared marks ([`crate::glyph_marks`]) in the window's toolkit.

emcraft_macros::named_enum! {
    /// A glyph; its id names it (a picture sheet, a test).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Glyph {
        Waveguide = "waveguide",
        Box = "box",
        Cylinder = "cylinder",
        Sphere = "sphere",
        Feature = "feature",
        Inclusion = "inclusion",
        Import = "import",
        Material = "material",
        Walls = "walls",
        Port = "port",
        Lumped = "lumped",
        Dipole = "dipole",
        PlaneWave = "plane_wave",
        Open = "open",
        Mesh = "mesh",
        Sweep = "sweep",
        Variable = "variable",
        ParamSweep = "param_sweep",
        Optimize = "optimize",
        Eigen = "eigen",
        Radiation = "radiation",
        Time = "time",
        PortModes = "port_modes",
        Chart = "chart",
        Field = "field",
        Table = "table",
        Compare = "compare",
        Pattern = "pattern",
        Sample = "sample",
        Units = "units",
        Clear = "clear",
        Summary = "summary",
        Probe = "probe",
        Script = "script",
        Fit = "fit",
        Home = "home",
        Lookat = "lookat",
        Perspective = "perspective",
        Orthographic = "orthographic",
        New = "new",
        OpenFile = "open_file",
        Save = "save",
        Settings = "settings",
        Band = "band",
        Check = "check",
        Touchstone = "touchstone",
        KeepPlane = "keep_plane",
        Symmetry = "symmetry",
        Periodic = "periodic",
        Dispersion = "dispersion",
        Start = "start",
        Play = "play",
        Pause = "pause",
        /// A box cut open, its cut face filled: the section view.
        Section = "section",
        /// Two arrows turning back on each other: the other side.
        Flip = "flip",
        /// A ruler with its ticks: measure.
        Ruler = "ruler",
        /// A pencil: edit.
        Edit = "edit",
        /// A bin: delete.
        Delete = "delete",
        /// A face picked by a pointer: a port on a face.
        PickFace = "pick_face",
        /// Two points joined across a gap: a lumped port by two points.
        PickGap = "pick_gap",
        /// A two-port block, a wave in and out, its pass band over it: a frequency sweep.
        KindSweep = "kind_sweep",
        /// A closed box holding a standing wave: a resonator's eigenmodes.
        KindResonator = "kind_resonator",
        /// A dipole radiating inside a dashed layer: an open problem.
        KindAntenna = "kind_antenna",
        /// A wave packet running along a line: the time domain.
        KindPulse = "kind_pulse",
        /// The region solved: a box drawn dashed, faintly filled with its air.
        Region = "region",
        /// A metal body: a box with its faces filled.
        Metal = "metal",
        /// A dielectric body: a box with its front hatched.
        Dielectric = "dielectric",
        /// An imported part: a metal box with an arrow coming in.
        Part = "part",
        /// A symmetry plane that is an electric wall: a dashed plane with E across it.
        SymmetryElectric = "symmetry_electric",
        /// A symmetry plane that is a magnetic wall: a dashed plane with H across it.
        SymmetryMagnetic = "symmetry_magnetic",
        /// Field lines: arrows bending round.
        FieldLines = "field_lines",
    /// A plain rounded square: a command without a glyph of its own.
        Blank = "blank",
    }
}
