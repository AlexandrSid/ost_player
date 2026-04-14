#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    Navigate(Screen),
    SetStatus(String),
    ClearStatus,

    SelectFolderDelta(i32),
    SelectPlaylistDelta(i32),

    // Player
    PlayerLoadFromLibrary { start_index: usize },
    PlayerTogglePlayPause,
    PlayerStop,
    PlayerNext,
    PlayerPrev,
    PlayerSeekRelativeSeconds(i64),

    // Config mutations
    AddFolder(String),
    RemoveFolderAt(usize),
    SetMinSizeBytes(u64),
    ToggleShuffle,
    CycleRepeat,

    // Indexer
    RescanLibrary,

    // Playlist mutations
    CreatePlaylist { name: String },
    RenamePlaylist { idx: usize, name: String },
    DeletePlaylist { idx: usize },
    OverwritePlaylistWithCurrent { idx: usize },
    LoadPlaylist { idx: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    MainMenu,
    Settings,
    Playlists,
    Folders,
    NowPlaying,
}

