use super::{Role, Worker};
use crate::{
    player,
    protocol::{MAX_SUBTITLE_BYTES, Message, SubtitleChunk},
};
use std::path::PathBuf;
use sync2gether_core::subtitle::{SubtitleError, from_bytes, split_chunks};

pub(super) struct SharedSubtitle {
    chunks: Vec<SubtitleChunk>,
}

pub(super) struct SubtitleLoad {
    result: Result<Option<(PathBuf, String, Vec<SubtitleChunk>)>, String>,
}

fn load_subtitle_file(
    path: PathBuf,
    id: u64,
) -> Result<(PathBuf, String, Vec<SubtitleChunk>), String> {
    let errors = &crate::i18n::text().errors;
    let metadata = std::fs::metadata(&path).map_err(|_| errors.subtitle_read_failed.to_owned())?;
    if !metadata.is_file() {
        return Err(errors.not_regular_file.to_owned());
    }
    if metadata.len() > MAX_SUBTITLE_BYTES as u64 {
        return Err(errors.subtitle_too_large.to_owned());
    }
    let bytes = std::fs::read(&path).map_err(|_| errors.subtitle_read_failed.to_owned())?;
    let vtt = from_bytes(&bytes).map_err(|error| match error {
        SubtitleError::TooLarge => errors.subtitle_too_large.to_owned(),
        SubtitleError::Empty | SubtitleError::InvalidFormat | SubtitleError::InvalidName => {
            errors.subtitle_invalid.to_owned()
        }
    })?;
    let name = crate::media::display_name(&path);
    let chunks = split_chunks(id, &name, &vtt).map_err(|error| match error {
        SubtitleError::TooLarge => errors.subtitle_too_large.to_owned(),
        SubtitleError::Empty | SubtitleError::InvalidFormat | SubtitleError::InvalidName => {
            errors.subtitle_invalid.to_owned()
        }
    })?;
    let canonical = path.canonicalize().unwrap_or(path);
    Ok((canonical, name, chunks))
}

impl Worker {
    pub(super) fn share_subtitle(&mut self, path: Option<PathBuf>) {
        if self.view.role != Role::Host {
            return;
        }
        self.subtitle_id = self.subtitle_id.wrapping_add(1).max(1);
        let id = self.subtitle_id;
        let tx = self.subtitle_tx.clone();
        tokio::task::spawn_blocking(move || {
            let selected = path.or_else(|| {
                rfd::FileDialog::new()
                    .set_title(crate::i18n::text().errors.pick_subtitle_title)
                    .add_filter("SRT / WebVTT", &["srt", "vtt"])
                    .pick_file()
            });
            let result = selected
                .map(|path| load_subtitle_file(path, id))
                .transpose();
            let _ = tx.blocking_send(SubtitleLoad { result });
        });
    }

    pub(super) fn subtitle_event(&mut self, loaded: SubtitleLoad) {
        if self.view.role != Role::Host {
            return;
        }
        match loaded.result {
            Ok(Some((path, name, chunks))) => {
                self.view.error = None;
                self.view.shared_subtitle = Some(name);
                self.shared_subtitle = Some(SharedSubtitle { chunks });
                self.player(player::Command::AddSubtitle(path));
                self.queue_shared_subtitle();
                self.drain_subtitle_outbox();
            }
            Ok(None) => {}
            Err(error) => {
                self.view.error = Some(error);
            }
        }
    }

    pub(super) fn clear_shared_subtitle(&mut self, notify: bool) {
        let had = self.shared_subtitle.take().is_some();
        self.view.shared_subtitle = None;
        self.subtitle_outbox.clear();
        if notify && had && self.view.connected && self.session.state.guest.web_guest {
            self.subtitle_outbox.push_back(Message::SubtitleClear);
            self.drain_subtitle_outbox();
        }
    }

    pub(super) fn queue_shared_subtitle(&mut self) {
        self.subtitle_outbox.clear();
        if !self.view.connected || !self.session.state.guest.web_guest {
            return;
        }
        if let Some(shared) = &self.shared_subtitle {
            for chunk in &shared.chunks {
                self.subtitle_outbox
                    .push_back(Message::SubtitleChunk(Box::new(chunk.clone())));
            }
        }
    }

    pub(super) fn drain_subtitle_outbox(&mut self) {
        if !self.view.connected || !self.session.state.guest.web_guest {
            self.subtitle_outbox.clear();
            return;
        }
        let Some(outgoing) = &self.outgoing else {
            return;
        };
        while outgoing.capacity() > 16 {
            let Some(message) = self.subtitle_outbox.pop_front() else {
                break;
            };
            if outgoing.try_send(message).is_err() {
                self.disconnected();
                break;
            }
        }
    }
}
