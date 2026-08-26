use std::path::PathBuf;

/// State of the app-server streaming path for this process.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppMode {
    Off,
    Ready,
    Downgraded,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum V4Mode {
    Unknown,
    Available,
    Unavailable,
}

/// Borrow-free tag for a connection handshake phase.
#[derive(Clone, Copy)]
pub(crate) enum ConnectStage {
    Create,
    Resume,
    Subscribe,
    V4Subscribe,
}

/// Availability of the kernel database used by optional live progress.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum DbState {
    Unknown,
    Enabled(PathBuf),
    Disabled,
}
