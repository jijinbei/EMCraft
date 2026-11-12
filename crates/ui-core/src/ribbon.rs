//! The tabs of the ribbon the window had until the model tree took its commands
//! (docs/adr/0009-model-tree-window.md; each step's ＋, [`crate::outline::adds`]): named still in
//! the recordings and themes made then.

emcraft_macros::named_enum! {
    /// A tab of the former ribbon: its id is its title.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum RibbonTab {
        #[default]
        Model = "MODEL",
        Materials = "MATERIALS",
        PortsSources = "PORTS & SOURCES",
        Boundaries = "BOUNDARIES",
        Mesh = "MESH",
        Simulate = "SIMULATE",
        Results = "RESULTS",
        /// A system's chain: its stages, source and receiver.
        System = "SYSTEM",
    }
}
emcraft_macros::named_serde!(RibbonTab);
