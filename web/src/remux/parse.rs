use matroska_demuxer::TrackEntry;
use shiguredo_mp4::{
    Decode, FixedPointNumber, Uint,
    boxes::{
        AudioSampleEntryFields, Avc1Box, AvccBox, DopsBox, EsdsBox, Hvc1Box, HvccBox, Mp4aBox,
        OpusBox, SampleEntry, VisualSampleEntryFields,
    },
    codec_string,
    descriptors::{DecoderConfigDescriptor, DecoderSpecificInfo, EsDescriptor, SlConfigDescriptor},
};
use std::io::{Read, Seek, SeekFrom};
use sync2gether_core::protocol::AudioTrackInfo;

const OBJECT_TYPE_MP3: u8 = 0x6B;

#[derive(Clone, Debug)]
pub(super) struct VideoTrackMeta {
    pub track_number: u64,
    pub sample_entry: SampleEntry,
    pub codec_string: String,
    pub default_duration_ticks: u64,
}

#[derive(Clone, Debug)]
pub(super) struct AudioTrackMeta {
    pub info: AudioTrackInfo,
    pub track_number: u64,
    pub sample_entry: SampleEntry,
    pub codec_string: String,
    pub timescale: u32,
    pub default_duration_samples: u32,
}

pub(super) fn parse_video_track(track: &TrackEntry) -> Result<VideoTrackMeta, String> {
    let codec_id = track.codec_id();
    let video = track.video().ok_or_else(|| codec_id.to_owned())?;
    let width = video.pixel_width().get().min(u64::from(u16::MAX)) as u16;
    let height = video.pixel_height().get().min(u64::from(u16::MAX)) as u16;
    let visual = VisualSampleEntryFields {
        data_reference_index: VisualSampleEntryFields::DEFAULT_DATA_REFERENCE_INDEX,
        width,
        height,
        horizresolution: VisualSampleEntryFields::DEFAULT_HORIZRESOLUTION,
        vertresolution: VisualSampleEntryFields::DEFAULT_VERTRESOLUTION,
        frame_count: VisualSampleEntryFields::DEFAULT_FRAME_COUNT,
        compressorname: VisualSampleEntryFields::NULL_COMPRESSORNAME,
        depth: VisualSampleEntryFields::DEFAULT_DEPTH,
    };
    let priv_data = track.codec_private().unwrap_or(&[]);
    let sample_entry = match codec_id {
        "V_MPEG4/ISO/AVC" => {
            let avcc_box = decode_box_payload::<AvccBox>(b"avcC", priv_data)
                .map_err(|_| codec_id.to_owned())?;
            SampleEntry::Avc1(Avc1Box {
                visual,
                avcc_box,
                unknown_boxes: vec![],
            })
        }
        "V_MPEGH/ISO/HEVC" => {
            let hvcc_box = decode_box_payload::<HvccBox>(b"hvcC", priv_data)
                .map_err(|_| codec_id.to_owned())?;
            SampleEntry::Hvc1(Hvc1Box {
                visual,
                hvcc_box,
                unknown_boxes: vec![],
            })
        }
        other => return Err(other.to_owned()),
    };
    let codec_str =
        codec_string::from_sample_entry(&sample_entry).unwrap_or_else(|_| "avc1.64001f".to_owned());
    let default_duration_ticks = track
        .default_duration()
        .map(|d| (d.get() / 1_000_000).max(1))
        .unwrap_or(33);
    Ok(VideoTrackMeta {
        track_number: track.track_number().get(),
        sample_entry,
        codec_string: codec_str,
        default_duration_ticks,
    })
}

pub(super) fn parse_audio_track(track: &TrackEntry, index: u8) -> Result<AudioTrackMeta, String> {
    let codec_id = track.codec_id();
    let audio = track
        .audio()
        .ok_or_else(|| friendly_audio_codec(codec_id))?;
    let sample_rate = audio
        .output_sampling_frequency()
        .unwrap_or_else(|| audio.sampling_frequency())
        .round()
        .clamp(8000.0, 192_000.0) as u32;
    let channels = audio.channels().get().clamp(1, 8) as u8;
    let audio_fields = AudioSampleEntryFields {
        data_reference_index: AudioSampleEntryFields::DEFAULT_DATA_REFERENCE_INDEX,
        channelcount: u16::from(channels),
        samplesize: AudioSampleEntryFields::DEFAULT_SAMPLESIZE,
        samplerate: FixedPointNumber::new(sample_rate.min(u32::from(u16::MAX)) as u16, 0),
    };

    let (sample_entry, codec_name, codec_str, timescale, default_duration_samples) =
        if codec_id == "A_AAC" || codec_id.starts_with("A_AAC/") {
            let asc = match track.codec_private() {
                Some(data) if !data.is_empty() => data.to_vec(),
                _ => synthesize_aac_asc(sample_rate, channels),
            };
            let entry = SampleEntry::Mp4a(Mp4aBox {
                audio: audio_fields,
                esds_box: build_esds(
                    DecoderConfigDescriptor::OBJECT_TYPE_INDICATION_AUDIO_ISO_IEC_14496_3,
                    Some(asc),
                ),
                unknown_boxes: vec![],
            });
            let cstr =
                codec_string::from_sample_entry(&entry).unwrap_or_else(|_| "mp4a.40.2".to_owned());
            (entry, "aac".to_owned(), cstr, sample_rate, 1024)
        } else if codec_id == "A_OPUS" {
            let dops_box = parse_opus_head(track.codec_private(), channels, sample_rate);
            let entry = SampleEntry::Opus(OpusBox {
                audio: AudioSampleEntryFields {
                    samplerate: FixedPointNumber::new(48_000u16, 0),
                    ..audio_fields
                },
                dops_box,
                unknown_boxes: vec![],
            });
            (entry, "opus".to_owned(), "opus".to_owned(), 48_000, 960)
        } else if codec_id == "A_MPEG/L3" {
            let entry = SampleEntry::Mp4a(Mp4aBox {
                audio: audio_fields,
                esds_box: build_esds(OBJECT_TYPE_MP3, None),
                unknown_boxes: vec![],
            });
            (
                entry,
                "mp3".to_owned(),
                "mp4a.6b".to_owned(),
                sample_rate,
                1152,
            )
        } else {
            return Err(friendly_audio_codec(codec_id));
        };

    let language = track
        .language_bcp47()
        .or_else(|| track.language())
        .filter(|s| !s.is_empty())
        .map(|s| truncate_str(s, 32));
    let title = track
        .name()
        .filter(|s| !s.is_empty())
        .map(|s| truncate_str(s, 128));

    Ok(AudioTrackMeta {
        info: AudioTrackInfo {
            index,
            language,
            title,
            codec: codec_name,
            channels,
            default: track.flag_default(),
        },
        track_number: track.track_number().get(),
        sample_entry,
        codec_string: codec_str,
        timescale,
        default_duration_samples,
    })
}

fn build_esds(object_type_indication: u8, asc: Option<Vec<u8>>) -> EsdsBox {
    EsdsBox {
        es: EsDescriptor {
            es_id: 2,
            stream_priority: Uint::new(0),
            depends_on_es_id: None,
            url_string: None,
            ocr_es_id: None,
            dec_config_descr: DecoderConfigDescriptor {
                object_type_indication,
                stream_type: DecoderConfigDescriptor::STREAM_TYPE_AUDIO,
                up_stream: DecoderConfigDescriptor::UP_STREAM_FALSE,
                buffer_size_db: Uint::new(0),
                max_bitrate: 0,
                avg_bitrate: 0,
                dec_specific_info: asc.map(|payload| DecoderSpecificInfo { payload }),
            },
            sl_config_descr: SlConfigDescriptor,
        },
    }
}

fn parse_opus_head(priv_data: Option<&[u8]>, channels: u8, sample_rate: u32) -> DopsBox {
    if let Some(data) = priv_data
        && data.len() >= 19
        && &data[..8] == b"OpusHead"
    {
        return DopsBox {
            output_channel_count: data[9],
            pre_skip: u16::from_le_bytes([data[10], data[11]]),
            input_sample_rate: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
            output_gain: i16::from_le_bytes([data[16], data[17]]),
        };
    }
    DopsBox {
        output_channel_count: channels,
        pre_skip: 312,
        input_sample_rate: sample_rate,
        output_gain: 0,
    }
}

fn synthesize_aac_asc(sample_rate: u32, channels: u8) -> Vec<u8> {
    let freq_idx: u8 = match sample_rate {
        96_000 => 0,
        88_200 => 1,
        64_000 => 2,
        48_000 => 3,
        44_100 => 4,
        32_000 => 5,
        24_000 => 6,
        22_050 => 7,
        16_000 => 8,
        12_000 => 9,
        11_025 => 10,
        8_000 => 11,
        7_350 => 12,
        _ => 3,
    };
    let object_type: u8 = 2;
    let b0 = (object_type << 3) | (freq_idx >> 1);
    let b1 = ((freq_idx & 1) << 7) | ((channels.min(7)) << 3);
    vec![b0, b1]
}

fn friendly_audio_codec(codec_id: &str) -> String {
    match codec_id {
        "A_AC3" => "AC-3".into(),
        "A_EAC3" => "E-AC-3".into(),
        "A_DTS" | "A_DTS/EXPRESS" | "A_DTS/LOSSLESS" => "DTS".into(),
        "A_TRUEHD" => "TrueHD".into(),
        "A_FLAC" => "FLAC".into(),
        "A_VORBIS" => "Vorbis".into(),
        other => other.to_owned(),
    }
}

fn truncate_str(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

fn decode_box_payload<T: Decode>(tag: &[u8; 4], payload: &[u8]) -> Result<T, ()> {
    if payload.is_empty() {
        return Err(());
    }
    let total = (payload.len() + 8) as u32;
    let mut buf = Vec::with_capacity(payload.len() + 8);
    buf.extend_from_slice(&total.to_be_bytes());
    buf.extend_from_slice(tag);
    buf.extend_from_slice(payload);
    T::decode(&buf).map(|(value, _)| value).map_err(|_| ())
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CueEntry {
    pub time_ticks: u64,
    pub cluster_offset: u64,
    pub track: Option<u64>,
}

#[derive(Clone, Debug)]
pub(super) struct EbmlLayout {
    pub first_cluster_offset: u64,
    pub cues: Vec<CueEntry>,
}

#[derive(Clone, Debug)]
pub(super) struct MkvFrame {
    pub track: u64,
    pub timestamp: u64,
    pub is_keyframe: bool,
    pub data: Vec<u8>,
}

pub(super) fn scan_ebml_layout<R: Read + Seek>(reader: &mut R) -> Option<EbmlLayout> {
    reader.seek(SeekFrom::Start(0)).ok()?;
    let (ebml_id, ebml_size) = read_ebml_header(reader)?;
    if ebml_id != 0x1A45_DFA3 {
        return None;
    }
    reader.seek(SeekFrom::Current(ebml_size as i64)).ok()?;
    let (seg_id, _) = read_ebml_header(reader)?;
    if seg_id != 0x1853_8067 {
        return None;
    }
    let segment_data_offset = reader.stream_position().ok()?;
    let mut cues_offset: Option<u64> = None;
    let mut first_cluster_offset: Option<u64> = None;

    for _ in 0..64 {
        let pos = reader.stream_position().ok()?;
        if pos.saturating_sub(segment_data_offset) > 512 * 1024 {
            break;
        }
        let Some((id, size)) = read_ebml_header(reader) else {
            break;
        };
        let body_start = reader.stream_position().ok()?;
        match id {
            0x114D_9B74 => {
                if let Some((cues_rel, cluster_rel)) = parse_seek_head(reader, body_start + size) {
                    if let Some(c) = cues_rel {
                        cues_offset = Some(segment_data_offset + c);
                    }
                    if let Some(cl) = cluster_rel {
                        first_cluster_offset = Some(segment_data_offset + cl);
                    }
                }
                reader.seek(SeekFrom::Start(body_start + size)).ok()?;
            }
            0x1C53_BB6B => {
                cues_offset = Some(pos);
                if size == u64::MAX {
                    break;
                }
                reader.seek(SeekFrom::Start(body_start + size)).ok()?;
            }
            0x1F43_B675 => {
                first_cluster_offset.get_or_insert(pos);
                break;
            }
            _ => {
                if size == u64::MAX {
                    break;
                }
                reader.seek(SeekFrom::Start(body_start + size)).ok()?;
            }
        }
    }

    let mut cues = Vec::new();
    if let Some(cues_pos) = cues_offset
        && reader.seek(SeekFrom::Start(cues_pos)).is_ok()
        && let Some((id, size)) = read_ebml_header(reader)
        && id == 0x1C53_BB6B
        && size != u64::MAX
        && size <= 32 * 1024 * 1024
        && let Ok(cues_start) = reader.stream_position()
    {
        let cues_end = cues_start + size;
        while reader.stream_position().ok().is_some_and(|p| p < cues_end) {
            let Some((child_id, child_size)) = read_ebml_header(reader) else {
                break;
            };
            let Ok(child_start) = reader.stream_position() else {
                break;
            };
            if child_id == 0xBB {
                parse_cue_point(
                    reader,
                    child_start + child_size,
                    segment_data_offset,
                    &mut cues,
                );
            }
            if reader
                .seek(SeekFrom::Start(child_start + child_size))
                .is_err()
            {
                break;
            }
        }
    }

    let first_cluster_offset = first_cluster_offset
        .or_else(|| cues.first().map(|c| c.cluster_offset))
        .unwrap_or(segment_data_offset);
    Some(EbmlLayout {
        first_cluster_offset,
        cues,
    })
}

fn parse_cue_point<R: Read + Seek>(
    reader: &mut R,
    point_end: u64,
    segment_data_offset: u64,
    out: &mut Vec<CueEntry>,
) {
    let mut time_ticks: Option<u64> = None;
    let mut positions: Vec<(Option<u64>, u64)> = Vec::new();
    while reader.stream_position().ok().is_some_and(|p| p < point_end) {
        let Some((f_id, f_size)) = read_ebml_header(reader) else {
            break;
        };
        let Ok(f_start) = reader.stream_position() else {
            break;
        };
        match f_id {
            0xB3 if f_size <= 8 => {
                time_ticks = read_uint(reader, f_size as usize);
            }
            0xB7 => {
                let pos_end = f_start + f_size;
                let mut track: Option<u64> = None;
                let mut cluster_rel: Option<u64> = None;
                while reader.stream_position().ok().is_some_and(|p| p < pos_end) {
                    let Some((p_id, p_size)) = read_ebml_header(reader) else {
                        break;
                    };
                    let Ok(p_start) = reader.stream_position() else {
                        break;
                    };
                    match p_id {
                        0xF7 if p_size <= 8 => track = read_uint(reader, p_size as usize),
                        0xF1 if p_size <= 8 => cluster_rel = read_uint(reader, p_size as usize),
                        _ => {
                            let _ = reader.seek(SeekFrom::Start(p_start + p_size));
                        }
                    }
                }
                if let Some(rel) = cluster_rel {
                    positions.push((track, segment_data_offset + rel));
                }
            }
            _ => {
                let _ = reader.seek(SeekFrom::Start(f_start + f_size));
            }
        }
    }
    if let Some(time_ticks) = time_ticks {
        for (track, cluster_offset) in positions {
            out.push(CueEntry {
                time_ticks,
                cluster_offset,
                track,
            });
        }
    }
}

fn parse_seek_head<R: Read + Seek>(reader: &mut R, end: u64) -> Option<(Option<u64>, Option<u64>)> {
    let mut cues = None;
    let mut cluster = None;
    while reader.stream_position().ok()? < end {
        let (id, size) = read_ebml_header(reader)?;
        let start = reader.stream_position().ok()?;
        if id == 0x4DBB {
            let entry_end = start + size;
            let mut seek_id: Option<u64> = None;
            let mut seek_pos: Option<u64> = None;
            while reader.stream_position().ok()? < entry_end {
                let (f_id, f_size) = read_ebml_header(reader)?;
                let f_start = reader.stream_position().ok()?;
                match f_id {
                    0x53AB if f_size <= 8 => seek_id = read_uint(reader, f_size as usize),
                    0x53AC if f_size <= 8 => seek_pos = read_uint(reader, f_size as usize),
                    _ => {
                        reader.seek(SeekFrom::Start(f_start + f_size)).ok()?;
                    }
                }
            }
            match (seek_id, seek_pos) {
                (Some(0x1C53_BB6B), Some(pos)) => cues = Some(pos),
                (Some(0x1F43_B675), Some(pos)) => {
                    cluster.get_or_insert(pos);
                }
                _ => {}
            }
        }
        reader.seek(SeekFrom::Start(start + size)).ok()?;
    }
    Some((cues, cluster))
}

pub(super) fn read_clusters_from<R: Read + Seek>(
    reader: &mut R,
    start_offset: u64,
    mut on_frame: impl FnMut(u64, MkvFrame) -> bool,
) -> Result<(), String> {
    reader
        .seek(SeekFrom::Start(start_offset))
        .map_err(|e| format!("Falha ao posicionar no arquivo MKV: {e}"))?;
    let mut cluster_ts = 0u64;
    let mut cluster_offset = start_offset;
    while let Ok(elem_pos) = reader.stream_position() {
        let Some((id, size)) = read_ebml_header(reader) else {
            break;
        };
        let Ok(body_start) = reader.stream_position() else {
            break;
        };
        match id {
            0x1F43_B675 => {
                cluster_offset = elem_pos;
                cluster_ts = 0;
            }
            0xE7 if size <= 8 => {
                if let Some(ts) = read_uint(reader, size as usize) {
                    cluster_ts = ts;
                }
            }
            0xA0 => {
                if size == u64::MAX {
                    break;
                }
                let group_end = body_start + size;
                let mut block_frames = Vec::new();
                let mut has_reference = false;
                while reader.stream_position().ok().is_some_and(|p| p < group_end) {
                    let Some((g_id, g_size)) = read_ebml_header(reader) else {
                        break;
                    };
                    let Ok(g_start) = reader.stream_position() else {
                        break;
                    };
                    match g_id {
                        0xA1 if g_size != u64::MAX && g_size <= 32 * 1024 * 1024 => {
                            let mut buf = vec![0u8; g_size as usize];
                            if reader.read_exact(&mut buf).is_ok() {
                                parse_block_frames(&buf, cluster_ts, false, &mut block_frames);
                            }
                        }
                        0xFB => {
                            has_reference = true;
                            let _ = reader.seek(SeekFrom::Start(g_start + g_size));
                        }
                        _ => {
                            if g_size == u64::MAX {
                                break;
                            }
                            let _ = reader.seek(SeekFrom::Start(g_start + g_size));
                        }
                    }
                }
                let _ = reader.seek(SeekFrom::Start(group_end));
                for mut f in block_frames {
                    f.is_keyframe = !has_reference;
                    if !on_frame(cluster_offset, f) {
                        return Ok(());
                    }
                }
            }
            0xA3 if size != u64::MAX && size <= 32 * 1024 * 1024 => {
                let mut buf = vec![0u8; size as usize];
                if reader.read_exact(&mut buf).is_err() {
                    break;
                }
                let mut frames = Vec::new();
                parse_block_frames(&buf, cluster_ts, true, &mut frames);
                for f in frames {
                    if !on_frame(cluster_offset, f) {
                        return Ok(());
                    }
                }
            }
            0x1C53_BB6B | 0x1254_C367 | 0x1043_A770 | 0x1941_A469 => break,
            _ => {
                if size == u64::MAX {
                    break;
                }
                if reader.seek(SeekFrom::Start(body_start + size)).is_err() {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn parse_block_frames(buf: &[u8], cluster_ts: u64, simple: bool, out: &mut Vec<MkvFrame>) {
    let mut cursor = std::io::Cursor::new(buf);
    let Some(track) = read_vint(&mut cursor, false) else {
        return;
    };
    let mut hdr = [0u8; 3];
    if cursor.read_exact(&mut hdr).is_err() {
        return;
    }
    let rel_ts = i16::from_be_bytes([hdr[0], hdr[1]]);
    let flags = hdr[2];
    let ts = if rel_ts >= 0 {
        cluster_ts.saturating_add(rel_ts as u64)
    } else {
        cluster_ts.saturating_sub(rel_ts.unsigned_abs() as u64)
    };
    let is_keyframe = simple && (flags & 0x80) != 0;
    let lacing = (flags >> 1) & 0x03;
    let pos = cursor.position() as usize;
    let rest = &buf[pos..];
    if lacing == 0 {
        out.push(MkvFrame {
            track,
            timestamp: ts,
            is_keyframe,
            data: rest.to_vec(),
        });
        return;
    }
    if rest.is_empty() {
        return;
    }
    let frame_count = rest[0] as usize + 1;
    let mut idx = 1usize;
    let mut sizes = Vec::with_capacity(frame_count);
    match lacing {
        1 => {
            for _ in 0..frame_count - 1 {
                let mut total = 0usize;
                loop {
                    if idx >= rest.len() {
                        return;
                    }
                    let b = rest[idx] as usize;
                    idx += 1;
                    total += b;
                    if b < 255 {
                        break;
                    }
                }
                sizes.push(total);
            }
            let used: usize = sizes.iter().sum();
            let rem = rest.len().saturating_sub(idx);
            if used > rem {
                return;
            }
            sizes.push(rem - used);
        }
        2 => {
            let rem = rest.len().saturating_sub(idx);
            if !rem.is_multiple_of(frame_count) {
                return;
            }
            let each = rem / frame_count;
            for _ in 0..frame_count {
                sizes.push(each);
            }
        }
        3 => {
            let mut lcur = std::io::Cursor::new(&rest[idx..]);
            let Some(first) = read_vint(&mut lcur, false) else {
                return;
            };
            let mut prev = first as i64;
            sizes.push(first as usize);
            for _ in 1..frame_count - 1 {
                let Some(delta) = read_signed_vint(&mut lcur) else {
                    return;
                };
                prev += delta;
                if prev < 0 {
                    return;
                }
                sizes.push(prev as usize);
            }
            idx += lcur.position() as usize;
            let used: usize = sizes.iter().sum();
            let rem = rest.len().saturating_sub(idx);
            if used > rem {
                return;
            }
            sizes.push(rem - used);
        }
        _ => return,
    }
    for sz in sizes {
        if idx + sz > rest.len() {
            break;
        }
        out.push(MkvFrame {
            track,
            timestamp: ts,
            is_keyframe,
            data: rest[idx..idx + sz].to_vec(),
        });
        idx += sz;
    }
}

fn read_signed_vint<R: Read>(reader: &mut R) -> Option<i64> {
    let mut first = [0u8; 1];
    reader.read_exact(&mut first).ok()?;
    let leading = first[0].leading_zeros() as usize;
    if leading >= 8 {
        return None;
    }
    let width = leading + 1;
    let mask = (1u8 << (7 - leading)) - 1;
    let mut value = u64::from(first[0] & mask);
    let mut rest = [0u8; 7];
    if width > 1 {
        reader.read_exact(&mut rest[..width - 1]).ok()?;
        for &b in &rest[..width - 1] {
            value = (value << 8) | u64::from(b);
        }
    }
    let bias = (1i64 << (width * 7 - 1)) - 1;
    Some(value as i64 - bias)
}

fn read_ebml_header<R: Read>(reader: &mut R) -> Option<(u64, u64)> {
    let id = read_vint(reader, true)?;
    let size = read_vint(reader, false)?;
    Some((id, size))
}

fn read_vint<R: Read>(reader: &mut R, keep_marker: bool) -> Option<u64> {
    let mut first = [0u8; 1];
    reader.read_exact(&mut first).ok()?;
    let leading = first[0].leading_zeros() as usize;
    if leading >= 8 {
        return None;
    }
    let width = leading + 1;
    let mask = if keep_marker {
        0xFFu8
    } else {
        (1u8 << (7 - leading)) - 1
    };
    let mut value = u64::from(first[0] & mask);
    let mut all_ones = !keep_marker && (first[0] & mask) == mask;
    let mut rest = [0u8; 7];
    if width > 1 {
        reader.read_exact(&mut rest[..width - 1]).ok()?;
        for &b in &rest[..width - 1] {
            value = (value << 8) | u64::from(b);
            if b != 0xFF {
                all_ones = false;
            }
        }
    }
    if all_ones {
        return Some(u64::MAX);
    }
    Some(value)
}

fn read_uint<R: Read>(reader: &mut R, len: usize) -> Option<u64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf[..len]).ok()?;
    let mut value = 0u64;
    for &b in &buf[..len] {
        value = (value << 8) | u64::from(b);
    }
    Some(value)
}
