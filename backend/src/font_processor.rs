use std::path::Path;

use pbf_font_tools::freetype::{Face, Library};
use pbf_font_tools::prost::Message;
use pbf_font_tools::{render_sdf_glyph, Fontstack, Glyphs};
use thiserror::Error;
use tracing::debug;

const MAX_UNICODE_CP: usize = 0xFFFF;
const CP_RANGE_SIZE: usize = 256;
const FONT_SIZE: usize = 24;
#[allow(clippy::cast_possible_wrap)]
const CHAR_HEIGHT: isize = (FONT_SIZE as isize) << 6;
const BUFFER_SIZE: usize = 3;
const RADIUS: usize = 8;
const CUTOFF: f64 = 0.25_f64;

#[derive(Debug, Error)]
pub enum FontError {
    #[error("FreeType error: {0}")]
    FreeType(#[from] pbf_font_tools::freetype::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Font has no family name")]
    MissingFamilyName,

    #[error("Font has no glyphs")]
    NoGlyphs,

    #[error("Failed to create directory: {0}")]
    DirectoryCreation(String),
}

#[derive(Debug, Clone)]
pub struct FontMetadata {
    pub family: String,
    pub style: Option<String>,
    pub fontstack: String,
    pub glyph_count: usize,
    pub start_cp: usize,
    pub end_cp: usize,
}

pub fn process_font(
    font_path: &Path,
    glyphs_dir: &Path,
) -> Result<(FontMetadata, Vec<(usize, usize)>), FontError> {
    let lib = Library::init()?;
    let mut face = lib.new_face(font_path, 0)?;

    let family = face.family_name().ok_or(FontError::MissingFamilyName)?;
    let style = face.style_name();

    let mut fontstack = family.clone();
    if let Some(ref s) = style {
        fontstack.push(' ');
        fontstack.push_str(s);
    }
    fontstack = fontstack
        .replace(['/', ','], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let (glyph_count, start_cp, end_cp) = count_glyphs(&mut face)?;

    if glyph_count == 0 {
        return Err(FontError::NoGlyphs);
    }

    let metadata = FontMetadata {
        family,
        style,
        fontstack,
        glyph_count,
        start_cp,
        end_cp,
    };

    std::fs::create_dir_all(glyphs_dir).map_err(|e| FontError::DirectoryCreation(e.to_string()))?;

    face.set_char_size(0, CHAR_HEIGHT, 0, 0)?;

    let mut ranges = Vec::new();

    for range_start in (0..=MAX_UNICODE_CP).step_by(CP_RANGE_SIZE) {
        let range_end = (range_start + CP_RANGE_SIZE - 1).min(MAX_UNICODE_CP);

        let pbf_data = generate_pbf_range(&mut face, range_start, range_end, &metadata.fontstack)?;

        if !pbf_data.is_empty() {
            let filename = format!("{}-{}.pbf", range_start, range_end);
            let pbf_path = glyphs_dir.join(&filename);
            std::fs::write(&pbf_path, &pbf_data)?;
            ranges.push((range_start, range_end));
            debug!(
                range = format!("{}-{}", range_start, range_end),
                path = %pbf_path.display(),
                "Generated glyph range"
            );
        }
    }

    Ok((metadata, ranges))
}

fn count_glyphs(face: &mut Face) -> Result<(usize, usize, usize), FontError> {
    let mut count = 0;
    let mut first: Option<usize> = None;
    let mut last = 0;

    for cp in 0..=MAX_UNICODE_CP {
        if face.get_char_index(cp).is_some() {
            count += 1;
            if first.is_none() {
                first = Some(cp);
            }
            last = cp;
        }
    }

    let start = first.unwrap_or(0);
    Ok((count, start, last))
}

fn generate_pbf_range(
    face: &mut Face,
    start_cp: usize,
    end_cp: usize,
    fontstack_name: &str,
) -> Result<Vec<u8>, FontError> {
    let mut stack = Fontstack {
        name: fontstack_name.to_string(),
        range: format!("{}-{}", start_cp, end_cp),
        ..Default::default()
    };

    for cp in start_cp..=end_cp {
        if face.get_char_index(cp).is_some() {
            match render_sdf_glyph(face, cp as u32, BUFFER_SIZE, RADIUS, CUTOFF) {
                Ok(glyph) => {
                    stack.glyphs.push(glyph);
                }
                Err(e) => {
                    debug!(
                        codepoint = cp,
                        error = %e,
                        "Failed to render glyph, skipping"
                    );
                }
            }
        }
    }

    if stack.glyphs.is_empty() {
        return Ok(Vec::new());
    }

    let mut glyphs = Glyphs::default();
    glyphs.stacks.push(stack);
    Ok(glyphs.encode_to_vec())
}
