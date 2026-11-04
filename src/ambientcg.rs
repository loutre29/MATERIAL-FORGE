//! ambientCG (https://ambientcg.com, CC0 scanned materials) for Muse: the
//! material catalog (ids, tags, popularity; cached for a week), the best
//! match for a few tag words, and the download of a material's 2K maps into
//! Muse's own cache (`<data>/muse/scans/<category>/<id>`, the starter
//! assets' layout). A scan belongs to the projects that use it (they keep
//! its maps as resources), not to the shared library.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const API: &str = "https://ambientcg.com/api/v2/full_json";

#[derive(Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub category: String,
    pub tags: Vec<String>,
    pub popularity: f64,
}

/// Where downloaded scans are kept, out of the library.
pub fn scans_dir() -> PathBuf {
    crate::library::library_dir().join("..").join("muse").join("scans")
}

/// Moves the scans older versions downloaded into the library
/// (`material.json` with source ambientCG) to `scans_dir`, so they leave
/// the shared Materials shelf. Returns how many moved (failures go to
/// stderr).
pub fn move_scans_out_of_library() -> usize {
    let lib = crate::library::library_dir().join("materials");
    let Ok(cats) = std::fs::read_dir(&lib) else { return 0 };
    let mut moved = 0;
    for cat in cats.flatten().filter(|e| e.path().is_dir()) {
        let Ok(items) = std::fs::read_dir(cat.path()) else { continue };
        for item in items.flatten() {
            let dir = item.path();
            let scan = std::fs::read(dir.join("material.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .is_some_and(|v| v["source"] == "ambientCG");
            if !scan {
                continue;
            }
            let to = scans_dir().join(cat.file_name()).join(item.file_name());
            let r = if to.join("material.json").is_file() {
                std::fs::remove_dir_all(&dir)
            } else {
                let _ = std::fs::create_dir_all(to.parent().unwrap());
                let _ = std::fs::remove_dir_all(&to);
                std::fs::rename(&dir, &to)
            };
            match r {
                Ok(()) => moved += 1,
                Err(e) => eprintln!("moving the scan {} out of the library: {e}", dir.display()),
            }
        }
        // The category folder when the scans were all it held.
        if std::fs::read_dir(cat.path()).is_ok_and(|mut d| d.next().is_none()) {
            let _ = std::fs::remove_dir(cat.path());
        }
    }
    moved
}

fn cache_path() -> PathBuf {
    crate::library::library_dir().join("..").join("ambientcg_catalog.json")
}

/// Every ambientCG material (from the disk cache when under a week old).
pub fn catalog() -> Result<Vec<Asset>> {
    let path = cache_path();
    let fresh = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < Duration::from_secs(7 * 24 * 3600));
    if fresh {
        if let Some(c) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Vec<Asset>>(&b).ok()).filter(|c| !c.is_empty()) {
            return Ok(c);
        }
    }
    let mut out = Vec::new();
    for page in 0..40 {
        let url = format!("{API}?type=Material&sort=Popular&include=tagData,displayData,statisticsData&limit=250&offset={}", page * 250);
        let v: serde_json::Value = serde_json::from_reader(ureq::get(&url).timeout(Duration::from_secs(60)).call()?.into_reader())?;
        let assets = v["foundAssets"].as_array().cloned().unwrap_or_default();
        for a in &assets {
            let Some(id) = a["assetId"].as_str() else { continue };
            out.push(Asset {
                id: id.to_string(),
                name: a["displayName"].as_str().unwrap_or(id).to_string(),
                category: a["displayCategory"].as_str().unwrap_or("Other").to_string(),
                tags: a["tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str()).filter(|t| !t.chars().all(|c| c.is_ascii_digit())).map(|t| t.to_lowercase()).collect(),
                popularity: a["popularityScore"].as_f64().unwrap_or(0.0),
            });
        }
        if assets.len() < 250 {
            break;
        }
    }
    anyhow::ensure!(!out.is_empty(), "ambientCG returned no materials");
    let _ = std::fs::write(&path, serde_json::to_vec(&out)?);
    Ok(out)
}

/// The most used tags (words Muse can search with).
pub fn vocabulary(cat: &[Asset], n: usize) -> Vec<String> {
    let mut count: HashMap<&str, usize> = HashMap::new();
    for a in cat {
        for t in &a.tags {
            *count.entry(t.as_str()).or_default() += 1;
        }
    }
    let mut v: Vec<(&str, usize)> = count.into_iter().filter(|(_, c)| *c >= 3).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v.into_iter().take(n).map(|(t, _)| t.to_string()).collect()
}

/// A word without its plural: leaves, leaf; berries, berry; planks, plank.
fn stem(w: &str) -> String {
    let w = w.to_lowercase();
    if let Some(s) = w.strip_suffix("ves").filter(|s| s.len() > 1) {
        format!("{s}f")
    } else if let Some(s) = w.strip_suffix("ies").filter(|s| s.len() > 1) {
        format!("{s}y")
    } else if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") {
        w[..w.len() - 1].to_string()
    } else {
        w
    }
}

/// The material matching `query` (tag words) best: words found in its
/// category, tags or name, then popularity. The `avoid` ids (scans already
/// judged wrong) are never picked.
pub fn best<'a>(cat: &'a [Asset], query: &str, avoid: &[String]) -> Option<&'a Asset> {
    // "-word" leaves out the scans with that tag ("leaf green -ground").
    let (minus, plus): (Vec<&str>, Vec<&str>) = query.split_whitespace().partition(|w| w.starts_with('-'));
    let minus: Vec<String> = minus.iter().map(|w| stem(w.trim_start_matches('-'))).filter(|w| !w.is_empty()).collect();
    let words: Vec<String> = plus.join(" ").to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 1).map(String::from).collect();
    let excluded = |a: &Asset| a.category.split_whitespace().map(stem).chain(a.tags.iter().map(|t| stem(t))).any(|t| minus.contains(&t));
    // The query's material nouns: its words that name a category of the
    // catalog (leaf, bark, planks). When it has one, the scan must be that
    // material, never something else that only shares a color or a size.
    let nouns: std::collections::HashSet<String> = cat.iter().flat_map(|a| a.category.split_whitespace().map(stem).collect::<Vec<_>>()).collect();
    let wanted: Vec<String> = words.iter().map(|w| stem(w)).filter(|w| nouns.contains(w)).collect();
    let is_material = |a: &Asset| a.category.split_whitespace().map(stem).chain(a.tags.iter().map(|t| stem(t))).any(|t| wanted.contains(&t));
    // Single cut-out objects (a leaf, a decal on a background) do not tile
    // as a surface: only when asked for.
    let cutouts = words.iter().any(|w| matches!(w.as_str(), "single" | "cutout" | "decal" | "decals"));
    let cutout = |a: &Asset| a.tags.iter().any(|t| t == "single" || t == "decal") || matches!(a.category.to_lowercase().as_str(), "leaf" | "decal" | "decals");
    // Color and finish words alone never pick a material: "leaf green
    // smooth" must not land on a smooth green concrete.
    const LOOKS: [&str; 34] = [
        "dark", "light", "pale", "bright", "brown", "grey", "gray", "black", "white", "red", "green", "orange", "blue", "yellow", "purple", "pink", "beige",
        "polished", "clean", "dirty", "worn", "old", "new", "smooth", "rough", "weathered", "spotted", "coarse", "fine", "glossy", "shiny", "matte", "wet", "dry",
    ];
    let kind = |a: &Asset| {
        words.iter().filter(|w| !LOOKS.contains(&w.as_str())).any(|w| {
            let singular = w.trim_end_matches('s');
            a.category.to_lowercase() == *w || a.category.to_lowercase() == singular || a.tags.iter().any(|t| t == w || t == singular)
        })
    };
    let score = |a: &Asset| {
        let mut s = 0.0;
        for w in &words {
            let singular = w.trim_end_matches('s');
            if a.category.to_lowercase() == *w || a.category.to_lowercase() == singular {
                s += 3.0;
            }
            if a.tags.iter().any(|t| t == w || t == singular) {
                s += 2.0;
            } else if a.tags.iter().any(|t| t.starts_with(singular) && singular.len() > 3) {
                s += 1.0;
            }
        }
        s + a.popularity.max(0.0).ln_1p() * 0.05
    };
    cat.iter().filter(|a| (cutouts || !cutout(a)) && kind(a) && (wanted.is_empty() || is_material(a)) && !avoid.contains(&a.id) && !excluded(a)).map(|a| (score(a), a)).filter(|(s, _)| *s >= 2.0).max_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, a)| a)
}

/// Downloads `a`'s 2K maps into Muse's cache (once) and returns its folder.
pub fn download(a: &Asset, stop: &AtomicBool) -> Result<PathBuf> {
    let dir = scans_dir().join(&a.category).join(&a.id);
    if dir.join("material.json").is_file() {
        return Ok(dir);
    }
    let mut bytes = Vec::new();
    let mut last = None;
    for res in ["2K-JPG", "1K-JPG"] {
        let url = format!("https://ambientcg.com/get?file={}_{res}.zip", a.id);
        match ureq::get(&url).timeout(Duration::from_secs(300)).call() {
            Ok(r) => {
                r.into_reader().take(200 << 20).read_to_end(&mut bytes)?;
                break;
            }
            Err(e) => last = Some(e),
        }
    }
    if bytes.is_empty() {
        return Err(last.map(anyhow::Error::from).unwrap_or_else(|| anyhow::anyhow!("empty download"))).with_context(|| format!("downloading {}", a.id));
    }
    anyhow::ensure!(!stop.load(Ordering::Relaxed), "Cancelled.");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).with_context(|| format!("{} is not a zip", a.id))?;
    let tmp = dir.with_extension("part");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    // Map suffix in the archive, our file stem.
    const MAPS: [(&str, &str); 5] = [("_Color.", "color"), ("_Roughness.", "roughness"), ("_Metalness.", "metallic"), ("_Displacement.", "height"), ("_NormalGL.", "normal")];
    let mut found = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let name = f.name().to_string();
        if let Some((_, stem)) = MAPS.iter().find(|(suffix, _)| name.contains(suffix)) {
            let ext = name.rsplit('.').next().unwrap_or("jpg").to_lowercase();
            let mut data = Vec::new();
            f.read_to_end(&mut data)?;
            std::fs::write(tmp.join(format!("{stem}.{ext}")), data)?;
            found.push(*stem);
        }
    }
    anyhow::ensure!(found.contains(&"color"), "{} has no color map", a.id);
    let meta = serde_json::json!({
        "name": a.name,
        "category": a.category,
        "source": "ambientCG",
        "id": a.id,
        "url": format!("https://ambientcg.com/view?id={}", a.id),
        "license": "CC0",
        "metal": !found.contains(&"metallic") && a.category == "Metal",
        "tags": a.tags,
    });
    std::fs::write(tmp.join("material.json"), serde_json::to_vec_pretty(&meta)?)?;
    let _ = std::fs::remove_dir_all(&dir);
    if let Some(p) = dir.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::rename(&tmp, &dir)?;
    Ok(dir)
}
