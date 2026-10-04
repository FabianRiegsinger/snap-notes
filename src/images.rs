use crate::note::Note;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Largest image file accepted for import.
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;

const EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
const DIR: &str = "images";

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}

/// Writes via `write` to a temp file, renames it to `images/<uuid>.<ext>` and returns that path.
fn store(dir: &Path, ext: &str, write: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<String> {
    let images = dir.join(DIR);
    fs::create_dir_all(&images)?;
    let name = format!("{}.{ext}", Uuid::new_v4());
    let dest = images.join(&name);
    let tmp = images.join(format!("{name}.tmp"));
    if let Err(e) = write(&tmp).and_then(|()| fs::rename(&tmp, &dest)) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(format!("{DIR}/{name}"))
}

/// Copies `src` into `images/` under a fresh uuid name and returns `images/<uuid>.<ext>`.
pub fn import(dir: &Path, src: &Path) -> io::Result<String> {
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| EXTENSIONS.contains(&e.as_str()))
        .ok_or_else(|| invalid("unsupported image type"))?;
    if fs::metadata(src)?.len() > MAX_BYTES {
        return Err(invalid("image too large"));
    }
    store(dir, &ext, |tmp| fs::copy(src, tmp).map(|_| ()))
}

/// Encodes RGBA pixels as a PNG into `images/` and returns `images/<uuid>.png`.
pub fn import_png(dir: &Path, rgba: &[u8], width: u32, height: u32) -> io::Result<String> {
    store(dir, "png", |tmp| {
        let file = io::BufWriter::new(fs::File::create(tmp)?);
        let mut enc = png::Encoder::new(file, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(io::Error::other)?;
        writer.write_image_data(rgba).map_err(io::Error::other)?;
        writer.finish().map_err(io::Error::other)
    })
}

/// Maps a note's `images/<file>` reference to an existing file, or `None` for anything else.
pub fn resolve(dir: &Path, rel: &str) -> Option<PathBuf> {
    let name = rel.strip_prefix("images/")?;
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.contains("..") {
        return None;
    }
    let path = dir.join(DIR).join(name);
    path.is_file().then_some(path)
}

/// Whether `rel` resolves to an image file that can be decoded (its header
/// is read, not the whole image).
pub fn is_usable(dir: &Path, rel: &str) -> bool {
    resolve(dir, rel).is_some_and(|path| image::image_dimensions(path).is_ok())
}

/// Deletes `<uuid>.<ext>` files in `images/` that no note references; returns how many.
pub fn sweep(dir: &Path, notes: &[Note]) -> io::Result<usize> {
    let entries = match fs::read_dir(dir.join(DIR)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut removed = 0;
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| EXTENSIONS.contains(&e));
        let stem_ok = path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| Uuid::parse_str(s).is_ok());
        if !(ext_ok && stem_ok) {
            continue;
        }
        let needle = format!("{DIR}/{name}");
        if !notes.iter().any(|n| n.content.contains(&needle)) {
            fs::remove_file(&path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::{Note, PALETTE};
    use std::fs;
    use std::fs::File;
    use std::io::ErrorKind;

    fn src_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn import_copies_under_uuid_name() {
        let d = tempfile::tempdir().unwrap();
        let src = src_file(d.path(), "photo.PNG", b"abc");
        let rel = import(d.path(), &src).unwrap();
        assert!(rel.starts_with("images/") && rel.ends_with(".png"));
        assert_eq!(rel.len(), "images/".len() + 36 + ".png".len());
        assert_eq!(fs::read(d.path().join(&rel)).unwrap(), b"abc");
    }

    #[test]
    fn import_rejects_other_types() {
        let d = tempfile::tempdir().unwrap();
        let src = src_file(d.path(), "notes.txt", b"x");
        assert_eq!(
            import(d.path(), &src).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
    }

    #[test]
    fn import_rejects_oversized() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("big.png");
        File::create(&p).unwrap().set_len(MAX_BYTES + 1).unwrap();
        assert_eq!(
            import(d.path(), &p).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
    }

    #[test]
    fn import_png_writes_decodable_png() {
        let d = tempfile::tempdir().unwrap();
        let rel = import_png(d.path(), &[255; 16], 2, 2).unwrap();
        assert!(rel.ends_with(".png"));
        let dec = png::Decoder::new(std::io::BufReader::new(
            File::open(d.path().join(&rel)).unwrap(),
        ));
        let reader = dec.read_info().unwrap();
        assert_eq!(reader.info().width, 2);
    }

    #[test]
    fn resolve_accepts_imported() {
        let d = tempfile::tempdir().unwrap();
        let src = src_file(d.path(), "a.jpg", b"x");
        let rel = import(d.path(), &src).unwrap();
        assert_eq!(resolve(d.path(), &rel), Some(d.path().join(&rel)));
    }

    #[test]
    fn resolve_rejects_escapes() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("notes.json"), "{}").unwrap();
        fs::create_dir_all(d.path().join("images/sub")).unwrap();
        fs::write(d.path().join("images/sub/x.png"), "x").unwrap();
        for rel in [
            "../notes.json",
            "images/../notes.json",
            "images/sub/x.png",
            "/etc/hosts",
            "C:\\x.png",
            "images\\x.png",
            "https://e.com/a.png",
            "images/",
            "images/..",
        ] {
            assert_eq!(resolve(d.path(), rel), None, "{rel}");
        }
    }

    #[test]
    fn sweep_removes_only_unreferenced_uuid_files() {
        let d = tempfile::tempdir().unwrap();
        let a = import(d.path(), &src_file(d.path(), "a.png", b"a")).unwrap();
        let b = import(d.path(), &src_file(d.path(), "b.png", b"b")).unwrap();
        let mut note = Note::new(PALETTE[0]);
        note.content = format!("hi ![x]({a})");
        assert_eq!(sweep(d.path(), &[note]).unwrap(), 1);
        assert!(d.path().join(&a).exists());
        assert!(!d.path().join(&b).exists());
    }

    #[test]
    fn sweep_keeps_foreign_files() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("images")).unwrap();
        fs::write(d.path().join("images/mine.png"), "x").unwrap();
        assert_eq!(sweep(d.path(), &[]).unwrap(), 0);
        assert!(d.path().join("images/mine.png").exists());
    }

    #[test]
    fn sweep_without_dir_is_ok() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(sweep(d.path(), &[]).unwrap(), 0);
    }
}
