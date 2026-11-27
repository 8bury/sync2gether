use crate::protocol::{
    MAX_MESSAGE, MAX_SUBTITLE_BYTES, MAX_SUBTITLE_CHUNK, MAX_SUBTITLE_CHUNKS, SubtitleChunk,
    valid_file_name,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleError {
    TooLarge,
    Empty,
    InvalidFormat,
    InvalidName,
}

const CP1252_EXTRA: [char; 32] = [
    '\u{20AC}', '\u{FFFD}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{FFFD}', '\u{017D}', '\u{FFFD}',
    '\u{FFFD}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{FFFD}', '\u{017E}', '\u{0178}',
];

pub fn decode_bytes(bytes: &[u8]) -> Result<String, SubtitleError> {
    if bytes.is_empty() {
        return Err(SubtitleError::Empty);
    }
    if bytes.len() > MAX_SUBTITLE_BYTES {
        return Err(SubtitleError::TooLarge);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(text.to_owned());
    }
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        let ch = if (0x80..=0x9F).contains(&b) {
            CP1252_EXTRA[(b - 0x80) as usize]
        } else {
            char::from(b)
        };
        out.push(ch);
    }
    Ok(out)
}

pub fn to_webvtt(input: &str) -> Result<String, SubtitleError> {
    if input.len() > MAX_SUBTITLE_BYTES {
        return Err(SubtitleError::TooLarge);
    }
    let stripped = input.strip_prefix('\u{feff}').unwrap_or(input);
    let normalized = normalize_newlines(stripped);
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return Err(SubtitleError::Empty);
    }
    let vtt = if is_webvtt(trimmed) {
        normalize_webvtt(trimmed)?
    } else {
        convert_srt(trimmed)?
    };
    if vtt.len() > MAX_SUBTITLE_BYTES {
        return Err(SubtitleError::TooLarge);
    }
    Ok(vtt)
}

pub fn from_bytes(bytes: &[u8]) -> Result<String, SubtitleError> {
    let text = decode_bytes(bytes)?;
    to_webvtt(&text)
}

fn normalize_newlines(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else if c != '\0' {
            out.push(c);
        }
    }
    out
}

fn is_webvtt(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("WEBVTT") else {
        return false;
    };
    rest.is_empty() || rest.starts_with('\n') || rest.starts_with(' ') || rest.starts_with('\t')
}

fn normalize_webvtt(text: &str) -> Result<String, SubtitleError> {
    let mut has_cue = false;
    for line in text.lines() {
        if parse_timing_line(line).is_some() {
            has_cue = true;
            break;
        }
    }
    if !has_cue {
        return Err(SubtitleError::InvalidFormat);
    }
    let mut out = text.to_owned();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn convert_srt(text: &str) -> Result<String, SubtitleError> {
    let mut out = String::from("WEBVTT\n\n");
    let mut cues = 0usize;
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        while i < lines.len() && lines[i].trim().is_empty() {
            i += 1;
        }
        if i >= lines.len() {
            break;
        }
        let mut block = Vec::new();
        while i < lines.len() && !lines[i].trim().is_empty() {
            block.push(lines[i]);
            i += 1;
        }
        let timing_idx = if let Some(pos) = block
            .iter()
            .take(2)
            .position(|line| parse_timing_line(line).is_some())
        {
            pos
        } else {
            continue;
        };
        let Some((start_ms, end_ms, settings)) = parse_timing_line(block[timing_idx]) else {
            continue;
        };
        let payload = &block[timing_idx + 1..];
        if payload.is_empty() {
            continue;
        }
        if cues > 0 {
            out.push('\n');
        }
        out.push_str(&format_vtt_time(start_ms));
        out.push_str(" --> ");
        out.push_str(&format_vtt_time(end_ms));
        if !settings.is_empty() {
            out.push(' ');
            out.push_str(settings);
        }
        out.push('\n');
        for line in payload {
            let clean = sanitize_srt_line(line);
            out.push_str(&clean);
            out.push('\n');
        }
        cues += 1;
    }
    if cues == 0 {
        return Err(SubtitleError::InvalidFormat);
    }
    Ok(out)
}

fn parse_timing_line(line: &str) -> Option<(i64, i64, &str)> {
    let arrow = line.find("-->")?;
    let left = line[..arrow].trim();
    let right_full = line[arrow + 3..].trim();
    let (right, settings) = match right_full.find(|c: char| c.is_whitespace()) {
        Some(idx) => (&right_full[..idx], right_full[idx..].trim()),
        None => (right_full, ""),
    };
    let start_ms = parse_timestamp(left)?;
    let end_ms = parse_timestamp(right)?;
    if end_ms < start_ms {
        return None;
    }
    Some((start_ms, end_ms, settings))
}

fn parse_timestamp(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    let (hms, frac) = match raw.find([',', '.']) {
        Some(idx) => (&raw[..idx], &raw[idx + 1..]),
        None => (raw, "0"),
    };
    if frac.is_empty() || frac.len() > 3 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ms: i64 = match frac.len() {
        1 => frac.parse::<i64>().ok()? * 100,
        2 => frac.parse::<i64>().ok()? * 10,
        _ => frac.parse::<i64>().ok()?,
    };
    let parts: Vec<&str> = hms.split(':').collect();
    let (h, m, s): (i64, i64, i64) = match parts.as_slice() {
        [h, m, s] => (h.parse().ok()?, m.parse().ok()?, s.parse().ok()?),
        [m, s] => (0, m.parse().ok()?, s.parse().ok()?),
        _ => return None,
    };
    if h < 0 || !(0..60).contains(&m) || !(0..60).contains(&s) {
        return None;
    }
    Some(((h * 3600 + m * 60 + s) * 1000) + ms)
}

fn format_vtt_time(total_ms: i64) -> String {
    let clamped = total_ms.max(0);
    let ms = clamped % 1000;
    let total_s = clamped / 1000;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

fn sanitize_srt_line(line: &str) -> String {
    let no_ass = strip_ass_tags(line);
    let mut out = String::with_capacity(no_ass.len());
    let bytes = no_ass.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<'
            && let Some(rel_end) = no_ass[i + 1..].find('>')
        {
            let tag = &no_ass[i + 1..i + 1 + rel_end];
            let lower = tag.trim().to_ascii_lowercase();
            if matches!(lower.as_str(), "i" | "/i" | "b" | "/b" | "u" | "/u") {
                out.push('<');
                out.push_str(&lower);
                out.push('>');
                i += rel_end + 2;
                continue;
            }
            if lower.starts_with("font")
                || lower == "/font"
                || lower == "br"
                || lower == "br/"
                || lower == "br /"
            {
                if lower.starts_with("br") {
                    out.push(' ');
                }
                i += rel_end + 2;
                continue;
            }
        }
        let ch = no_ass[i..].chars().next().unwrap();
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
        i += ch.len_utf8();
    }
    out
}

fn strip_ass_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find("{\\") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find('}') {
            rest = &after[end + 1..];
        } else {
            out.push_str(&rest[start..]);
            return out;
        }
    }
    out.push_str(rest);
    out
}

pub fn shift_webvtt(vtt: &str, delay_ms: i64) -> String {
    if delay_ms == 0 {
        return vtt.to_owned();
    }
    let normalized = normalize_newlines(vtt);
    let mut out = String::with_capacity(normalized.len());
    let blocks: Vec<&str> = normalized.split("\n\n").collect();
    for (idx, block) in blocks.iter().enumerate() {
        let lines: Vec<&str> = block.lines().collect();
        if lines.is_empty() {
            continue;
        }
        if let Some(timing_idx) = lines
            .iter()
            .take(2)
            .position(|line| parse_timing_line(line).is_some())
            && let Some((start_ms, end_ms, settings)) = parse_timing_line(lines[timing_idx])
        {
            let new_start = start_ms + delay_ms;
            let new_end = end_ms + delay_ms;
            if new_end <= 0 {
                continue;
            }
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            for &prefix in &lines[..timing_idx] {
                out.push_str(prefix);
                out.push('\n');
            }
            out.push_str(&format_vtt_time(new_start.max(0)));
            out.push_str(" --> ");
            out.push_str(&format_vtt_time(new_end));
            if !settings.is_empty() {
                out.push(' ');
                out.push_str(settings);
            }
            for &payload in &lines[timing_idx + 1..] {
                out.push('\n');
                out.push_str(payload);
            }
        } else {
            if !out.is_empty() {
                out.push_str("\n\n");
            } else if idx > 0 {
                out.push('\n');
            }
            out.push_str(block);
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub fn split_chunks(id: u64, name: &str, vtt: &str) -> Result<Vec<SubtitleChunk>, SubtitleError> {
    if id == 0 || !valid_file_name(name) {
        return Err(SubtitleError::InvalidName);
    }
    if vtt.is_empty() {
        return Err(SubtitleError::Empty);
    }
    if vtt.len() > MAX_SUBTITLE_BYTES {
        return Err(SubtitleError::TooLarge);
    }
    let mut slices = Vec::new();
    let mut start = 0;
    while start < vtt.len() {
        let mut end = (start + MAX_SUBTITLE_CHUNK).min(vtt.len());
        while end > start && !vtt.is_char_boundary(end) {
            end -= 1;
        }
        let mut piece = &vtt[start..end];
        while serialized_too_large(id, name, piece) && end > start {
            end = start + (end - start) / 2;
            while end > start && !vtt.is_char_boundary(end) {
                end -= 1;
            }
            piece = &vtt[start..end];
        }
        if piece.is_empty() {
            return Err(SubtitleError::TooLarge);
        }
        slices.push(piece.to_owned());
        start = end;
    }
    if slices.len() > MAX_SUBTITLE_CHUNKS as usize {
        return Err(SubtitleError::TooLarge);
    }
    let total = slices.len() as u32;
    Ok(slices
        .into_iter()
        .enumerate()
        .map(|(index, data)| SubtitleChunk {
            id,
            name: name.to_owned(),
            index: index as u32,
            total,
            data,
        })
        .collect())
}

fn serialized_too_large(id: u64, name: &str, data: &str) -> bool {
    let sample = crate::protocol::Message::SubtitleChunk(Box::new(SubtitleChunk {
        id,
        name: name.to_owned(),
        index: MAX_SUBTITLE_CHUNKS,
        total: MAX_SUBTITLE_CHUNKS,
        data: data.to_owned(),
    }));
    serde_json::to_vec(&sample).is_ok_and(|bytes| bytes.len() > MAX_MESSAGE)
}

#[derive(Default)]
pub struct SubtitleAssembler {
    id: u64,
    name: String,
    total: u32,
    received: u32,
    total_bytes: usize,
    parts: Vec<Option<String>>,
}

impl SubtitleAssembler {
    pub fn clear(&mut self) {
        self.id = 0;
        self.name.clear();
        self.total = 0;
        self.received = 0;
        self.total_bytes = 0;
        self.parts.clear();
    }

    pub fn push(&mut self, chunk: SubtitleChunk) -> Option<(String, String)> {
        if !chunk.valid() {
            return None;
        }
        if self.id != chunk.id {
            self.id = chunk.id;
            self.name = chunk.name;
            self.total = chunk.total;
            self.received = 0;
            self.total_bytes = 0;
            self.parts = vec![None; chunk.total as usize];
        } else if self.total != chunk.total {
            self.clear();
            return None;
        }
        let slot = &mut self.parts[chunk.index as usize];
        if slot.is_none() {
            let next_bytes = self.total_bytes.saturating_add(chunk.data.len());
            if next_bytes > MAX_SUBTITLE_BYTES {
                self.clear();
                return None;
            }
            self.total_bytes = next_bytes;
            *slot = Some(chunk.data);
            self.received += 1;
        }
        if self.received == self.total {
            let name = std::mem::take(&mut self.name);
            let mut full = String::with_capacity(self.total_bytes);
            for part in self.parts.drain(..) {
                full.push_str(&part?);
            }
            self.clear();
            Some((name, full))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_srt_with_bom_crlf_numbered_cues_and_html_tags() {
        let srt = "\u{feff}1\r\n00:00:01,500 --> 00:00:04,250\r\n{\\an8}<i>Olá, mundo!</i>\r\n<font color=\"#ffff00\">Linha 2 & fim</font>\r\n\r\n2\r\n00:01:05,5 --> 00:01:08,05\r\n<b>Negrito</b> e 1 < 2\r\n";
        let vtt = to_webvtt(srt).unwrap();
        assert!(vtt.starts_with("WEBVTT\n\n"));
        assert!(
            vtt.contains("00:00:01.500 --> 00:00:04.250\n<i>Olá, mundo!</i>\nLinha 2 &amp; fim\n")
        );
        assert!(vtt.contains("00:01:05.500 --> 00:01:08.050\n<b>Negrito</b> e 1 &lt; 2\n"));
        assert!(!vtt.contains('\r'));
        assert!(!vtt.contains('\u{feff}'));
        assert!(!vtt.contains("<font"));
        assert!(!vtt.contains("{\\an8}"));
    }

    #[test]
    fn accepts_existing_webvtt_and_normalizes_line_endings() {
        let raw = "\u{feff}WEBVTT - Teste\r\n\r\n00:00:01.000 --> 00:00:03.000 line:85%\r\nLegenda pronta\r\n";
        let vtt = to_webvtt(raw).unwrap();
        assert!(vtt.starts_with("WEBVTT - Teste\n\n"));
        assert!(vtt.contains("00:00:01.000 --> 00:00:03.000 line:85%\nLegenda pronta\n"));
    }

    #[test]
    fn decodes_windows_1252_portuguese_srt() {
        let bytes = b"1\r\n00:00:01,000 --> 00:00:02,000\r\nA\xe7\xe3o e cora\xe7\xe3o\x85\r\n";
        let vtt = from_bytes(bytes).unwrap();
        assert!(vtt.contains("Ação e coração\u{2026}"));
    }

    #[test]
    fn rejects_empty_invalid_and_oversized_subtitles() {
        assert_eq!(to_webvtt("   \r\n\t "), Err(SubtitleError::Empty));
        assert_eq!(
            to_webvtt("apenas texto sem tempos"),
            Err(SubtitleError::InvalidFormat)
        );
        assert_eq!(
            to_webvtt("WEBVTT\n\nsem blocos validos"),
            Err(SubtitleError::InvalidFormat)
        );
        let huge = "a".repeat(MAX_SUBTITLE_BYTES + 1);
        assert_eq!(to_webvtt(&huge), Err(SubtitleError::TooLarge));
        assert_eq!(
            from_bytes(&vec![b'a'; MAX_SUBTITLE_BYTES + 1]),
            Err(SubtitleError::TooLarge)
        );
    }

    #[test]
    fn shifts_webvtt_cues_with_positive_and_negative_delay() {
        let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:03.000 align:center\nPrimeiro\n\n00:00:05.000 --> 00:00:07.500\nSegundo\n";
        let plus = shift_webvtt(vtt, 1500);
        assert!(plus.contains("00:00:02.500 --> 00:00:04.500 align:center\nPrimeiro"));
        assert!(plus.contains("00:00:06.500 --> 00:00:09.000\nSegundo"));

        let minus = shift_webvtt(vtt, -2000);
        assert!(minus.contains("00:00:00.000 --> 00:00:01.000 align:center\nPrimeiro"));
        assert!(minus.contains("00:00:03.000 --> 00:00:05.500\nSegundo"));

        let dropped = shift_webvtt(vtt, -4000);
        assert!(!dropped.contains("Primeiro"));
        assert!(dropped.contains("00:00:01.000 --> 00:00:03.500\nSegundo"));
    }

    #[test]
    fn splits_and_reassembles_chunks_within_message_limit() {
        let mut vtt = String::from("WEBVTT\n\n");
        for i in 0..400 {
            vtt.push_str(&format!(
                "{} --> {}\nFala com acentuação #{i} e \"aspas\"\n\n",
                format_vtt_time(i * 2000),
                format_vtt_time(i * 2000 + 1500)
            ));
        }
        let chunks = split_chunks(7, "filme.pt-BR.srt", &vtt).unwrap();
        assert!(chunks.len() > 2);
        for chunk in &chunks {
            assert!(chunk.valid());
            let json = serde_json::to_vec(&crate::protocol::Message::SubtitleChunk(Box::new(
                chunk.clone(),
            )))
            .unwrap();
            assert!(json.len() <= MAX_MESSAGE);
        }
        let mut assembler = SubtitleAssembler::default();
        let mut assembled = None;
        for chunk in chunks.into_iter().rev() {
            assembled = assembler.push(chunk);
        }
        let (name, text) = assembled.expect("legenda completa");
        assert_eq!(name, "filme.pt-BR.srt");
        assert_eq!(text, vtt);
    }
}
