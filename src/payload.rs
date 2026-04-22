/*! Embedded Nix closure and metadata

The installer carries the zstd-compressed Nix closure (and the store
paths needed to set up the default profile) as a *trailer* appended to
its own executable by `scripts/pack`, so the Rust build is independent
of the Nix being shipped and stays cacheable.

Trailer layout (all little-endian), appended after the original
executable bytes:

```text
  [tarball bytes]
  [metadata JSON]
  u32  metadata length
  u64  tarball length
  [8]  magic = "NIXINST1"
```
*/

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    sync::OnceLock,
};

/// Magic bytes terminating an appended payload.
///
/// Built at runtime so the literal does not appear verbatim in the
/// executable's own `.rodata`, where the backward scan could pick it
/// up by accident.
fn magic() -> [u8; 8] {
    let mut m = *std::hint::black_box(b"1TSNIXIN");
    m.reverse();
    m
}
const MAGIC_LEN: usize = 8;
/// `u32` meta_len + `u64` tarball_len + magic.
pub const FOOTER_LEN: u64 = 4 + 8 + MAGIC_LEN as u64;
/// On macOS the trailer is followed by a code-signature blob, so the
/// footer is not at EOF.  Scan this much from the end for `MAGIC`;
/// ad-hoc signatures are well under this.
const TAIL_SCAN: u64 = 1 << 20;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Metadata {
    pub nix_store_path: String,
    pub cacert_store_path: String,
    pub nix_version: String,
}

pub struct Payload {
    pub tarball: Vec<u8>,
    pub nix_store_path: String,
    pub cacert_store_path: String,
    pub nix_version: String,
}

static PAYLOAD: OnceLock<Payload> = OnceLock::new();

/// The appended payload, or exit with a hint if this is a bare binary
/// that was never `pack`ed.
pub fn get() -> &'static Payload {
    PAYLOAD.get_or_init(|| match read_trailer() {
        Ok(Some(p)) => {
            tracing::debug!(nix_version = %p.nix_version, "Loaded appended Nix payload");
            p
        },
        Ok(None) => {
            tracing::error!(
                "This nix-installer binary carries no Nix closure. \
                 Run `scripts/pack` to append one, or use a release build.",
            );
            std::process::exit(1);
        },
        Err(e) => {
            tracing::error!("Reading appended Nix payload from own executable: {e}");
            std::process::exit(1);
        },
    })
}

fn read_trailer() -> io::Result<Option<Payload>> {
    let exe = std::env::current_exe()?;
    let mut f = File::open(&exe)?;
    read_trailer_from(&mut f)
}

/// Split out for testing.
pub(crate) fn read_trailer_from<R: Read + Seek>(f: &mut R) -> io::Result<Option<Payload>> {
    let len = f.seek(SeekFrom::End(0))?;
    if len < FOOTER_LEN {
        return Ok(None);
    }

    // The footer is usually at EOF, but on macOS a code-signature blob
    // sits after it.  Scan a bounded tail for the *last* occurrence of
    // MAGIC and validate it.
    let scan = TAIL_SCAN.min(len);
    let tail_start = len - scan;
    f.seek(SeekFrom::Start(tail_start))?;
    let mut tail = vec![0u8; scan as usize];
    f.read_exact(&mut tail)?;

    let magic = magic();
    for off in memrfind(&tail, &magic) {
        let footer_end = tail_start + off as u64 + MAGIC_LEN as u64;
        if let Some(p) = try_footer_at(f, len, footer_end)? {
            return Ok(Some(p));
        }
    }
    Ok(None)
}

fn try_footer_at<R: Read + Seek>(
    f: &mut R,
    file_len: u64,
    footer_end: u64,
) -> io::Result<Option<Payload>> {
    if footer_end < FOOTER_LEN {
        return Ok(None);
    }
    f.seek(SeekFrom::Start(footer_end - FOOTER_LEN))?;
    let mut footer = [0u8; FOOTER_LEN as usize];
    f.read_exact(&mut footer)?;

    let meta_len = u32::from_le_bytes(footer[0..4].try_into().unwrap()) as u64;
    let tarball_len = u64::from_le_bytes(footer[4..12].try_into().unwrap());

    let payload_len = match tarball_len
        .checked_add(meta_len)
        .and_then(|n| n.checked_add(FOOTER_LEN))
    {
        Some(n) if n <= footer_end && n <= file_len => n,
        _ => return Ok(None),
    };

    f.seek(SeekFrom::Start(footer_end - payload_len))?;
    let mut tarball = vec![0u8; tarball_len as usize];
    f.read_exact(&mut tarball)?;
    let mut meta_buf = vec![0u8; meta_len as usize];
    f.read_exact(&mut meta_buf)?;

    let meta: Metadata = match serde_json::from_slice(&meta_buf) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };

    Ok(Some(Payload {
        tarball,
        nix_store_path: meta.nix_store_path,
        cacert_store_path: meta.cacert_store_path,
        nix_version: meta.nix_version,
    }))
}

/// Yield byte offsets of `needle` in `hay`, from the end.
fn memrfind<'a>(hay: &'a [u8], needle: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    let mut i = (hay.len() + 1).saturating_sub(needle.len());
    std::iter::from_fn(move || {
        while i > 0 {
            i -= 1;
            if hay[i..].starts_with(needle) {
                return Some(i);
            }
        }
        None
    })
}

/// Append `tarball` and `meta` as a trailer to `out`.  Mirrors
/// `scripts/pack`; kept for round-trip tests.
#[cfg(test)]
fn write_trailer<W: io::Write>(mut out: W, tarball: &[u8], meta: &Metadata) -> io::Result<()> {
    let meta_buf = serde_json::to_vec(meta).unwrap();
    out.write_all(tarball)?;
    out.write_all(&meta_buf)?;
    out.write_all(&(meta_buf.len() as u32).to_le_bytes())?;
    out.write_all(&(tarball.len() as u64).to_le_bytes())?;
    out.write_all(&magic())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn no_trailer_on_plain_data() {
        let mut c = Cursor::new(b"not an installer".to_vec());
        assert!(read_trailer_from(&mut c).unwrap().is_none());
    }

    #[test]
    fn roundtrip() {
        let mut buf = b"pretend this is an ELF".to_vec();
        let tarball = b"zstd-compressed-stuff";
        let meta = Metadata {
            nix_store_path: "/nix/store/aaa-nix".into(),
            cacert_store_path: "/nix/store/bbb-cacert".into(),
            nix_version: "9.99".into(),
        };
        write_trailer(&mut buf, tarball, &meta).unwrap();

        let mut c = Cursor::new(buf);
        let p = read_trailer_from(&mut c).unwrap().unwrap();
        assert_eq!(&*p.tarball, tarball);
        assert_eq!(p.nix_store_path, "/nix/store/aaa-nix");
        assert_eq!(p.cacert_store_path, "/nix/store/bbb-cacert");
        assert_eq!(p.nix_version, "9.99");
    }

    #[test]
    fn rejects_oversized_claim() {
        let mut buf = b"exe".to_vec();
        // Hand-craft a footer that claims more bytes than exist.
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&u64::MAX.to_le_bytes());
        buf.extend_from_slice(&magic());
        let mut c = Cursor::new(buf);
        // Spurious magic with garbage lengths is skipped, not fatal.
        assert!(read_trailer_from(&mut c).unwrap().is_none());
    }

    #[test]
    fn finds_trailer_before_trailing_junk() {
        // Simulates macOS: code-signature blob appended after our trailer.
        let mut buf = b"pretend this is a Mach-O".to_vec();
        let tarball = b"zstd";
        let meta = Metadata {
            nix_store_path: "/nix/store/aaa-nix".into(),
            cacert_store_path: "/nix/store/bbb-cacert".into(),
            nix_version: "1.0".into(),
        };
        write_trailer(&mut buf, tarball, &meta).unwrap();
        buf.extend_from_slice(&[0xAB; 4096]); // fake signature blob

        let mut c = Cursor::new(buf);
        let p = read_trailer_from(&mut c).unwrap().unwrap();
        assert_eq!(&*p.tarball, tarball);
        assert_eq!(p.nix_version, "1.0");
    }
}
