//! Native mpv playlist and playback queue types and methods.

use crate::error::MpvError;
use crate::handle::MpvHandle;

/// An individual media item in mpv's native playback queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistItem {
    /// 0-based index within the playlist.
    pub index: usize,
    /// Stable unique identifier assigned by mpv.
    pub id: i64,
    /// Path or URL of the media file.
    pub filename: String,
    /// Media title if available from demuxer/metadata.
    pub title: Option<String>,
    /// Whether this entry is the currently selected playlist item.
    pub is_current: bool,
    /// Whether this entry is currently actively decoding and playing.
    pub is_playing: bool,
}

impl MpvHandle {
    /// Queries the full playlist currently loaded in the mpv core.
    pub fn get_playlist(&self) -> Result<Vec<PlaylistItem>, MpvError> {
        let count: i64 = match self.get_property("playlist-count") {
            Ok(c) => c,
            Err(_) => return Ok(Vec::new()),
        };

        let mut items = Vec::with_capacity(count as usize);
        for i in 0..count {
            let id: i64 = self
                .get_property(&format!("playlist/{i}/id"))
                .unwrap_or(i + 1);
            let filename: String = self
                .get_property_string(&format!("playlist/{i}/filename"))
                .unwrap_or_default();
            let title = self
                .get_property_string(&format!("playlist/{i}/title"))
                .ok();
            let is_current: bool = self
                .get_property(&format!("playlist/{i}/current"))
                .unwrap_or(false);
            let is_playing: bool = self
                .get_property(&format!("playlist/{i}/playing"))
                .unwrap_or(false);

            items.push(PlaylistItem {
                index: i as usize,
                id,
                filename,
                title,
                is_current,
                is_playing,
            });
        }

        Ok(items)
    }

    /// Appends a media file or URL to the end of the playlist.
    pub fn playlist_append(&self, path: &str) -> Result<(), MpvError> {
        self.command(&["loadfile", path, "append"])
    }

    /// Appends a media file or URL to the playlist and starts playback if currently idle.
    pub fn playlist_append_play(&self, path: &str) -> Result<(), MpvError> {
        self.command(&["loadfile", path, "append-play"])
    }

    /// Immediately jumps to and plays the playlist item at the given 0-based index.
    pub fn playlist_play_index(&self, index: usize) -> Result<(), MpvError> {
        self.command(&["playlist-play-index", &index.to_string()])
    }

    /// Advances to the next item in the playlist.
    pub fn playlist_next(&self) -> Result<(), MpvError> {
        self.command(&["playlist-next"])
    }

    /// Returns to the previous item in the playlist.
    pub fn playlist_prev(&self) -> Result<(), MpvError> {
        self.command(&["playlist-prev"])
    }

    /// Removes the playlist item at the given 0-based index.
    pub fn playlist_remove(&self, index: usize) -> Result<(), MpvError> {
        self.command(&["playlist-remove", &index.to_string()])
    }

    /// Moves a playlist item from `from_index` to `to_index`.
    pub fn playlist_move(&self, from_index: usize, to_index: usize) -> Result<(), MpvError> {
        self.command(&[
            "playlist-move",
            &from_index.to_string(),
            &to_index.to_string(),
        ])
    }

    /// Clears all entries from the playlist.
    pub fn playlist_clear(&self) -> Result<(), MpvError> {
        self.command(&["playlist-clear"])
    }

    /// Randomizes the order of items in the playlist.
    pub fn playlist_shuffle(&self) -> Result<(), MpvError> {
        self.command(&["playlist-shuffle"])
    }
}
