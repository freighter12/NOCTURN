//! Title/artist matching, shared by lyrics and playlist import.
//! Same rules as the desktop app (src-tauri/src/music_import.rs).

use std::collections::HashSet;

fn fold(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'ç' | 'č' | 'ć' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' | 'ń' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'ß' => 's',
        'š' | 'ś' => 's',
        'ž' | 'ź' | 'ż' => 'z',
        'ł' => 'l',
        '’' | '‘' | '`' => '\'',
        c => c,
    }
}

const NOISE: &[&str] = &[
    "official", "video", "audio", "lyric", "lyrics", "visualizer", "visualiser", "remaster", "remastered", "hd", "4k", "hq", "explicit", "clean", "mv", "m/v", "music video", "feat", "ft", "featuring", "with", "prod", "single version", "album version", "radio edit", "original mix", "color coded",
];

fn is_noise(seg: &str) -> bool {
    let s = seg.trim();
    s.is_empty() || NOISE.iter().any(|n| s == *n || s.starts_with(&format!("{n} ")) || s.starts_with(&format!("{n}.")) || s.ends_with(&format!(" {n}")) || s.contains(&format!(" {n} ")))
        || s.chars().all(|c| c.is_ascii_digit() || c == ' ')
}

/// Normalised title words: lower case, accents folded, bracketed/dashed noise
/// ("(Official Video)", "- Remastered 2011", "feat. X") removed. Version words
/// like live/remix/acoustic/slowed are kept so versions don't get mixed up.
pub fn clean_title(t: &str) -> String {
    let lower: String = t.to_lowercase().chars().map(fold).collect();
    let mut out = String::new();
    let mut depth_seg = String::new();
    let mut depth = 0;
    for c in lower.chars() {
        match c {
            '(' | '[' | '{' => {
                if depth == 0 {
                    depth_seg.clear();
                } else {
                    depth_seg.push(c);
                }
                depth += 1;
            }
            ')' | ']' | '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 && !is_noise(&depth_seg) {
                    out.push(' ');
                    out.push_str(&depth_seg);
                    out.push(' ');
                } else if depth > 0 {
                    depth_seg.push(c);
                }
            }
            _ if depth > 0 => depth_seg.push(c),
            _ => out.push(c),
        }
    }
    let mut parts: Vec<&str> = out.split(" - ").collect();
    while parts.len() > 1 && is_noise(parts[parts.len() - 1]) {
        parts.pop();
    }
    let mut s = parts.join(" ");
    for k in [" feat. ", " feat ", " ft. ", " ft ", " featuring "] {
        if let Some(i) = s.find(k) {
            s.truncate(i);
        }
    }
    s.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn artist_set(a: &str) -> HashSet<String> {
    let lower: String = a.to_lowercase().chars().map(fold).collect();
    let mut s = lower;
    for sep in [" feat. ", " feat ", " ft. ", " ft ", " featuring ", " & ", " and ", " x ", " with ", ";", "/", ",", "、"] {
        s = s.replace(sep, "|");
    }
    s.split('|')
        .map(|p| p.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .map(|p| p.strip_prefix("the").filter(|x| x.len() > 2).map(String::from).unwrap_or(p))
        .map(|p| p.strip_suffix("vevo").map(String::from).unwrap_or(p))
        .filter(|p| !p.is_empty())
        .collect()
}

fn words(s: &str) -> HashSet<&str> {
    s.split(' ').filter(|w| !w.is_empty()).collect()
}

/// Dice similarity of word sets (1.0 = same words).
pub fn title_sim(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let (wa, wb) = (words(a), words(b));
    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    let common = wa.intersection(&wb).count() as f64;
    2.0 * common / (wa.len() + wb.len()) as f64
}

/// Do two artist strings share an artist ("The Weeknd & Daft Punk" vs "Daft Punk")?
pub fn artists_overlap(a: &str, b: &str) -> bool {
    let wa = artist_set(a);
    let hb = artist_set(b);
    wa.is_empty() || hb.is_empty() || wa.iter().any(|x| hb.iter().any(|y| x == y || (x.len() > 3 && y.contains(x.as_str())) || (y.len() > 3 && x.contains(y.as_str()))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(clean_title("Hey Jude (Remastered 2015)"), "hey jude");
        assert_eq!(clean_title("Blinding Lights - Radio Edit"), "blinding lights");
        assert_eq!(clean_title("Starboy (feat. Daft Punk)"), "starboy");
        assert_eq!(clean_title("Stay (Slowed + Reverb)"), "stay slowed reverb");
        assert!(artists_overlap("The Weeknd, Daft Punk", "Daft Punk"));
        assert!(!artists_overlap("Adele", "Drake"));
    }
}
