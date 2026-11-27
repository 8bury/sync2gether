use crate::{
    guest::{Guest, PlayerStatus},
    host::BrowserHost,
    host_stream::StreamServer,
    remux::MkvRemuxer,
    stream::StreamClient,
};
use js_sys::{Function, Uint8Array};
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use sync2gether_core::{
    chunks::{CHUNK_SIZE, Identifier, table_digest},
    protocol::{Control, Media, StreamProgress, Verification, WebStreamAnswer},
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
    pub fn request_subtitle(&mut self, track: u8, now: f64) {
        self.0.request_subtitle(track, now);
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
pub struct WebHost(BrowserHost);

#[wasm_bindgen]
impl WebHost {
    #[wasm_bindgen(constructor)]
    pub fn new(name: &str) -> WebHost {
        WebHost(BrowserHost::new(name))
    }
    pub fn restore(
        name: &str,
        secret: String,
        word0: u8,
        word1: u8,
        room: Option<String>,
        resume_key: Option<Vec<u8>>,
    ) -> WebHost {
        let key = resume_key.and_then(|bytes| bytes.try_into().ok());
        WebHost(BrowserHost::restore(
            name,
            secret,
            [word0, word1],
            room,
            key,
        ))
    }
    pub fn start(&mut self, now: f64) {
        self.0.start(now);
    }
    pub fn control_open(&mut self, now: f64) {
        self.0.control_open(now);
    }
    pub fn control_data(&mut self, bytes: &[u8], now: f64) {
        self.0.control_data(bytes, now);
    }
    pub fn control_closed(&mut self, now: f64) {
        self.0.control_closed(now);
    }
    pub fn accept_open(&mut self, id: u32, now: f64) {
        self.0.accept_open(id.into(), now);
    }
    pub fn accept_data(&mut self, id: u32, bytes: &[u8], now: f64) {
        self.0.accept_data(id.into(), bytes, now);
    }
    pub fn accept_closed(&mut self, id: u32, now: f64) {
        self.0.accept_closed(id.into(), now);
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
        now: f64,
    ) -> u32 {
        let media = hash
            .map(|hash| Media {
                hash,
                bytes: bytes.max(0.0) as u64,
            })
            .filter(Media::valid);
        self.0.set_media(media, file_name, now) as u32
    }
    pub fn set_loading(&mut self, file_name: Option<String>, done: f64, total: f64, now: f64) {
        self.0.set_loading(
            file_name,
            Some(Verification {
                bytes: done.max(0.0) as u64,
                total: total.max(0.0) as u64,
            }),
            now,
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
    pub fn take_control_output(&mut self) -> Vec<u8> {
        self.0.take_control_output()
    }
    pub fn take_accept_output(&mut self, id: u32) -> Vec<u8> {
        self.0.take_accept_output(id.into())
    }
    pub fn take_commands(&mut self) -> String {
        json(&self.0.take_commands())
    }
    pub fn view(&self) -> String {
        json(&self.0.view())
    }
    pub fn room(&self) -> Option<String> {
        self.0.room().map(str::to_owned)
    }
    pub fn secret(&self) -> String {
        self.0.secret().to_owned()
    }
    pub fn words(&self) -> Vec<u8> {
        self.0.words().to_vec()
    }
    pub fn resume_key(&self) -> Option<Vec<u8>> {
        self.0.resume_key().map(|key| key.to_vec())
    }
    pub fn set_stream_capability(&mut self, web_stream: bool, web_remux: bool, now: f64) {
        self.0.set_stream_capability(web_stream, web_remux, now);
    }
    pub fn stream_answer(&mut self, answer: &str, now: f64) -> bool {
        serde_json::from_str::<WebStreamAnswer>(answer)
            .is_ok_and(|answer| self.0.stream_answer(answer, now))
    }
    pub fn set_stream_active(&mut self, active: bool, now: f64) {
        self.0.set_stream_active(active, now);
    }
    pub fn stop_stream(&mut self, id: u32, now: f64) {
        self.0.stop_stream(id.into(), now);
    }
}

#[wasm_bindgen]
pub struct FileHasher {
    inner: Option<Identifier>,
    digest: String,
    table: Vec<u8>,
}

#[wasm_bindgen]
impl FileHasher {
    #[wasm_bindgen(constructor)]
    pub fn new() -> FileHasher {
        FileHasher {
            inner: Some(Identifier::new(CHUNK_SIZE)),
            digest: String::new(),
            table: Vec::new(),
        }
    }
    pub fn chunk_size() -> u32 {
        CHUNK_SIZE
    }
    pub fn update(&mut self, data: &[u8]) {
        if let Some(id) = &mut self.inner {
            id.update(data);
        }
    }
    pub fn finish(&mut self) -> String {
        match self.inner.take() {
            Some(id) => {
                let (media, hashes) = id.finish();
                self.digest = table_digest(&hashes);
                self.table = hashes.concat();
                json(&media)
            }
            None => "null".into(),
        }
    }
    pub fn digest(&self) -> String {
        self.digest.clone()
    }
    pub fn take_hash_table(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.table)
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

#[wasm_bindgen]
pub struct WebStreamServer(StreamServer);

#[wasm_bindgen]
impl WebStreamServer {
    #[wasm_bindgen(constructor)]
    pub fn new(
        file_size: f64,
        chunk_size: u32,
        hash_table: Vec<u8>,
        remux_enabled: bool,
    ) -> WebStreamServer {
        WebStreamServer(StreamServer::new(
            file_size.max(0.0) as u64,
            chunk_size,
            hash_table,
            remux_enabled,
        ))
    }
    pub fn on_request(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        self.0
            .on_request(bytes)
            .map_err(|error| JsError::new(&error))
    }
    pub fn next_action(&mut self) -> String {
        json(&self.0.next_action())
    }
    pub fn provide_chunk(&mut self, index: u32, data: &[u8]) {
        self.0.provide_chunk(index, data);
    }
    pub fn fail_chunk(&mut self, index: u32) {
        self.0.fail_chunk(index);
    }
    pub fn provide_mse_index(&mut self, data: &[u8]) {
        self.0.provide_mse_index(data);
    }
    pub fn fail_mse_index(&mut self) {
        self.0.fail_mse_index();
    }
    pub fn provide_mse_init(&mut self, track: u8, data: &[u8]) {
        self.0.provide_mse_init(track, data);
    }
    pub fn fail_mse_init(&mut self, track: u8) {
        self.0.fail_mse_init(track);
    }
    pub fn provide_mse_segment(&mut self, track: u8, segment: u32, data: &[u8]) {
        self.0.provide_mse_segment(track, segment, data);
    }
    pub fn fail_mse_segment(&mut self, track: u8, segment: u32) {
        self.0.fail_mse_segment(track, segment);
    }
    pub fn take_fragment(&mut self) -> Option<Vec<u8>> {
        self.0.take_fragment()
    }
    pub fn has_outgoing(&self) -> bool {
        self.0.has_outgoing()
    }
    pub fn sent_bytes(&self) -> f64 {
        self.0.sent_bytes() as f64
    }
    pub fn unique_chunks(&self) -> u32 {
        self.0.unique_chunks()
    }
    pub fn total_chunks(&self) -> u32 {
        self.0.total_chunks()
    }
}

struct JsFileReader {
    read_at: Function,
    size: u64,
    pos: u64,
}

impl Read for JsFileReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.size || buf.is_empty() {
            return Ok(0);
        }
        let want = (self.size - self.pos).min(buf.len() as u64) as u32;
        let value = self
            .read_at
            .call2(
                &JsValue::NULL,
                &JsValue::from_f64(self.pos as f64),
                &JsValue::from(want),
            )
            .map_err(|_| io::Error::other("Falha ao ler arquivo no navegador."))?;
        let array = Uint8Array::new(&value);
        let len = (array.length() as usize).min(buf.len());
        if len == 0 {
            return Ok(0);
        }
        array.slice(0, len as u32).copy_to(&mut buf[..len]);
        self.pos += len as u64;
        Ok(len)
    }
}

impl Seek for JsFileReader {
    fn seek(&mut self, style: SeekFrom) -> io::Result<u64> {
        let next = match style {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::End(delta) => i128::from(self.size) + i128::from(delta),
            SeekFrom::Current(delta) => i128::from(self.pos) + i128::from(delta),
        };
        if next < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Posição negativa",
            ));
        }
        self.pos = (next as u64).min(self.size);
        Ok(self.pos)
    }
}

#[wasm_bindgen]
pub struct WasmMkvRemuxer(MkvRemuxer<BufReader<JsFileReader>>);

#[wasm_bindgen]
impl WasmMkvRemuxer {
    pub fn open(file_size: f64, read_at: Function) -> Result<WasmMkvRemuxer, JsError> {
        let reader = BufReader::with_capacity(
            64 * 1024,
            JsFileReader {
                read_at,
                size: file_size.max(0.0) as u64,
                pos: 0,
            },
        );
        MkvRemuxer::open(reader)
            .map(WasmMkvRemuxer)
            .map_err(|error| JsError::new(&json(&error)))
    }
    pub fn mse_info(&self) -> String {
        json(&self.0.web_stream_mse())
    }
    pub fn skipped_audio_codecs(&self) -> String {
        json(&self.0.skipped_audio_codecs())
    }
    pub fn encoded_index(&self) -> Vec<u8> {
        self.0.encoded_index()
    }
    pub fn init_segment(&self, track: u8) -> Result<Vec<u8>, JsError> {
        self.0
            .init_segment(track)
            .map_err(|error| JsError::new(&error))
    }
    pub fn media_segment(&mut self, track: u8, segment: u32) -> Result<Vec<u8>, JsError> {
        self.0
            .media_segment(track, segment)
            .map_err(|error| JsError::new(&error))
    }
}
