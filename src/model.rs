use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Stage {
    Inspect,
    Metadata,
    Verify,
    Build,
    Package,
    Release,
    Upload,
    Binstall,
}

impl Stage {
    pub fn order(self) -> usize {
        self as usize
    }
}
