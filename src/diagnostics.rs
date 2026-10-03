//! Checkpoints sem I/O na UI; uma thread própria grava amostras de diagnóstico.
use std::{
    fs::{File, create_dir_all},
    io::{self, Write},
    path::PathBuf,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU8, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum Stage {
    Startup,
    NativeLoop,
    CreatingApp,
    Logic,
    ReadViews,
    DrawUi,
    UiDone,
    PaintLock,
    GlInitialize,
    GlUpdate,
    GlRender,
    GlBlit,
    PaintDone,
    GlDestroy,
    Exit,
    NativeError,
}
impl Stage {
    fn name(value: u8) -> &'static str {
        match value {
            0 => "startup",
            1 => "native-event-loop",
            2 => "creating-app",
            3 => "app-logic",
            4 => "reading-views",
            5 => "drawing-ui",
            6 => "ui-returned",
            7 => "waiting-paint-lock",
            8 => "mpv-gl-initialize",
            9 => "mpv-gl-update",
            10 => "mpv-gl-render",
            11 => "gl-blit",
            12 => "paint-returned",
            13 => "mpv-gl-destroy",
            14 => "exit",
            _ => "native-error",
        }
    }
}
struct State {
    origin: Instant,
    stage: AtomicU8,
    progress_ms: AtomicU64,
    window: AtomicU8,
    callback_ms: AtomicU64,
    callback_active: AtomicU8,
}
impl State {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
            stage: AtomicU8::new(Stage::Startup as u8),
            progress_ms: AtomicU64::new(0),
            window: AtomicU8::new(0),
            callback_ms: AtomicU64::new(0),
            callback_active: AtomicU8::new(0),
        }
    }
    fn now(&self) -> u64 {
        self.origin.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
    fn sample(&self, output: &mut impl Write) -> io::Result<()> {
        let now = self.now();
        let flags = self.window.load(Ordering::Relaxed);
        writeln!(
            output,
            "elapsed_ms={now} stage={} checkpoint_age_ms={} window_known={} focused={} minimized={} occluded={} media_loaded={} mpv_callback_active={} mpv_callback_age_ms={}",
            Stage::name(self.stage.load(Ordering::Relaxed)),
            now.saturating_sub(self.progress_ms.load(Ordering::Relaxed)),
            flags & 1 != 0,
            flags & 2 != 0,
            flags & 4 != 0,
            flags & 8 != 0,
            flags & 16 != 0,
            self.callback_active.load(Ordering::Relaxed) != 0,
            now.saturating_sub(self.callback_ms.load(Ordering::Relaxed)),
        )
    }
}
static STATE: OnceLock<Arc<State>> = OnceLock::new();

pub fn mark(stage: Stage) {
    if let Some(state) = STATE.get() {
        state.stage.store(stage as u8, Ordering::Relaxed);
        state.progress_ms.store(state.now(), Ordering::Relaxed);
    }
}
pub(crate) fn window(ctx: &eframe::egui::Context, media_loaded: bool) {
    if let Some(state) = STATE.get() {
        let flags = ctx.input(|i| {
            let v = i.viewport();
            1 | (u8::from(v.focused.unwrap_or(false)) << 1)
                | (u8::from(v.minimized.unwrap_or(false)) << 2)
                | (u8::from(v.occluded.unwrap_or(false)) << 3)
                | (u8::from(media_loaded) << 4)
        });
        state.window.store(flags, Ordering::Relaxed);
    }
}
pub(crate) fn callback(active: bool) {
    if let Some(state) = STATE.get() {
        state.callback_ms.store(state.now(), Ordering::Relaxed);
        state
            .callback_active
            .store(u8::from(active), Ordering::Relaxed);
    }
}

/// Mantém a thread de diagnóstico viva até o encerramento do processo.
pub struct Guard {
    stop: mpsc::Sender<()>,
    join: Option<thread::JoinHandle<()>>,
}
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
/// Ativação explícita antes da janela. Não registra argumentos ou dados da mídia.
pub fn start() -> io::Result<(Guard, PathBuf)> {
    let path = PathBuf::from(format!(
        ".cache/sync2gether-diagnostics-{}.log",
        std::process::id()
    ));
    create_dir_all(".cache")?;
    let mut file = File::create(&path)?;
    writeln!(
        file,
        "sync2gether={} pid={} os={} arch={} wayland_env={} x11_env={} sample_interval_ms=1000",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        std::env::var_os("DISPLAY").is_some(),
    )?;
    let state = Arc::new(State::new());
    STATE
        .set(state.clone())
        .map_err(|_| io::Error::other("Diagnóstico já iniciado"))?;
    let (stop, rx) = mpsc::channel();
    let join = thread::Builder::new()
        .name("diagnostics".into())
        .spawn(move || {
            // Limite de uma hora para evitar arquivos sem limite de tamanho.
            for _ in 0..3600 {
                if state.sample(&mut file).and_then(|()| file.flush()).is_err() {
                    eprintln!("Não foi possível continuar gravando o diagnóstico");
                    return;
                }
                if rx.recv_timeout(Duration::from_secs(1)) != Err(mpsc::RecvTimeoutError::Timeout) {
                    let _ = state.sample(&mut file);
                    return;
                }
            }
            let _ = writeln!(file, "diagnostics-limit-reached");
        })?;
    Ok((
        Guard {
            stop,
            join: Some(join),
        },
        path,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_identifies_checkpoint_and_window_without_session_data() {
        let state = State::new();
        state.stage.store(Stage::GlRender as u8, Ordering::Relaxed);
        state.window.store(1 | 4, Ordering::Relaxed);
        state.callback_active.store(1, Ordering::Relaxed);
        let mut output = Vec::new();
        state.sample(&mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("stage=mpv-gl-render"));
        assert!(text.contains("window_known=true focused=false minimized=true"));
        assert!(text.contains("media_loaded=false mpv_callback_active=true"));
        assert_eq!(text.split_whitespace().count(), 10);
    }
}
