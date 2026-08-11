use std::fs;
use std::io;
use std::path::Path;

/// FNV-1a 64-bit hash — fixed algorithm so avatar colors and cache
/// filenames stay stable across Rust versions and platforms.
fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub fn identity_hash(hostname: &str, ip: &str) -> u64 {
    fnv1a(&format!("{hostname}+{ip}"))
}

/// Hue in [0, 360) derived from the identity hash.
pub fn hue(hostname: &str, ip: &str) -> u32 {
    (identity_hash(hostname, ip) % 360) as u32
}

/// First letter of the hostname, uppercased; falls back to the IP's
/// first character, then '?'.
pub fn first_letter(hostname: &str, ip: &str) -> char {
    hostname
        .chars()
        .next()
        .or_else(|| ip.chars().next())
        .unwrap_or('?')
        .to_ascii_uppercase()
}

fn svg(letter: char, hue: u32) -> String {
    // Fixed mid-tone saturation/lightness keeps the white letter legible.
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"96\" height=\"96\">\
         <circle cx=\"48\" cy=\"48\" r=\"48\" fill=\"hsl({hue},55%,45%)\"/>\
         <text x=\"48\" y=\"64\" font-size=\"48\" font-family=\"sans-serif\" \
         fill=\"#ffffff\" text-anchor=\"middle\">{letter}</text></svg>"
    )
}

pub fn avatar_file_name(hostname: &str, ip: &str) -> String {
    format!("{:016x}.svg", identity_hash(hostname, ip))
}

pub fn avatar_url(hostname: &str, ip: &str) -> String {
    format!("/files/icons/{}", avatar_file_name(hostname, ip))
}

/// Ensure the avatar SVG exists under `<icons_dir>/<hash>.svg`.
/// Idempotent: existing files are left untouched.
pub fn ensure_avatar(icons_dir: &Path, hostname: &str, ip: &str) -> io::Result<()> {
    fs::create_dir_all(icons_dir)?;
    let path = icons_dir.join(avatar_file_name(hostname, ip));
    if path.exists() {
        return Ok(());
    }
    let content = svg(first_letter(hostname, ip), hue(hostname, ip));
    fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_hash_and_color() {
        let h1 = identity_hash("macbook", "192.168.1.2");
        let h2 = identity_hash("macbook", "192.168.1.2");
        assert_eq!(h1, h2);
        assert_eq!(hue("macbook", "192.168.1.2"), hue("macbook", "192.168.1.2"));
        assert_eq!(
            avatar_file_name("macbook", "192.168.1.2"),
            avatar_file_name("macbook", "192.168.1.2")
        );
    }

    #[test]
    fn different_identities_get_different_hashes() {
        let mut hues = std::collections::HashSet::new();
        let mut hash_differs = false;
        let base = identity_hash("host0", "192.168.1.0");
        for i in 1..50 {
            let h = identity_hash(&format!("host{i}"), &format!("192.168.1.{i}"));
            if h != base {
                hash_differs = true;
            }
            hues.insert((h % 360) as u32);
        }
        assert!(hash_differs);
        // Expect reasonable hue spread across 49 distinct identities.
        assert!(hues.len() > 30, "hues too clustered: {}", hues.len());
    }

    #[test]
    fn first_letter_extraction() {
        assert_eq!(first_letter("macbook", "1.2.3.4"), 'M');
        assert_eq!(first_letter("Desktop-PC", "1.2.3.4"), 'D');
        assert_eq!(first_letter("", "192.168.1.1"), '1');
        assert_eq!(first_letter("", ""), '?');
    }

    #[test]
    fn svg_contains_legible_midtone_color() {
        let s = svg('A', 200);
        assert!(s.contains("hsl(200,55%,45%)"));
        assert!(s.contains("fill=\"#ffffff\""));
        assert!(s.contains(">A</text>"));
    }

    #[test]
    fn ensure_avatar_writes_once() {
        let dir = std::env::temp_dir().join(format!("easyshare-avatar-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        ensure_avatar(&dir, "testhost", "10.0.0.1").unwrap();
        let path = dir.join(avatar_file_name("testhost", "10.0.0.1"));
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("<svg"));
        assert!(content.contains(">T</text>"));
        // Second call must not fail nor truncate.
        ensure_avatar(&dir, "testhost", "10.0.0.1").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
        let _ = fs::remove_dir_all(&dir);
    }
}
