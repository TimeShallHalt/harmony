//! Gives the Windows executable its icon: compiles packaging/windows/yapfast.rc
//! with the available resource compiler and links the result. Makes Twemoji's
//! pictures a font for the program to carry. On Linux, lets a plain `cargo build` link
//! rust-skia's own Skia archive beside libwebrtc.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

#[allow(dead_code, reason = "the program builds other fonts from it")]
#[path = "src/emoji_font_tables.rs"]
mod emoji_font_tables;

#[path = "scripts/harmony_build_stamp.rs"]
mod harmony_build_stamp;

fn main() {
    harmony_build_stamp::emit();
    println!("cargo:rerun-if-changed=packaging/windows");
    println!("cargo:rerun-if-env-changed=SKIA_BINARIES_URL");
    println!("cargo:rerun-if-changed=vendor/twemoji/72x72");
    println!("cargo:rerun-if-changed=src/emoji_font_tables.rs");
    if env::var_os("CARGO_FEATURE_DESKTOP").is_some() {
        pack_twemoji();
    }
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => windows_icon(),
        Ok("linux") => linux_skia(),
        _ => {}
    }
}

/// Writes Twemoji's pictures as one font the program carries, and an index that
/// finds a picture by its name: a count, then for each picture where its name
/// starts and how long it is, and where it starts in the font and how long it
/// is, then the names. `src/twemoji.rs` reads both, and the picture's place in
/// the sorted list is its glyph. Each picture gets the transparent margin the font's
/// bitmaps have; `emoji_font_tables::MARGIN` says why.
fn pack_twemoji() {
    let folder = Path::new("vendor/twemoji/72x72");
    let mut pictures: Vec<(String, Vec<u8>)> = fs::read_dir(folder)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", folder.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "png"))
        .map(|path| {
            let name = path
                .file_stem()
                .expect("a picture's name")
                .to_string_lossy()
                .to_ascii_lowercase();
            let bytes = fs::read(&path)
                .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
            (name, bytes)
        })
        .collect();
    assert!(
        !pictures.is_empty(),
        "vendor/twemoji/72x72 holds no pictures"
    );
    // Each picture is a few milliseconds' work, so they share the machine's threads.
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let share = pictures.len().div_ceil(threads);
    std::thread::scope(|scope| {
        for pictures in pictures.chunks_mut(share) {
            scope.spawn(move || {
                for (_, bytes) in pictures {
                    *bytes = with_margin(bytes);
                }
            });
        }
    });
    // Names sort, so the program can look one up by binary search.
    pictures.sort_by(|left, right| left.0.cmp(&right.0));
    let also: Vec<(u32, char)> = pictures
        .iter()
        .enumerate()
        .filter_map(|(index, (name, _))| Some((own_code_point(name)?, glyph_at(index))))
        .collect();
    let names: Vec<String> = pictures.iter().map(|(name, _)| name.clone()).collect();
    let glyphs: Vec<(char, Vec<u8>)> = pictures
        .into_iter()
        .enumerate()
        .map(|(index, (_, bytes))| (glyph_at(index), bytes))
        .collect();
    // Small text is filtered once at build time, not point-sampled from the
    // 72px artwork by DirectWrite. Retain that original first for the picker.
    let small: Vec<_> = [16u8, 20, 32]
        .into_iter()
        .map(|ppem| small_strike(ppem, &glyphs))
        .collect();
    let mut strikes = vec![(&emoji_font_tables::TWEMOJI, glyphs.as_slice())];
    strikes.extend(
        small
            .iter()
            .map(|(strike, glyphs)| (strike, glyphs.as_slice())),
    );
    let font = emoji_font_tables::build_strikes("Yapfast Twemoji", &strikes, &also);
    // An entry is where its name starts, u32, and how long it is, u16, then where
    // its picture starts and how long it is, u32 each.
    const ENTRY: usize = 14;
    let mut name_at = 4 + names.len() * ENTRY;
    let mut index = Vec::new();
    index.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for (name, picture) in names
        .iter()
        .zip(emoji_font_tables::pictures_at(&font, &glyphs))
    {
        index.extend_from_slice(&(name_at as u32).to_le_bytes());
        index.extend_from_slice(&(name.len() as u16).to_le_bytes());
        index.extend_from_slice(&(picture.start as u32).to_le_bytes());
        index.extend_from_slice(&(picture.len() as u32).to_le_bytes());
        name_at += name.len();
    }
    for name in &names {
        index.extend_from_slice(name.as_bytes());
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("twemoji.ttf"), font).expect("writing Twemoji's font");
    fs::write(out.join("twemoji.index"), index).expect("writing Twemoji's index");
}

/// A PNG with the font's margin around it, kept small as Twemoji's are: most have a
/// few dozen colours, so they keep a palette, which takes half the bytes.
fn with_margin(png: &[u8]) -> Vec<u8> {
    let picture = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .expect("a picture of Twemoji's")
        .into_rgba8();
    let margin = emoji_font_tables::TWEMOJI_MARGIN;
    let side = picture.width() + 2 * margin;
    assert_eq!(
        side,
        u32::from(emoji_font_tables::TWEMOJI.side),
        "Twemoji's pictures are 72 pixels square"
    );
    let mut bitmap = image::RgbaImage::new(side, side);
    image::imageops::overlay(&mut bitmap, &picture, i64::from(margin), i64::from(margin));
    // What can't be seen is one colour, the margin's.
    for pixel in bitmap.pixels_mut().filter(|pixel| pixel[3] == 0) {
        pixel.0 = [0; 4];
    }
    let mut colours: Vec<[u8; 4]> = bitmap.pixels().map(|pixel| pixel.0).collect();
    colours.sort_unstable();
    colours.dedup();

    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, side, side);
    encoder.set_compression(png::Compression::High);
    encoder.set_depth(png::BitDepth::Eight);
    let data: Vec<u8> = if colours.len() <= 256 {
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_palette(
            colours
                .iter()
                .flat_map(|colour| colour[..3].to_vec())
                .collect::<Vec<u8>>(),
        );
        encoder.set_trns(colours.iter().map(|colour| colour[3]).collect::<Vec<u8>>());
        encoder.set_filter(png::Filter::NoFilter);
        bitmap
            .pixels()
            .map(|pixel| colours.binary_search(&pixel.0).expect("a colour it holds") as u8)
            .collect()
    } else {
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_filter(png::Filter::Adaptive);
        bitmap.into_raw()
    };
    let mut writer = encoder.write_header().expect("a PNG's header");
    writer.write_image_data(&data).expect("encoding a picture");
    writer.finish().expect("encoding a picture");
    out
}

/// A text-size strike of the same artwork, area-filtered with premultiplied
/// alpha. Resampling straight RGBA darkens its transparent/coloured boundary.
fn small_strike(
    ppem: u8,
    glyphs: &[(char, Vec<u8>)],
) -> (emoji_font_tables::Strike, Vec<(char, Vec<u8>)>) {
    use image::{ImageEncoder, imageops};
    let scaled = |px: f32| (px * f32::from(ppem) / 53.0).round();
    let side = scaled(80.0) as u32;
    let strike = emoji_font_tables::Strike {
        ppem,
        side: side as u8,
        bearing_x: scaled(-3.0) as i8,
        bearing_y: scaled(58.0) as i8,
        advance: scaled(74.0) as u8,
    };
    let glyphs = glyphs
        .iter()
        .map(|(glyph, png)| {
            // Filter the transparent margin too. Cropping it before a resize makes
            // the filter clamp opaque edge pixels, reproducing the flat cut.
            let mut original = image::load_from_memory(png)
                .expect("a glyph PNG")
                .into_rgba8();
            for pixel in original.pixels_mut() {
                for channel in 0..3 {
                    pixel[channel] =
                        ((u32::from(pixel[channel]) * u32::from(pixel[3]) + 127) / 255) as u8;
                }
            }
            let mut filtered =
                imageops::resize(&original, side, side, imageops::FilterType::Triangle);
            for pixel in filtered.pixels_mut() {
                let alpha = u32::from(pixel[3]);
                for channel in 0..3 {
                    pixel[channel] = if alpha == 0 {
                        0
                    } else {
                        ((u32::from(pixel[channel]) * 255 + alpha / 2) / alpha).min(255) as u8
                    };
                }
            }
            let bitmap = filtered;
            let mut png = Vec::new();
            image::codecs::png::PngEncoder::new(&mut png)
                .write_image(
                    bitmap.as_raw(),
                    bitmap.width(),
                    bitmap.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .expect("a filtered glyph PNG");
            (*glyph, png)
        })
        .collect();
    (strike, glyphs)
}

fn glyph_at(index: usize) -> char {
    char::from_u32(emoji_font_tables::TWEMOJI_LAST - index as u32).expect("a code point")
}

/// The code point a picture also draws at, so text that keeps an emoji as typed,
/// such as a composer's, draws it too: an emoji of that one character, which
/// leaves out what only shows as an emoji with a selector after it, and what joins
/// others, as a skin tone does, since this font doesn't join them.
fn own_code_point(name: &str) -> Option<u32> {
    let point = u32::from_str_radix(name, 16).ok()?;
    let text = char::from_u32(point)?.to_string();
    (emojis::get(&text)?.as_str() == text).then_some(point)
}

fn windows_icon() {
    let resources = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("yapfast.res");
    // The script names only the icon beside it, so it needs no preprocessor.
    let status = match Command::new("llvm-rc")
        .current_dir("packaging/windows")
        .args(["-no-preprocess", "-FO"])
        .arg(&resources)
        .arg("yapfast.rc")
        .status()
    {
        Ok(status) => status,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let rc = find_windows_rc().unwrap_or_else(|| {
                panic!("could not find llvm-rc or the Windows SDK's rc.exe; install LLVM for Windows and add its bin directory to PATH")
            });
            Command::new(rc)
                .current_dir("packaging/windows")
                .args(["/fo"])
                .arg(&resources)
                .arg("yapfast.rc")
                .status()
                .unwrap_or_else(|error| panic!("could not run the Windows SDK's rc.exe: {error}"))
        }
        Err(error) => panic!("could not run llvm-rc: {error}"),
    };
    assert!(
        status.success(),
        "resource compiler could not compile yapfast.rc"
    );
    // The MSVC linker takes compiled resources as an input.
    println!("cargo:rustc-link-arg-bins={}", resources.display());
}

fn find_windows_rc() -> Option<PathBuf> {
    if let Some(vswhere) = env::var_os("ProgramFiles(x86)")
        .map(|folder| PathBuf::from(folder).join("Microsoft Visual Studio/Installer/vswhere.exe"))
        && let Ok(output) = Command::new(vswhere)
            .args([
                "-latest",
                "-products",
                "*",
                "-find",
                r"**\Windows Kits\10\bin\*\x64\rc.exe",
            ])
            .output()
        && output.status.success()
        && let Some(path) = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| PathBuf::from(line.trim()))
            .find(|path| path.is_file())
    {
        return Some(path);
    }
    // The SDK is not one of vswhere's products, so look where its installer
    // puts it. `WindowsSdkDir` names the SDK only inside a developer command
    // prompt, which a build run from an ordinary shell does not have.
    let mut roots = Vec::new();
    if let Some(sdk) = env::var_os("WindowsSdkDir") {
        roots.push(PathBuf::from(sdk).join("bin"));
    }
    for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(folder) = env::var_os(variable) {
            roots.push(PathBuf::from(folder).join("Windows Kits/10/bin"));
        }
    }
    let mut versions = roots
        .iter()
        .filter_map(|root| fs::read_dir(root).ok())
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    versions.sort();
    versions.reverse();
    versions
        .into_iter()
        .map(|version| version.join("x64/rc.exe"))
        .find(|path| path.is_file())
}

/// libwebrtc's libjpeg-turbo prefixes its functions with `chromium_`, but not
/// its table `jpeg_natural_order`: Chromium lists only functions to prefix.
/// The libjpeg-turbo in rust-skia's Skia archive defines the same table, so
/// the two clash at the link. The Nix shells and the Arch package set
/// `SKIA_BINARIES_URL` to a copy of the archive with Skia's table renamed
/// (`nix/prepare-skia-native.py`) and link without this flag, so a new clash
/// still fails there. Without it, the linker keeps the first of two
/// definitions; both tables hold the same 320 bytes.
fn linux_skia() {
    if env::var_os("CARGO_FEATURE_DESKTOP").is_none() || env::var_os("SKIA_BINARIES_URL").is_some()
    {
        return;
    }
    // For the tests' executables too, which link Skia as well.
    println!("cargo:rustc-link-arg=-Wl,--allow-multiple-definition");
}
