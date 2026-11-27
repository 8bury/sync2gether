use crate::{
    guest::{Guest, PlayerStatus},
    stream::StreamClient,
};
use sync2gether_core::{
    chunks::{CHUNK_SIZE, Identifier},
    protocol::{Control, Media, StreamProgress, Verification},
    subtitle::{SubtitleError, from_bytes, shift_webvtt},
};
use wasm_bindgen::prelude::*;

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

#[wasm_bindgen]
pub struct WebGuest(Guest);

#[wasm_bindgen]
impl WebGuest {
    #[wasm_bindgen(constructor)]
    pub fn new(code: &str, name: &str) -> Result<WebGuest, JsError> {
        Guest::new(code, name)
            .map(WebGuest)
            .map_err(|error| JsError::new(&error))
    }
    pub fn start(&mut self, now: f64) {
        self.0.start(now);
    }
    pub fn socket_open(&mut self, now: f64) {
        self.0.socket_open(now);
    }
    pub fn socket_data(&mut self, bytes: &[u8], now: f64) {
        self.0.socket_data(bytes, now);
    }
    pub fn socket_closed(&mut self, now: f64) {
        self.0.socket_closed(now);
    }
    pub fn tick(&mut self, now: f64) {
        self.0.tick(now);
    }
    pub fn leave(&mut self, now: f64) {
        self.0.leave(now);
    }
    pub fn play(&mut self, now: f64) {
        self.0.control(Control::Play, now);
    }
    pub fn pause(&mut self, now: f64) {
        self.0.control(Control::Pause, now);
    }
    pub fn seek(&mut self, position: f64, now: f64) {
        self.0.control(Control::Seek(position), now);
    }
    pub fn set_media(
        &mut self,
        hash: Option<String>,
        bytes: f64,
        file_name: Option<String>,
    ) -> u32 {
        let media = hash
            .map(|hash| Media {
                hash,
                bytes: bytes.max(0.0) as u64,
            })
            .filter(Media::valid);
        self.0.set_media(media, file_name) as u32
    }
    pub fn set_loading(&mut self, file_name: Option<String>, done: f64, total: f64) {
        self.0.set_loading(
            file_name,
            Some(Verification {
                bytes: done.max(0.0) as u64,
                total: total.max(0.0) as u64,
            }),
        );
    }
    pub fn generation(&self) -> u32 {
        self.0.generation() as u32
    }
    pub fn player_status(&mut self, status: &str, now: f64) {
        if let Ok(status) = serde_json::from_str::<PlayerStatus>(status) {
            self.0.player_status(status, now);
        }
    }
    pub fn player_prepared(&mut self, token: u32, now: f64) {
        self.0.player_prepared(token.into(), now);
    }
    pub fn player_started(&mut self, token: u32, at: f64, now: f64) {
        self.0.player_started(token.into(), at, now);
    }
    pub fn player_missed(&mut self, token: u32, now: f64) {
        self.0.player_missed(token.into(), now);
    }
    pub fn player_error(&mut self, error: String, now: f64) {
        self.0.player_error(error, now);
    }
    pub fn take_output(&mut self) -> Vec<u8> {
        self.0.take_output()
    }
    pub fn take_commands(&mut self) -> String {
        json(&self.0.take_commands())
    }
    pub fn view(&self) -> String {
        json(&self.0.view())
    }
    pub fn stream_offer(&mut self, id: u32, sdp: String, now: f64) -> bool {
        self.0.stream_offer(id.into(), sdp, now)
    }
    pub fn stream_started(&mut self, hash: String, bytes: f64, file_name: String) -> u32 {
        let media = Media {
            hash,
            bytes: bytes.max(0.0) as u64,
        };
        if !media.valid() {
            return self.0.generation() as u32;
        }
        self.0.stream_started(media, file_name) as u32
    }
    pub fn stream_progress(&mut self, progress: &str) {
        if let Ok(progress) = serde_json::from_str::<StreamProgress>(progress) {
            self.0.stream_progress(progress);
        }
    }
    pub fn stop_stream(&mut self, now: f64) {
        self.0.stop_stream(now);
    }
    pub fn resume_key(&self) -> Option<Vec<u8>> {
        self.0.resume_key().map(|key| key.to_vec())
    }
    pub fn resume_with(&mut self, key: &[u8]) {
        if let Ok(key) = key.try_into() {
            self.0.resume_with(key);
        }
    }
}

#[wasm_bindgen]
pub struct FileHasher(Option<Identifier>);

#[wasm_bindgen]
impl FileHasher {
    #[wasm_bindgen(constructor)]
    pub fn new() -> FileHasher {
        FileHasher(Some(Identifier::new(CHUNK_SIZE)))
    }
    pub fn chunk_size() -> u32 {
        CHUNK_SIZE
    }
    pub fn update(&mut self, data: &[u8]) {
        if let Some(id) = &mut self.0 {
            id.update(data);
        }
    }
    pub fn finish(&mut self) -> String {
        match self.0.take() {
            Some(id) => json(&id.finish().0),
            None => "null".into(),
        }
    }
}

impl Default for FileHasher {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
pub struct WebStreamClient(StreamClient);

#[wasm_bindgen]
impl WebStreamClient {
    #[wasm_bindgen(constructor)]
    pub fn new(file_size: f64, chunk_size: u32, digest: &str, budget: f64) -> WebStreamClient {
        WebStreamClient(StreamClient::new(
            file_size.max(0.0) as u64,
            chunk_size,
            digest,
            budget.max(0.0) as u64,
        ))
    }
    pub fn on_message(&mut self, bytes: &[u8]) {
        self.0.on_message(bytes);
    }
    pub fn take_requests(&mut self) -> Vec<u8> {
        self.0.take_requests().concat()
    }
    pub fn read(&mut self, start: f64, end: f64) -> Option<Vec<u8>> {
        self.0.read(start.max(0.0) as u64, end.max(0.0) as u64)
    }
    pub fn ready(&self) -> bool {
        self.0.ready()
    }
    pub fn error(&self) -> Option<String> {
        self.0.error().map(str::to_owned)
    }
    pub fn file_size(&self) -> f64 {
        self.0.file_size() as f64
    }
    pub fn received_bytes(&self) -> f64 {
        self.0.received_bytes() as f64
    }
    pub fn reset_transport(&mut self) {
        self.0.reset_transport();
    }
    pub fn progress(&self, transfer_bps: f64, reconnecting: bool) -> String {
        json(&self.0.progress(transfer_bps.max(0.0) as u64, reconnecting))
    }
}

#[wasm_bindgen]
pub fn parse_subtitle(bytes: &[u8]) -> Result<String, JsError> {
    from_bytes(bytes).map_err(|error| {
        JsError::new(match error {
            SubtitleError::TooLarge => "A legenda é grande demais (limite de 1 MiB).",
            SubtitleError::Empty | SubtitleError::InvalidFormat | SubtitleError::InvalidName => {
                "Não foi possível ler esta legenda. Escolha um arquivo .srt ou .vtt válido."
            }
        })
    })
}

#[wasm_bindgen]
pub fn shift_subtitle(vtt: &str, delay_ms: i32) -> String {
    shift_webvtt(vtt, i64::from(delay_ms))
}
