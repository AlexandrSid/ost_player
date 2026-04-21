#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    Navigate(Screen),
    SetStatus(String),
    ClearStatus,

    /// Best-effort signal from the terminal event loop to the player thread.
    /// Used to control periodic snapshot emission and TUI refresh cadence.
    PlayerSetUiActivity {
        focused: bool,
        minimized: bool,
    },

    SelectFolderDelta(i32),
    SelectPlaylistDelta(i32),

    // Player
    PlayerLoadFromLibrary {
        start_index: usize,
    },
    PlayerTogglePlayPause,
    PlayerStop,
    PlayerNext,
    PlayerPrev,
    PlayerSeekRelativeSeconds(i64),
    VolumeUp,
    VolumeDown,

    // Config mutations
    AddFolder(String),
    RemoveFolderAt(usize),
    ToggleFolderRootOnlyAt(usize),
    /// Set or clear per-folder custom min_size_kb override (None = use global default).
    SetFolderCustomMinSizeKb {
        idx: usize,
        custom_kb: Option<u32>,
    },
    SetMinSizeKb(u64),
    ToggleShuffle,
    CycleRepeat,

    // Indexer
    RescanLibrary,

    // Playlist mutations
    CreatePlaylist {
        name: String,
    },
    RenamePlaylist {
        idx: usize,
        name: String,
    },
    DeletePlaylist {
        idx: usize,
    },
    OverwritePlaylistWithCurrent {
        idx: usize,
    },
    LoadPlaylist {
        idx: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    MainMenu,
    Settings,
    Playlists,
    Folders,
    NowPlaying,
}
