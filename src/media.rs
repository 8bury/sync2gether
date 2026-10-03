//! Identidade do arquivo calculada fora da UI, sem expor o caminho à rede.
use crate::protocol::Media;
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

pub fn identify(path: &Path) -> io::Result<Media> {
    identify_with_progress(path, |_, _| Ok(()))
}

/// O callback permite cancelar entre blocos; nenhuma leitura passa pela UI.
pub fn identify_with_progress(
    path: &Path,
    mut progress: impl FnMut(u64, u64) -> io::Result<()>,
) -> io::Result<Media> {
    let mut file = File::open(path)?;
    let before = file.metadata()?;
    if !before.is_file() {
        return Err(io::Error::other("Selecione um arquivo regular"));
    }
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0; 1024 * 1024];
    let mut bytes = 0;
    progress(0, before.len())?;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        hasher.update(&buffer[..n]);
        progress(bytes.min(before.len()), before.len())?;
    }
    let after = file.metadata()?;
    if bytes != before.len()
        || after.len() != before.len()
        || after.modified()? != before.modified()?
    {
        return Err(io::Error::other("Arquivo mudou durante a verificação"));
    }
    Ok(Media {
        hash: hasher.finalize().to_hex().to_string(),
        bytes,
    })
}

/// Compartilha apenas o nome escolhido, nunca os diretórios.
pub fn display_name(path: &Path) -> String {
    let raw = path.file_name().unwrap_or_default().to_string_lossy();
    let mut name = String::new();
    for c in raw.chars() {
        let c = if c.is_control()
            || matches!(c, '/' | '\\' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            '_'
        } else {
            c
        };
        if name.len() + c.len_utf8() > 1024 {
            break;
        }
        name.push(c);
    }
    if name.trim().is_empty() {
        "Arquivo local".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_is_content_based() {
        let directory =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/media-test");
        std::fs::create_dir_all(&directory).unwrap();
        let a = directory.join("a");
        let b = directory.join("b");
        std::fs::write(&a, b"synthetic fixture").unwrap();
        std::fs::write(&b, b"synthetic fixture").unwrap();
        assert_eq!(identify(&a).unwrap(), identify(&b).unwrap());
        std::fs::write(&b, b"different fixture").unwrap();
        assert_ne!(identify(&a).unwrap(), identify(&b).unwrap());
    }

    #[test]
    fn verification_reports_progress_and_can_stop_between_blocks() {
        let directory =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/media-progress-test");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("synthetic.bin");
        std::fs::write(&path, vec![42; 3 * 1024 * 1024]).unwrap();
        let mut updates = Vec::new();
        let result = identify_with_progress(&path, |bytes, total| {
            updates.push((bytes, total));
            if bytes > 0 {
                Err(io::Error::other("cancelled"))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert_eq!(
            updates,
            [(0, 3 * 1024 * 1024), (1024 * 1024, 3 * 1024 * 1024)]
        );
        updates.clear();
        let media = identify_with_progress(&path, |bytes, total| {
            updates.push((bytes, total));
            Ok(())
        })
        .unwrap();
        assert_eq!(updates.last(), Some(&(media.bytes, media.bytes)));
        assert!(updates.windows(2).all(|w| w[0].0 <= w[1].0));
        assert_eq!(media, identify(&path).unwrap());
    }

    #[test]
    fn shared_name_omits_directories_and_removes_control_characters() {
        assert_eq!(
            display_name(Path::new("/private/movies/Filme.1080p.mkv")),
            "Filme.1080p.mkv"
        );
        for path in [
            "/private/a\nb.mkv".to_owned(),
            format!("/private/{}", "é".repeat(1024)),
            "/private/\u{202e}mkv.txt".to_owned(),
            "/private/\\film.mkv".to_owned(),
        ] {
            let name = display_name(Path::new(&path));
            assert!(crate::protocol::valid_file_name(&name));
            assert!(!name.contains("private"));
        }
    }
}
