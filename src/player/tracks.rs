//! Leitura da lista de faixas via MPV_FORMAT_NODE, com cópia dos textos públicos.
use super::Api;
use std::ffi::{CStr, c_char, c_int, c_void};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Subtitle,
}
#[derive(Clone, Debug)]
pub struct Track {
    pub id: i64,
    pub kind: TrackKind,
    pub title: Option<String>,
    pub language: Option<String>,
    pub selected: bool,
}
impl Track {
    pub fn label(&self) -> String {
        match (&self.title, &self.language) {
            (Some(title), Some(language)) => format!("{title} · {language}"),
            (Some(title), None) => title.clone(),
            (None, Some(language)) => language.clone(),
            (None, None) => format!("Faixa {}", self.id),
        }
    }
}
#[repr(C)]
union Data {
    string: *mut c_char,
    flag: c_int,
    int64: i64,
    double: f64,
    list: *mut List,
}
#[repr(C)]
pub(super) struct Node {
    data: Data,
    format: c_int,
}
#[repr(C)]
struct List {
    count: c_int,
    values: *mut Node,
    keys: *mut *mut c_char,
}
impl Node {
    unsafe fn text(&self) -> Option<String> {
        if self.format != 1 {
            return None;
        }
        // SAFETY: MPV_FORMAT_STRING contém uma string C válida fornecida pelo mpv.
        unsafe {
            Some(
                CStr::from_ptr(self.data.string)
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }
    unsafe fn field(&self, key: &CStr) -> Option<&Node> {
        if self.format != 8 {
            return None;
        }
        // SAFETY: MPV_FORMAT_NODE_MAP tem count elementos válidos até free_node.
        unsafe {
            let list = &*self.data.list;
            (0..list.count as usize).find_map(|i| {
                (CStr::from_ptr(*list.keys.add(i)) == key).then(|| &*list.values.add(i))
            })
        }
    }
}
pub(super) fn read(api: &Api, handle: *mut c_void) -> Vec<Track> {
    let mut node = Node {
        data: Data { int64: 0 },
        format: 0,
    };
    // SAFETY: saída NODE é inicializada por mpv e liberada depois de copiar os dados.
    unsafe {
        if (api.get)(
            handle,
            c"track-list".as_ptr(),
            6,
            (&mut node as *mut Node).cast(),
        ) < 0
        {
            return Vec::new();
        }
        let tracks = parse(&node);
        (api.free_node)(&mut node);
        tracks
    }
}
unsafe fn parse(node: &Node) -> Vec<Track> {
    if node.format != 7 {
        return Vec::new();
    }
    // SAFETY: MPV_FORMAT_NODE_ARRAY e mapas são válidos até free_node_contents.
    unsafe {
        let list = &*node.data.list;
        (0..list.count as usize)
            .filter_map(|i| {
                let item = &*list.values.add(i);
                let kind = match item.field(c"type")?.text()?.as_str() {
                    "audio" => TrackKind::Audio,
                    "sub" => TrackKind::Subtitle,
                    _ => return None,
                };
                let id = item.field(c"id")?;
                if id.format != 4 {
                    return None;
                }
                Some(Track {
                    id: id.data.int64,
                    kind,
                    title: item.field(c"title").and_then(|n| n.text()),
                    language: item.field(c"lang").and_then(|n| n.text()),
                    selected: item
                        .field(c"selected")
                        .is_some_and(|n| n.format == 3 && n.data.flag != 0),
                })
            })
            .collect()
    }
}
