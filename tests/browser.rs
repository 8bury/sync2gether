use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Child, Command as Process, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use sync2gether::{
    network::relay::DEFAULT_RELAY,
    protocol::Control,
    runtime::{Command, Role, Runtime, View},
    stream::StreamPhase,
};
use sync2gether_relay::{Limits, Relay, ws};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message as Frame};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "css" => "text/css",
        "wasm" => "application/wasm",
        "ttf" => "font/ttf",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

async fn serve_http(listener: TcpListener, site: PathBuf, config: String) {
    let site = Arc::new(site);
    let config = Arc::new(config);
    loop {
        let Ok((mut stream, _)) = listener.accept().await else {
            continue;
        };
        let (site, config) = (site.clone(), config.clone());
        tokio::spawn(async move {
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buffer).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => request.extend_from_slice(&buffer[..n]),
                }
            }
            let text = String::from_utf8_lossy(&request);
            let path = text
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .split(['?', '#'])
                .next()
                .unwrap_or("/")
                .to_owned();
            let (status, kind, body) = if path == "/config.js" {
                ("200 OK", "text/javascript", config.as_bytes().to_vec())
            } else {
                let relative = if path == "/" {
                    "index.html"
                } else {
                    &path[1..]
                };
                let file = site.join(relative);
                match (relative.contains(".."), std::fs::read(&file)) {
                    (false, Ok(body)) => ("200 OK", mime(&file), body),
                    _ => ("404 Not Found", "text/plain", b"404".to_vec()),
                }
            };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(&body).await;
            let _ = stream.shutdown().await;
        });
    }
}

enum Protocol {
    Bidi { context: String },
    Cdp,
}

struct Browser {
    child: Child,
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    protocol: Protocol,
    next: u64,
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn profile(name: &str) -> PathBuf {
    let profile = root()
        .join(".cache")
        .join(format!("browser-profile-{name}"));
    let _ = std::fs::remove_dir_all(&profile);
    std::fs::create_dir_all(&profile).unwrap();
    profile
}

fn chrome_binary() -> Option<String> {
    if let Ok(binary) = std::env::var("SYNC2GETHER_TEST_CHROME")
        && !binary.is_empty()
    {
        return Some(binary);
    }
    [
        "google-chrome-stable",
        "google-chrome",
        "chromium",
        "chromium-browser",
    ]
    .into_iter()
    .find(|name| {
        Process::new("which")
            .arg(name)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
    .map(str::to_owned)
}

async fn http_get(port: u16, path: &str) -> Option<String> {
    let fetch = async {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.ok()?;
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.ok()?;
        let mut response = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            if let Some(split) = response.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&response[..split]).to_ascii_lowercase();
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())?;
                if response.len() >= split + 4 + length {
                    let body = &response[split + 4..split + 4 + length];
                    return Some(String::from_utf8_lossy(body).into_owned());
                }
            }
            let n = stream.read(&mut buffer).await.ok()?;
            if n == 0 {
                return None;
            }
            response.extend_from_slice(&buffer[..n]);
        }
    };
    tokio::time::timeout(Duration::from_secs(3), fetch)
        .await
        .ok()
        .flatten()
}

impl Browser {
    async fn firefox(name: &str) -> Self {
        let profile = profile(&format!("firefox-{name}"));
        std::fs::write(
            profile.join("user.js"),
            [
                "user_pref(\"media.autoplay.default\", 0);",
                "user_pref(\"media.autoplay.blocking_policy\", 0);",
                "user_pref(\"browser.shell.checkDefaultBrowser\", false);",
                "user_pref(\"datareporting.policy.dataSubmissionEnabled\", false);",
                "user_pref(\"toolkit.telemetry.reportingpolicy.firstRun\", false);",
                "user_pref(\"browser.aboutwelcome.enabled\", false);",
                "user_pref(\"app.update.disabledForTesting\", true);",
                "user_pref(\"media.volume_scale\", \"0.0\");",
            ]
            .join("\n"),
        )
        .unwrap();
        let port = free_port();
        let child = Process::new("firefox")
            .args(["--headless", "--no-remote", "--profile"])
            .arg(&profile)
            .args(["--remote-debugging-port", &port.to_string(), "about:blank"])
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env("MOZ_HEADLESS", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("firefox não encontrado");
        let deadline = Instant::now() + Duration::from_secs(30);
        let socket = loop {
            match tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/session")).await {
                Ok((socket, _)) => break socket,
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(200)).await
                }
                Err(error) => panic!("BiDi indisponível: {error}"),
            }
        };
        let mut browser = Self {
            child,
            socket,
            protocol: Protocol::Bidi {
                context: String::new(),
            },
            next: 0,
        };
        browser
            .call("session.new", json!({ "capabilities": {} }))
            .await;
        let tree = browser.call("browsingContext.getTree", json!({})).await;
        browser.protocol = Protocol::Bidi {
            context: tree["contexts"][0]["context"].as_str().unwrap().to_owned(),
        };
        browser
    }

    async fn chrome(binary: &str, name: &str) -> Self {
        let profile = profile(&format!("chrome-{name}"));
        let port = free_port();
        let child = Process::new(binary)
            .arg("--headless=new")
            .arg(format!("--remote-debugging-port={port}"))
            .arg(format!("--user-data-dir={}", profile.display()))
            .args([
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-sync",
                "--autoplay-policy=no-user-gesture-required",
                "--mute-audio",
                "about:blank",
            ])
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Chrome não encontrado");
        let deadline = Instant::now() + Duration::from_secs(30);
        let url = loop {
            let page = http_get(port, "/json/list").await.and_then(|body| {
                let targets: Value = serde_json::from_str(&body).ok()?;
                targets
                    .as_array()?
                    .iter()
                    .find(|target| target["type"] == json!("page"))
                    .and_then(|target| target["webSocketDebuggerUrl"].as_str())
                    .map(str::to_owned)
            });
            match page {
                Some(url) => break url,
                None if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(200)).await
                }
                None => panic!("DevTools indisponível"),
            }
        };
        let (socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        Self {
            child,
            socket,
            protocol: Protocol::Cdp,
            next: 0,
        }
    }

    async fn call(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        let message = json!({ "id": id, "method": method, "params": params });
        self.socket
            .send(Frame::Text(message.to_string().into()))
            .await
            .unwrap();
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(30), self.socket.next())
                .await
                .expect("navegador sem resposta")
                .unwrap()
                .unwrap();
            let Frame::Text(text) = frame else { continue };
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["id"] == json!(id) {
                assert!(
                    value["type"] != json!("error") && value.get("error").is_none(),
                    "{method}: {value}"
                );
                return value["result"].clone();
            }
        }
    }

    async fn navigate(&mut self, url: &str) {
        match &self.protocol {
            Protocol::Bidi { context } => {
                let context = context.clone();
                self.call(
                    "browsingContext.navigate",
                    json!({ "context": context, "url": url, "wait": "complete" }),
                )
                .await;
            }
            Protocol::Cdp => {
                self.call("Page.navigate", json!({ "url": url })).await;
                self.until(
                    "carregar a página",
                    "document.readyState === 'complete' && location.href !== 'about:blank'",
                )
                .await;
            }
        }
    }

    async fn eval(&mut self, expression: &str) -> Value {
        match &self.protocol {
            Protocol::Bidi { context } => {
                let context = context.clone();
                let result = self
                    .call(
                        "script.evaluate",
                        json!({
                            "expression": expression,
                            "target": { "context": context },
                            "awaitPromise": true,
                        }),
                    )
                    .await;
                assert_eq!(result["type"], json!("success"), "{expression}: {result}");
                result["result"]["value"].clone()
            }
            Protocol::Cdp => {
                let result = self
                    .call(
                        "Runtime.evaluate",
                        json!({
                            "expression": expression,
                            "awaitPromise": true,
                            "returnByValue": true,
                        }),
                    )
                    .await;
                assert!(
                    result.get("exceptionDetails").is_none(),
                    "{expression}: {result}"
                );
                result["result"]["value"].clone()
            }
        }
    }

    async fn choose_file(&mut self, id: &str, path: &Path) {
        let path = path.to_str().unwrap().to_owned();
        match &self.protocol {
            Protocol::Bidi { context } => {
                let context = context.clone();
                let found = self
                    .call(
                        "script.evaluate",
                        json!({
                            "expression": format!("document.getElementById('{id}')"),
                            "target": { "context": context },
                            "awaitPromise": false,
                        }),
                    )
                    .await;
                let element = found["result"]["sharedId"].as_str().unwrap().to_owned();
                self.call(
                    "input.setFiles",
                    json!({
                        "context": context,
                        "element": { "sharedId": element },
                        "files": [path],
                    }),
                )
                .await;
            }
            Protocol::Cdp => {
                let document = self.call("DOM.getDocument", json!({})).await;
                let root = document["root"]["nodeId"].clone();
                let found = self
                    .call(
                        "DOM.querySelector",
                        json!({ "nodeId": root, "selector": format!("#{id}") }),
                    )
                    .await;
                self.call(
                    "DOM.setFileInputFiles",
                    json!({ "files": [path], "nodeId": found["nodeId"] }),
                )
                .await;
            }
        }
    }

    async fn text(&mut self, id: &str) -> String {
        self.eval(&format!("document.getElementById('{id}').textContent"))
            .await
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    async fn until(&mut self, what: &str, expression: &str) {
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            if self.eval(expression).await == json!(true) {
                return;
            }
            if Instant::now() > deadline {
                let status = self.text("status").await;
                let copy = self.text("copy-status").await;
                let notice = self.text("notice").await;
                panic!("navegador: {what}; status={status:?} cópia={copy:?} aviso={notice:?}");
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

fn latest(runtime: &Runtime, view: &mut View) {
    while let Ok(next) = runtime.views.try_recv() {
        *view = next;
    }
}

async fn host_until(runtime: &Runtime, view: &mut View, what: &str, done: impl Fn(&View) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        latest(runtime, view);
        if done(view) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "anfitrião: {what}; status={} erro={:?} aviso={:?}",
            view.status,
            view.error,
            view.notice
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

struct Setup {
    host: Runtime,
    view: View,
    url: String,
    fixture: PathBuf,
}

fn fixture() -> PathBuf {
    let fixture = root().join(".cache/browser-fixture.mp4");
    assert!(fixture.is_file(), "execute scripts/test-browser.sh");
    fixture
}

fn mkv_fixture() -> PathBuf {
    let fixture = root().join(".cache/browser-fixture.mkv");
    assert!(fixture.is_file(), "execute scripts/test-browser.sh");
    fixture
}

fn mkv_aac_fixture() -> PathBuf {
    let fixture = root().join(".cache/browser-fixture-aac.mkv");
    assert!(fixture.is_file(), "execute scripts/test-browser.sh");
    fixture
}

async fn setup() -> Setup {
    setup_with_fixture(fixture()).await
}

async fn setup_with_fixture(fixture: PathBuf) -> Setup {
    if let Ok(public) = std::env::var("SYNC2GETHER_TEST_PUBLIC_URL")
        && !public.is_empty()
    {
        return host_room(
            public.trim_end_matches('/').to_owned(),
            DEFAULT_RELAY.into(),
            fixture,
        )
        .await;
    }
    let site = root().join("target/web");
    assert!(site.join("pkg").is_dir(), "execute scripts/build-web.sh");
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let web = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay_address: SocketAddr = tcp.local_addr().unwrap();
    let ws_address = web.local_addr().unwrap();
    let origin = format!("http://127.0.0.1:{}", http.local_addr().unwrap().port());
    let relay = Relay::new(Limits::default(), None);
    let browsers = relay.clone();
    tokio::spawn(async move { relay.serve(tcp).await });
    let origins = vec![origin.clone()];
    tokio::spawn(async move {
        browsers
            .serve_ws(
                web,
                ws::Config {
                    origins,
                    client_ip_header: None,
                },
            )
            .await
    });
    let config = format!(
        "export const RELAY_URL = \"ws://{ws_address}{}\";\n",
        ws::PATH
    );
    tokio::spawn(serve_http(http, site, config));
    host_room(origin, relay_address.to_string(), fixture).await
}

async fn host_room(origin: String, relay: String, fixture: PathBuf) -> Setup {
    let host = Runtime::start();
    let mut view = View::default();
    host.commands
        .send(Command::OpenPath(fixture.clone()))
        .await
        .unwrap();
    host_until(&host, &mut view, "abrir o filme", |v| {
        v.player.loaded && !v.loading
    })
    .await;
    host.commands
        .send(Command::HostInternet(relay))
        .await
        .unwrap();
    host_until(&host, &mut view, "código da sala", |v| {
        v.room_code.is_some()
    })
    .await;
    let code = view.room_code.clone().unwrap();
    Setup {
        host,
        view,
        url: format!("{origin}/#{code}"),
        fixture,
    }
}

async fn play_in_sync(firefox: &mut Browser, host: &Runtime, view: &mut View) {
    firefox
        .until(
            "pronto para reproduzir",
            "!document.getElementById('toggle').disabled && document.getElementById('status').textContent.startsWith('Pronto')",
        )
        .await;
    firefox
        .eval("document.getElementById('video').muted = true; document.getElementById('toggle').click(); true")
        .await;
    host_until(host, view, "reproduzir", |v| {
        !v.player.paused && v.player.position > 1.0
    })
    .await;
    firefox
        .until(
            "vídeo tocando no navegador",
            "(() => { const v = document.getElementById('video'); return !v.paused && v.currentTime > 1; })()",
        )
        .await;
    tokio::time::sleep(Duration::from_secs(4)).await;
    let mut drifts = Vec::new();
    for _ in 0..10 {
        latest(host, view);
        if let Some(drift) = view.peer_drift {
            drifts.push(drift.abs());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let status = firefox.text("status").await;
    println!("diferenças vistas pelo anfitrião: {drifts:?}; navegador: {status}");
    assert!(!drifts.is_empty(), "anfitrião não mediu a diferença");
    let worst = drifts.iter().copied().fold(0.0, f64::max);
    assert!(worst < 0.25, "navegador fora de sincronia: {worst:.3}s");
    if !status.starts_with("Sincronizado") {
        let diag = firefox
            .eval("(() => { const v = document.getElementById('video'); const b = []; for (let i = 0; i < v.buffered.length; i++) b.push([v.buffered.start(i), v.buffered.end(i)]); return JSON.stringify({ t: v.currentTime, paused: v.paused, rs: v.readyState, buffered: b }); })()")
            .await;
        panic!("{status} diag={diag}");
    }

    host.commands
        .send(Command::Control(Control::Pause))
        .await
        .unwrap();
    firefox
        .until(
            "pausar no navegador",
            "document.getElementById('video').paused",
        )
        .await;
}

async fn own_copy(mut firefox: Browser) {
    let Setup {
        host,
        mut view,
        url,
        fixture,
    } = setup().await;
    firefox.navigate(&url).await;
    firefox
        .until("entrar na sala", "!document.getElementById('room').hidden")
        .await;
    assert_eq!(
        firefox.eval("location.hash").await,
        json!(""),
        "o código deve sair da barra de endereço"
    );
    host_until(&host, &mut view, "conectar", |v| {
        v.connected && v.role == Role::Host
    })
    .await;
    firefox.choose_file("pick", &fixture).await;
    host_until(&host, &mut view, "cópias conferidas", |v| {
        v.matched && v.peer_ready
    })
    .await;

    let srt_path = root().join(".cache/browser-fixture.srt");
    std::fs::write(
        &srt_path,
        "1\r\n00:00:00,000 --> 00:00:30,000\r\nLegenda compartilhada do anfitrião\r\n",
    )
    .unwrap();
    host.commands
        .send(Command::ShareSubtitlePath(srt_path))
        .await
        .unwrap();
    host_until(&host, &mut view, "legenda compartilhada", |v| {
        v.shared_subtitle.as_deref() == Some("browser-fixture.srt")
    })
    .await;

    play_in_sync(&mut firefox, &host, &mut view).await;
    firefox
        .until(
            "cue da legenda compartilhada visível",
            "(() => { const v = document.getElementById('video'); const t = v.textTracks && v.textTracks[0]; return Boolean(t && t.mode === 'showing' && t.activeCues && t.activeCues.length > 0 && t.activeCues[0].text.includes('Legenda compartilhada')); })()",
        )
        .await;
    firefox
        .eval("document.getElementById('subtitles-btn').click(); document.getElementById('sub-delay-p500').click(); true")
        .await;
    assert_eq!(firefox.text("sub-delay-value").await, "+0.5 s");
    firefox
        .eval("document.getElementById('sub-opt-off').click(); true")
        .await;
    firefox
        .until(
            "legenda desativada",
            "document.getElementById('video').textTracks.length === 0",
        )
        .await;

    firefox
        .eval("document.getElementById('leave').click(); true")
        .await;
    host_until(&host, &mut view, "saída do navegador", |v| !v.connected).await;
}

async fn streaming(mut firefox: Browser) {
    let Setup {
        host,
        mut view,
        url,
        ..
    } = setup().await;
    firefox.navigate(&url).await;
    firefox
        .until(
            "botão de streaming",
            "!document.getElementById('room').hidden && !document.getElementById('stream').hidden",
        )
        .await;
    host_until(&host, &mut view, "convidado do navegador", |v| {
        v.connected && v.peer_name.is_some()
    })
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    latest(&host, &mut view);
    assert!(
        !view.can_stream,
        "o botão de streaming QUIC deve sumir para convidados no navegador"
    );
    firefox
        .eval("document.getElementById('stream').click(); true")
        .await;
    host_until(&host, &mut view, "streaming ativo", |v| {
        v.stream.is_sender && v.stream.phase == StreamPhase::Streaming
    })
    .await;
    host_until(&host, &mut view, "cópia por streaming", |v| {
        v.matched && v.peer_ready
    })
    .await;
    play_in_sync(&mut firefox, &host, &mut view).await;
    latest(&host, &mut view);
    assert!(view.stream.chunks_cached > 0, "nenhum bloco enviado");
    firefox
        .eval("(() => { const v = document.getElementById('video'); return v.duration > 15; })()")
        .await;
}

async fn streaming_mkv(mut browser: Browser) {
    let Setup {
        host,
        mut view,
        url,
        ..
    } = setup_with_fixture(mkv_fixture()).await;
    browser.navigate(&url).await;
    browser
        .until(
            "botão de streaming MKV",
            "!document.getElementById('room').hidden && !document.getElementById('stream').hidden",
        )
        .await;
    host_until(&host, &mut view, "convidado do navegador MKV", |v| {
        v.connected && v.peer_name.is_some()
    })
    .await;
    let warn = browser.text("format-warning").await;
    assert!(
        warn.contains("streaming"),
        "aviso de formato MKV deveria indicar suporte por streaming: {warn:?}"
    );
    browser
        .eval("document.getElementById('stream').click(); true")
        .await;
    host_until(&host, &mut view, "streaming MKV ativo", |v| {
        v.stream.is_sender && v.stream.phase == StreamPhase::Streaming
    })
    .await;
    host_until(&host, &mut view, "cópia MKV pronta", |v| {
        v.matched && v.peer_ready
    })
    .await;
    browser
        .until(
            "menu de faixas de áudio populado",
            "document.querySelectorAll('#audio-list button').length === 2",
        )
        .await;

    play_in_sync(&mut browser, &host, &mut view).await;

    host.commands
        .send(Command::Control(Control::Seek(12.0)))
        .await
        .unwrap();
    browser
        .until(
            "seek para posição não carregada no MKV",
            "(() => { const v = document.getElementById('video'); return v.currentTime >= 11.5 && v.readyState >= 3; })()",
        )
        .await;

    browser
        .eval("document.getElementById('subtitles-btn').click(); document.querySelector('#audio-list button[data-audio-index=\"1\"]').click(); true")
        .await;
    browser
        .until(
            "segunda faixa de áudio selecionada",
            "document.querySelector('#audio-list button[data-audio-index=\"1\"]').getAttribute('aria-checked') === 'true'",
        )
        .await;

    host.commands
        .send(Command::Control(Control::Play))
        .await
        .unwrap();
    browser
        .until(
            "reprodução após troca de faixa de áudio",
            "(() => { const v = document.getElementById('video'); return !v.paused && v.currentTime >= 12.5; })()",
        )
        .await;
    host.commands
        .send(Command::Control(Control::Pause))
        .await
        .unwrap();
    browser
        .until(
            "pausar MKV no navegador",
            "document.getElementById('video').paused",
        )
        .await;
}

async fn streaming_mkv_subtitles(mut browser: Browser) {
    let Setup {
        host,
        mut view,
        url,
        ..
    } = setup_with_fixture(mkv_fixture()).await;
    browser.navigate(&url).await;
    browser
        .until(
            "botão de streaming MKV com legendas",
            "!document.getElementById('room').hidden && !document.getElementById('stream').hidden",
        )
        .await;
    host_until(&host, &mut view, "convidado do navegador MKV", |v| {
        v.connected && v.peer_name.is_some()
    })
    .await;
    browser
        .eval("document.getElementById('stream').click(); true")
        .await;
    host_until(&host, &mut view, "cópia MKV pronta", |v| {
        v.matched && v.peer_ready
    })
    .await;
    browser
        .until(
            "legendas embutidas listadas no menu",
            "document.querySelectorAll('#embedded-subtitle-list button').length === 2",
        )
        .await;

    host.commands
        .send(Command::Control(Control::Seek(2.0)))
        .await
        .unwrap();
    browser
        .until(
            "posição inicial da legenda embutida",
            "(() => { const v = document.getElementById('video'); return v.currentTime >= 1.5 && v.currentTime <= 4.0 && v.readyState >= 3; })()",
        )
        .await;

    browser
        .eval("document.getElementById('subtitles-btn').click(); document.querySelector('#embedded-subtitle-list button[data-embedded-index=\"0\"]').click(); true")
        .await;
    browser
        .until(
            "cue da legenda SRT embutida visível",
            "(() => { const v = document.getElementById('video'); const t = v.textTracks && v.textTracks[0]; return Boolean(t && t.mode === 'showing' && t.activeCues && t.activeCues.length > 0 && t.activeCues[0].text.includes('Legenda embutida SRT inicio')); })()",
        )
        .await;

    browser
        .eval("document.querySelector('#embedded-subtitle-list button[data-embedded-index=\"1\"]').click(); true")
        .await;
    browser
        .until(
            "cue da legenda ASS embutida visível sem tags",
            "(() => { const v = document.getElementById('video'); const t = v.textTracks && v.textTracks[0]; return Boolean(t && t.mode === 'showing' && t.activeCues && t.activeCues.length > 0 && t.activeCues[0].text.includes('Embedded ASS start') && !t.activeCues[0].text.includes('{\\\\')); })()",
        )
        .await;

    host.commands
        .send(Command::Control(Control::Seek(12.0)))
        .await
        .unwrap();
    browser
        .until(
            "segundo cue ASS no tempo certo",
            "(() => { const v = document.getElementById('video'); const t = v.textTracks && v.textTracks[0]; return Boolean(v.currentTime >= 11.5 && t && t.activeCues && t.activeCues.length > 0 && t.activeCues[0].text.includes('Embedded ASS middle')); })()",
        )
        .await;

    browser
        .eval("document.querySelector('#embedded-subtitle-list button[data-embedded-index=\"0\"]').click(); true")
        .await;
    browser
        .until(
            "volta para legenda SRT em cache no segundo trecho",
            "(() => { const v = document.getElementById('video'); const t = v.textTracks && v.textTracks[0]; return Boolean(t && t.activeCues && t.activeCues.length > 0 && t.activeCues[0].text.includes('Legenda embutida SRT meio')); })()",
        )
        .await;

    browser
        .eval("document.getElementById('sub-opt-off').click(); true")
        .await;
    browser
        .until(
            "legenda embutida desativada",
            "document.getElementById('video').textTracks.length === 0",
        )
        .await;
}

async fn setup_web_only() -> (String, SocketAddr) {
    let site = root().join("target/web");
    assert!(site.join("pkg").is_dir(), "execute scripts/build-web.sh");
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let web = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay_address: SocketAddr = tcp.local_addr().unwrap();
    let ws_address = web.local_addr().unwrap();
    let origin = format!("http://127.0.0.1:{}", http.local_addr().unwrap().port());
    let relay = Relay::new(Limits::default(), None);
    let browsers = relay.clone();
    tokio::spawn(async move { relay.serve(tcp).await });
    let origins = vec![origin.clone()];
    tokio::spawn(async move {
        browsers
            .serve_ws(
                web,
                ws::Config {
                    origins,
                    client_ip_header: None,
                },
            )
            .await
    });
    let config = format!(
        "export const RELAY_URL = \"ws://{ws_address}{}\";\n",
        ws::PATH
    );
    tokio::spawn(serve_http(http, site, config));
    (origin, relay_address)
}

async fn host_with_desktop_guest(mut browser: Browser) {
    let fixture = fixture();
    let (origin, relay_address) = setup_web_only().await;
    browser.navigate(&origin).await;
    browser
        .until(
            "botão criar sala",
            "!document.getElementById('entry').hidden && !document.getElementById('create-room').hidden",
        )
        .await;
    browser
        .eval("document.getElementById('create-room').click(); true")
        .await;
    browser
        .until(
            "código gerado",
            "!document.getElementById('room').hidden && /^\\d{4,8}-[a-z]+-[a-z]+$/.test(document.getElementById('host-invite-code').textContent.trim())",
        )
        .await;
    let code = browser.text("host-invite-code").await.trim().to_owned();
    browser.choose_file("pick", &fixture).await;

    let guest = Runtime::start();
    let mut view = View::default();
    guest
        .commands
        .send(Command::OpenPath(fixture.clone()))
        .await
        .unwrap();
    host_until(&guest, &mut view, "convidado abrir filme", |v| {
        v.player.loaded && !v.loading
    })
    .await;
    guest
        .commands
        .send(Command::JoinCode {
            relay: relay_address.to_string(),
            code,
        })
        .await
        .unwrap();
    host_until(
        &guest,
        &mut view,
        "convidado desktop conectar e conferir",
        |v| v.connected && v.role == Role::Guest && v.matched && v.ready,
    )
    .await;

    browser
        .until(
            "anfitrião pronto para começar",
            "!document.getElementById('start-watching').disabled",
        )
        .await;
    browser
        .eval("document.getElementById('video').muted = true; document.getElementById('start-watching').click(); true")
        .await;
    host_until(&guest, &mut view, "convidado desktop reproduzir", |v| {
        !v.player.paused && v.player.position > 1.0
    })
    .await;
    browser
        .until(
            "vídeo tocando no navegador anfitrião",
            "(() => { const v = document.getElementById('video'); return !v.paused && v.currentTime > 1; })()",
        )
        .await;

    tokio::time::sleep(Duration::from_secs(4)).await;
    let mut drifts = Vec::new();
    for _ in 0..10 {
        latest(&guest, &mut view);
        if let Some(drift) = view.peer_drift {
            drifts.push(drift.abs());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let status = browser.text("status").await;
    println!(
        "anfitrião navegador; diferenças vistas pelo convidado desktop: {drifts:?}; navegador: {status}"
    );
    assert!(!drifts.is_empty(), "convidado não mediu a diferença");
    let worst = drifts.iter().copied().fold(0.0, f64::max);
    assert!(worst < 0.25, "fora de sincronia: {worst:.3}s");

    browser
        .eval("document.getElementById('toggle').click(); true")
        .await;
    host_until(&guest, &mut view, "convidado pausar", |v| v.player.paused).await;
    browser
        .until(
            "anfitrião pausar",
            "document.getElementById('video').paused",
        )
        .await;
}

async fn browser_host_and_browser_guest(mut host: Browser, mut guest: Browser) {
    let fixture = fixture();
    let (origin, _relay_address) = setup_web_only().await;
    host.navigate(&origin).await;
    host.until(
        "botão criar sala",
        "!document.getElementById('entry').hidden",
    )
    .await;
    host.eval("document.getElementById('create-room').click(); true")
        .await;
    host.until(
        "código gerado",
        "!document.getElementById('room').hidden && /^\\d{4,8}-[a-z]+-[a-z]+$/.test(document.getElementById('host-invite-code').textContent.trim())",
    )
    .await;
    let code = host.text("host-invite-code").await.trim().to_owned();
    host.choose_file("pick", &fixture).await;

    guest.navigate(&format!("{origin}/#{code}")).await;
    guest
        .until(
            "convidado entrar na sala e ver botão de streaming",
            "!document.getElementById('room').hidden && !document.getElementById('stream').hidden",
        )
        .await;
    guest.choose_file("pick", &fixture).await;

    host.until(
        "anfitrião pronto",
        "!document.getElementById('start-watching').disabled",
    )
    .await;
    guest
        .eval("document.getElementById('video').muted = true; true")
        .await;
    host.eval("document.getElementById('video').muted = true; document.getElementById('start-watching').click(); true")
        .await;

    host.until(
        "anfitrião tocando",
        "(() => { const v = document.getElementById('video'); return !v.paused && v.currentTime > 1; })()",
    )
    .await;
    guest
        .until(
            "convidado tocando",
            "(() => { const v = document.getElementById('video'); return !v.paused && v.currentTime > 1; })()",
        )
        .await;

    tokio::time::sleep(Duration::from_secs(4)).await;
    let host_t = host
        .eval("document.getElementById('video').currentTime")
        .await
        .as_f64()
        .unwrap();
    let guest_t = guest
        .eval("document.getElementById('video').currentTime")
        .await
        .as_f64()
        .unwrap();
    let diff = (host_t - guest_t).abs();
    let host_status = host.text("status").await;
    let guest_status = guest.text("status").await;
    println!("navegador+navegador: diff={diff:.3}s host={host_status:?} guest={guest_status:?}");
    assert!(
        diff < 0.25,
        "navegador+navegador fora de sincronia: {diff:.3}s"
    );

    host.eval("document.getElementById('toggle').click(); true")
        .await;
    host.until(
        "anfitrião pausou",
        "document.getElementById('video').paused",
    )
    .await;
    guest
        .until(
            "convidado pausou",
            "document.getElementById('video').paused",
        )
        .await;
}

async fn browser_host_streams_to_browser_guest(
    mut host: Browser,
    mut guest: Browser,
    fixture: PathBuf,
    expect_mse_audio_tracks: bool,
) {
    let (origin, _relay_address) = setup_web_only().await;
    host.navigate(&origin).await;
    host.until(
        "botão criar sala",
        "!document.getElementById('entry').hidden",
    )
    .await;
    host.eval("document.getElementById('create-room').click(); true")
        .await;
    host.until(
        "código gerado",
        "!document.getElementById('room').hidden && /^\\d{4,8}-[a-z]+-[a-z]+$/.test(document.getElementById('host-invite-code').textContent.trim())",
    )
    .await;
    let code = host.text("host-invite-code").await.trim().to_owned();
    host.choose_file("pick", &fixture).await;

    guest.navigate(&format!("{origin}/#{code}")).await;
    guest
        .until(
            "convidado ver botão de streaming do anfitrião navegador",
            "!document.getElementById('room').hidden && !document.getElementById('stream').hidden",
        )
        .await;
    guest
        .eval("document.getElementById('stream').click(); true")
        .await;

    host.until(
        "anfitrião exibir estado de streaming e ficar pronto",
        "!document.getElementById('host-stream-strip').hidden && !document.getElementById('start-watching').disabled",
    )
    .await;
    if expect_mse_audio_tracks {
        guest
            .until(
                "convidado listar faixas de áudio do MKV",
                "document.querySelectorAll('#audio-list button').length === 2",
            )
            .await;
    }

    guest
        .eval("document.getElementById('video').muted = true; true")
        .await;
    host.eval("document.getElementById('video').muted = true; document.getElementById('start-watching').click(); true")
        .await;

    host.until(
        "anfitrião tocando",
        "(() => { const v = document.getElementById('video'); return !v.paused && v.currentTime > 1; })()",
    )
    .await;
    guest
        .until(
            "convidado tocando por streaming",
            "(() => { const v = document.getElementById('video'); return !v.paused && v.currentTime > 1; })()",
        )
        .await;

    tokio::time::sleep(Duration::from_secs(4)).await;
    let host_t = host
        .eval("document.getElementById('video').currentTime")
        .await
        .as_f64()
        .unwrap();
    let guest_t = guest
        .eval("document.getElementById('video').currentTime")
        .await
        .as_f64()
        .unwrap();
    let diff = (host_t - guest_t).abs();
    let host_status = host.text("status").await;
    let guest_status = guest.text("status").await;
    let kind = if expect_mse_audio_tracks {
        "MKV"
    } else {
        "MP4"
    };
    println!(
        "streaming navegador->navegador ({kind}): diff={diff:.3}s host={host_status:?} guest={guest_status:?}"
    );
    assert!(
        diff < 0.25,
        "streaming navegador->navegador ({kind}) fora de sincronia: {diff:.3}s"
    );

    host.eval("document.getElementById('forward').click(); true")
        .await;
    host.until(
        "anfitrião após seek",
        "(() => { const v = document.getElementById('video'); return !v.paused && v.currentTime >= 11.0; })()",
    )
    .await;
    guest
        .until(
            "convidado após seek por streaming",
            "(() => { const v = document.getElementById('video'); return !v.paused && v.currentTime >= 11.0; })()",
        )
        .await;

    if expect_mse_audio_tracks {
        guest
            .eval("document.getElementById('subtitles-btn').click(); document.querySelector('#audio-list button[data-audio-index=\"1\"]').click(); true")
            .await;
        guest
            .until(
                "segunda faixa de áudio selecionada no MKV",
                "document.querySelector('#audio-list button[data-audio-index=\"1\"]').getAttribute('aria-checked') === 'true'",
            )
            .await;
    }

    host.eval("document.getElementById('toggle').click(); true")
        .await;
    host.until(
        "anfitrião pausou",
        "document.getElementById('video').paused",
    )
    .await;
    guest
        .until(
            "convidado pausou",
            "document.getElementById('video').paused",
        )
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Firefox e scripts/test-browser.sh"]
async fn firefox_host_streams_mp4_to_firefox_guest() {
    let host = Browser::firefox("stream-web-host-mp4").await;
    let guest = Browser::firefox("stream-web-guest-mp4").await;
    browser_host_streams_to_browser_guest(host, guest, fixture(), false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Chrome ou Chromium e scripts/test-browser.sh"]
async fn chrome_host_streams_mp4_to_chrome_guest() {
    let Some(binary) = chrome_binary() else {
        eprintln!("Chrome/Chromium não encontrado; defina SYNC2GETHER_TEST_CHROME");
        return;
    };
    let host = Browser::chrome(&binary, "stream-web-host-mp4").await;
    let guest = Browser::chrome(&binary, "stream-web-guest-mp4").await;
    browser_host_streams_to_browser_guest(host, guest, fixture(), false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Firefox e scripts/test-browser.sh"]
async fn firefox_host_streams_mkv_aac_to_firefox_guest() {
    let host = Browser::firefox("stream-web-host-mkv").await;
    let guest = Browser::firefox("stream-web-guest-mkv").await;
    browser_host_streams_to_browser_guest(host, guest, mkv_aac_fixture(), true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Chrome ou Chromium e scripts/test-browser.sh"]
async fn chrome_host_streams_mkv_aac_to_chrome_guest() {
    let Some(binary) = chrome_binary() else {
        eprintln!("Chrome/Chromium não encontrado; defina SYNC2GETHER_TEST_CHROME");
        return;
    };
    let host = Browser::chrome(&binary, "stream-web-host-mkv").await;
    let guest = Browser::chrome(&binary, "stream-web-guest-mkv").await;
    browser_host_streams_to_browser_guest(host, guest, mkv_aac_fixture(), true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Firefox, libmpv e scripts/test-browser.sh"]
async fn firefox_host_with_own_copy_plays_in_sync_with_desktop_guest() {
    host_with_desktop_guest(Browser::firefox("host-desktop-guest").await).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Chrome ou Chromium, libmpv e scripts/test-browser.sh"]
async fn chrome_host_with_own_copy_plays_in_sync_with_desktop_guest() {
    let Some(binary) = chrome_binary() else {
        eprintln!("Chrome/Chromium não encontrado; defina SYNC2GETHER_TEST_CHROME");
        return;
    };
    host_with_desktop_guest(Browser::chrome(&binary, "host-desktop-guest").await).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Firefox e scripts/test-browser.sh"]
async fn firefox_host_and_firefox_guest_play_in_sync_with_own_copy() {
    let host = Browser::firefox("host-web-host").await;
    let guest = Browser::firefox("host-web-guest").await;
    browser_host_and_browser_guest(host, guest).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Firefox, libmpv e scripts/test-browser.sh"]
async fn firefox_guest_selects_and_switches_embedded_subtitles_in_mkv() {
    streaming_mkv_subtitles(Browser::firefox("stream-mkv-subs").await).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Chrome ou Chromium, libmpv e scripts/test-browser.sh"]
async fn chrome_guest_selects_and_switches_embedded_subtitles_in_mkv() {
    let Some(binary) = chrome_binary() else {
        eprintln!("Chrome/Chromium não encontrado; defina SYNC2GETHER_TEST_CHROME");
        return;
    };
    streaming_mkv_subtitles(Browser::chrome(&binary, "stream-mkv-subs").await).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Firefox, libmpv e scripts/test-browser.sh"]
async fn firefox_guest_streams_mkv_ac3_via_mse_and_switches_audio() {
    streaming_mkv(Browser::firefox("stream-mkv").await).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Chrome ou Chromium, libmpv e scripts/test-browser.sh"]
async fn chrome_guest_streams_mkv_ac3_via_mse_and_switches_audio() {
    let Some(binary) = chrome_binary() else {
        eprintln!("Chrome/Chromium não encontrado; defina SYNC2GETHER_TEST_CHROME");
        return;
    };
    streaming_mkv(Browser::chrome(&binary, "stream-mkv").await).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Firefox, libmpv e scripts/test-browser.sh"]
async fn firefox_guest_with_own_copy_follows_the_desktop_host() {
    own_copy(Browser::firefox("copy").await).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Firefox, libmpv e scripts/test-browser.sh"]
async fn firefox_guest_streams_from_the_desktop_host_over_webrtc() {
    streaming(Browser::firefox("stream").await).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Chrome ou Chromium, libmpv e scripts/test-browser.sh"]
async fn chrome_guest_with_own_copy_follows_the_desktop_host() {
    let Some(binary) = chrome_binary() else {
        eprintln!("Chrome/Chromium não encontrado; defina SYNC2GETHER_TEST_CHROME");
        return;
    };
    own_copy(Browser::chrome(&binary, "copy").await).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requer Chrome ou Chromium, libmpv e scripts/test-browser.sh"]
async fn chrome_guest_streams_from_the_desktop_host_over_webrtc() {
    let Some(binary) = chrome_binary() else {
        eprintln!("Chrome/Chromium não encontrado; defina SYNC2GETHER_TEST_CHROME");
        return;
    };
    streaming(Browser::chrome(&binary, "stream").await).await;
}
