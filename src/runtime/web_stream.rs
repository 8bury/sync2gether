use super::{Role, Worker, stream::StreamPrepResult};
use crate::{
    protocol::{Message, WebStreamAnswer, WebStreamOffer},
    stream::{StreamPhase, StreamTelemetry, webrtc},
};
use std::time::{Duration, Instant};

const WEB_OFFER_COOLDOWN: Duration = Duration::from_secs(2);

impl Worker {
    pub(super) fn handle_web_stream_offer(&mut self, offer: WebStreamOffer) {
        if self.view.role != Role::Host || !offer.valid() || !self.host.guest_is_web() {
            return;
        }
        let (Some((path, index)), Some(media), Some(file_name)) = (
            self.local_media.clone(),
            self.presence.media.clone(),
            self.presence.file_name.clone(),
        ) else {
            self.send(Message::StreamStop { id: offer.id });
            return;
        };
        if self
            .last_stream_request
            .is_some_and(|at| at.elapsed() < WEB_OFFER_COOLDOWN)
        {
            self.send(Message::StreamStop { id: offer.id });
            return;
        }
        self.last_stream_request = Some(Instant::now());
        let _ = self.stop_stream_transport();
        let id = offer.id;
        self.stream_id = id;
        self.stream_waiting_since = Some(Instant::now());
        self.view.stream = StreamTelemetry {
            active: true,
            is_sender: true,
            phase: StreamPhase::Offering,
            total_bytes: media.bytes,
            chunks_total: index.hashes.len() as u64,
            ..Default::default()
        };
        self.presence.streaming = true;
        let duration = self.presence.duration;
        let stun = self.view.internet;
        let tx = self.stream_prep_tx.clone();
        tokio::spawn(async move {
            let result = match webrtc::prepare(&offer.sdp, path, index, duration, stun, !stun).await
            {
                Ok((sdp, host)) => StreamPrepResult::WebReady {
                    id,
                    answer: Box::new(WebStreamAnswer {
                        id,
                        sdp,
                        media,
                        file_name,
                        duration,
                        chunk_size: host.chunk_size(),
                        hash_table_digest: host.digest().to_owned(),
                        mse: host.mse_info(),
                        subtitle_tracks: host.subtitle_tracks(),
                    }),
                    host: Box::new(host),
                },
                Err(error) => StreamPrepResult::Failed { id, error },
            };
            let _ = tx.send(result).await;
        });
    }

    pub(super) fn web_stream_ready(&mut self, answer: WebStreamAnswer, host: webrtc::WebHost) {
        let id = answer.id;
        self.stream_waiting_since = None;
        self.view.stream.phase = StreamPhase::Connecting;
        self.embedded_subtitles = Some(host.embedded_subtitles());
        self.embedded_subtitle_cache.clear();
        self.send(Message::WebStreamAnswer(Box::new(answer)));
        self.stream_task = Some(host.run(id, self.stream_tx.clone()));
    }
}
