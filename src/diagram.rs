//! Mermaid diagrams for the Markdown viewer, drawn as SVG by merman.
use merman::{
    OperationControl, RenderOutput, RenderRequest, RenderTarget, Renderer, SvgEnvironment,
    SvgRequest,
    svg::{SvgPipeline, export::NativeFontContext, export::NativeFontOptions},
};
use std::{path::PathBuf, sync::OnceLock};

/// A system font with both Latin and Hangul. Labels are measured with it and drawn in it,
/// so a box fits its text; measuring with stand-in metrics left Korean labels overflowing.
const FONT_FILE: &str = "/System/Library/Fonts/AppleSDGothicNeo.ttc";
const FONT_FAMILY: &str = "Apple SD Gothic Neo";

/// The weights Mermaid's styles ask for: normal, 600 and bold.
const WEIGHTS: [u16; 3] = [400, 600, 700];

/// Loaded once: reading the font file takes tens of milliseconds. merman keeps every byte
/// of the files it is given, and the collection holds eighteen faces in 55MB, so it is
/// given only `WEIGHTS`, each cut out to a file of its own (about 3MB) and removed once
/// read. The whole collection if that fails.
fn fonts() -> Option<&'static NativeFontContext> {
    static FONTS: OnceLock<Option<NativeFontContext>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let faces = std::fs::read(FONT_FILE)
                .map(|collection| write_faces(&collection))
                .unwrap_or_default();
            let mut options = NativeFontOptions::default();
            if faces.is_empty() {
                options = options.with_font_file(FONT_FILE);
            }
            for path in &faces {
                options = options.with_font_file(path);
            }
            let fonts = options
                .load(&OperationControl::new())
                .inspect_err(|error| eprintln!("bareframe: diagram font {FONT_FILE}: {error}"))
                .ok();
            for path in &faces {
                let _ = std::fs::remove_file(path);
            }
            fonts
        })
        .as_ref()
}

/// The `FONT_FAMILY` faces of `collection` in `WEIGHTS`, each written to the temporary
/// directory as a font of its own.
fn write_faces(collection: &[u8]) -> Vec<PathBuf> {
    let count = ttf_parser::fonts_in_collection(collection).unwrap_or(0);
    (0..count)
        .filter_map(|index| {
            let face = ttf_parser::Face::parse(collection, index).ok()?;
            let weight = face.weight().to_number();
            // Its names are Mac Roman only, which is ASCII here.
            let family = face.names().into_iter().any(|name| {
                name.name_id == ttf_parser::name_id::FAMILY && name.name == FONT_FAMILY.as_bytes()
            });
            if !family || !WEIGHTS.contains(&weight) {
                return None;
            }
            let path = std::env::temp_dir().join(format!(
                "bareframe-diagram-font-{}-{weight}.ttf",
                std::process::id()
            ));
            std::fs::write(&path, face_file(collection, index as usize)?).ok()?;
            Some(path)
        })
        .collect()
}

/// Face `index` of a font collection as a font file: its table directory, then its tables
/// (offsets in a collection count from the start of the whole file).
fn face_file(collection: &[u8], index: usize) -> Option<Vec<u8>> {
    let number = |at: usize, size: usize| {
        let bytes = collection.get(at..at + size)?;
        Some(
            bytes
                .iter()
                .fold(0, |value, &byte| value << 8 | usize::from(byte)),
        )
    };
    let start = number(12 + 4 * index, 4)?;
    let tables = number(start + 4, 2)?;
    let mut font = collection.get(start..start + 12 + 16 * tables)?.to_vec();
    for table in 0..tables {
        let record = 12 + 16 * table;
        let offset = number(start + record + 8, 4)?;
        let length = number(start + record + 12, 4)?;
        let moved = u32::try_from(font.len()).ok()?;
        font[record + 8..record + 12].copy_from_slice(&moved.to_be_bytes());
        font.extend_from_slice(collection.get(offset..offset + length)?);
        font.resize(font.len().next_multiple_of(4), 0);
    }
    Some(font)
}

/// Mermaid's plain `base` theme in the app's colors, set out the way VS Code and cmux draw
/// diagrams: boxes filled apart from the page with a clear light (or dark) border, groups on
/// a gray ground, and edge labels on gray chips; ER rows striped in two quiet shades; pie
/// slices, mind map and timeline sections and git branches in the app's accent colors with
/// dark labels. Dark first, then light.
const THEME_VARIABLES: [&str; 2] = [
    r##""background": "#262624", "primaryColor": "#1c1b1a", "mainBkg": "#1c1b1a",
    "nodeBkg": "#1c1b1a", "primaryTextColor": "#ece9e1", "textColor": "#ece9e1",
    "nodeTextColor": "#ece9e1", "titleColor": "#ece9e1", "primaryBorderColor": "#bdbab1",
    "nodeBorder": "#bdbab1", "lineColor": "#bdbab1", "secondaryColor": "#30302e",
    "tertiaryColor": "#3a3936", "clusterBkg": "#3a3936", "clusterBorder": "#5e5c57",
    "edgeLabelBackground": "#4a4945",
    "rowOdd": "#2c2b29", "rowEven": "#1c1b1a""##,
    r##""background": "#faf9f5", "primaryColor": "#ffffff", "mainBkg": "#ffffff",
    "nodeBkg": "#ffffff", "primaryTextColor": "#141413", "textColor": "#141413",
    "nodeTextColor": "#141413", "titleColor": "#141413", "primaryBorderColor": "#5c5b56",
    "nodeBorder": "#5c5b56", "lineColor": "#5c5b56", "secondaryColor": "#f5f4ed",
    "tertiaryColor": "#f0eee6", "clusterBkg": "#f0eee6", "clusterBorder": "#c9c6bd",
    "edgeLabelBackground": "#e8e6dc",
    "rowOdd": "#ffffff", "rowEven": "#f5f4ed""##,
];

/// Series colors shared by both themes: pie slices, mind map and timeline sections and git
/// branches in the app's accents, each labelled in dark text.
const SERIES: [&str; 8] = [
    "#d97757", "#6a9bcc", "#8fa36f", "#d4a27f", "#c77d9a", "#6fb0aa", "#e0b765", "#9a8fd1",
];

fn series_variables() -> String {
    let mut variables = String::from(
        r##""pieSectionTextColor": "#1c1b1a", "pieStrokeColor": "#1c1b1a", "pieOuterStrokeColor": "#5e5c57""##,
    );
    for (index, color) in SERIES.iter().enumerate() {
        let pie = index + 1;
        variables.push_str(&format!(
            r##", "pie{pie}": "{color}", "cScale{index}": "{color}", "cScaleLabel{index}": "#1c1b1a", "git{index}": "{color}", "gitBranchLabel{index}": "#1c1b1a""##
        ));
    }
    variables
}

/// The diagram in `source` as SVG, in the app's dark or light colors.
///
/// Labels keep Mermaid's HTML layout, then merman's resvg-safe pass turns them into plain
/// SVG text: GPUI draws SVG with resvg, which shows no HTML. (merman's own SVG-text labels
/// wrap when measured but not when written, so long labels spilled out of their boxes.)
pub fn render(source: &str, dark: bool) -> Result<String, String> {
    // An init directive is one line.
    let variables = format!(
        "{}, {}",
        THEME_VARIABLES[usize::from(!dark)].replace('\n', " "),
        series_variables()
    );
    let mut environment = SvgEnvironment::deterministic();
    let mut family = String::new();
    if let Some(fonts) = fonts()
        && let Ok(native) = SvgEnvironment::deterministic().with_native_font_context(fonts.clone())
    {
        environment = native;
        family = format!(", \"fontFamily\": \"{FONT_FAMILY}\"");
    }
    // ELK gives a state's self-loop its own lane, where dagre laid its label over a
    // neighbouring edge's; for flowcharts ELK crowds labels, so they keep dagre.
    let layout = if source.trim_start().starts_with("stateDiagram") {
        "elk"
    } else {
        "dagre"
    };
    let source = format!(
        "%%{{init: {{\"theme\": \"base\", \"themeVariables\": {{{variables}}}, \"layout\": \"{layout}\"{family}}}}}%%\n{source}"
    );
    let request = SvgRequest {
        environment,
        pipeline: Some(SvgPipeline::resvg_safe()),
        ..Default::default()
    };
    let target = RenderTarget::Svg(request);
    match Renderer::new().render(RenderRequest::new(&source, target, OperationControl::new())) {
        Ok(RenderOutput::Svg(Some(svg))) => Ok(svg.svg().to_owned()),
        Ok(_) => Err("not a Mermaid diagram".into()),
        Err(error) => Err(error.to_string()),
    }
}

/// `svg` as drawn by `render`, showing only `part` of it (left, top, width and height as
/// fractions of the whole) and `size` points across: drawn at the screen's resolution, it
/// is as sharp as a browser's SVG at any zoom.
pub fn crop(svg: &str, part: [f32; 4], size: (f32, f32)) -> Option<String> {
    let (start, end) = root(svg)?;
    let mut root = svg[start..end].to_owned();
    let [x, y, width, height] = view_box(&root)?;
    // `width="100%"` and a `max-width` style would size the picture by its box.
    for name in ["width", "height", "viewBox", "style"] {
        if let Some(value) = attribute(&root, name) {
            root = root.replacen(&format!(" {name}=\"{value}\""), "", 1);
        }
    }
    let [left, top, across, down] = part;
    let view_box = format!(
        "{} {} {} {}",
        x + left * width,
        y + top * height,
        across * width,
        down * height
    );
    Some(format!(
        "{root} width=\"{}\" height=\"{}\" viewBox=\"{view_box}\"{}",
        size.0,
        size.1,
        &svg[end..]
    ))
}

/// How wide `svg` (as drawn by `render`) is at its own size, in points: its `viewBox`'s.
pub fn width(svg: &str) -> Option<f32> {
    let (start, end) = root(svg)?;
    Some(view_box(&svg[start..end])?[2])
}

/// Where the root `<svg …` start tag is, its closing `>` left out.
fn root(svg: &str) -> Option<(usize, usize)> {
    let start = svg.find("<svg")?;
    Some((start, start + svg[start..].find('>')?))
}

/// The left, top, width and height of a start tag's `viewBox`.
fn view_box(tag: &str) -> Option<[f32; 4]> {
    let numbers: Vec<f32> = attribute(tag, "viewBox")?
        .split([' ', ','])
        .filter(|word| !word.is_empty())
        .map(str::parse::<f32>)
        .collect::<Result<_, _>>()
        .ok()?;
    numbers.try_into().ok()
}

/// The value of the attribute `name` in a start tag (a space before it, so `stroke-width`
/// is not `width`).
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(&tag[at..at + tag[at..].find('"')?])
}

#[cfg(test)]
mod tests {
    use super::{FONT_FILE, crop, fonts, render, width};

    #[test]
    fn diagrams_keep_only_the_weights_they_use() {
        if !std::path::Path::new(FONT_FILE).exists() {
            return;
        }
        let fonts = fonts().expect("diagram font");
        assert_eq!(fonts.face_count(), 3);
        assert!(
            fonts.resident_font_bytes() < 12 << 20,
            "not the whole 55MB collection"
        );
    }

    #[test]
    fn a_crop_shows_that_part_at_that_size() {
        assert_eq!(
            width(r#"<svg width="100%" viewBox="-8 -8 1345.5 973"><g/></svg>"#),
            Some(1345.5)
        );
        assert_eq!(
            width(r#"<svg width="100%"><g viewBox="0 0 9 9"/></svg>"#),
            None
        );
        let svg = r#"<svg id="m" width="100%" style="max-width:420px" viewBox="0 0 420 200"><rect stroke-width="2"/></svg>"#;
        assert_eq!(
            crop(svg, [0.5, 0.25, 0.25, 0.5], (300.0, 400.0)).as_deref(),
            Some(
                r#"<svg id="m" width="300" height="400" viewBox="210 50 105 100"><rect stroke-width="2"/></svg>"#
            )
        );
        assert_eq!(
            crop("<svg id=\"m\"></svg>", [0.0, 0.0, 1.0, 1.0], (1.0, 1.0)),
            None
        );
    }

    #[test]
    fn diagrams_render_with_plain_text_labels() {
        let svg = render(
            "flowchart LR\n  A[시작] --> B & C\n  subgraph S[묶음]\n  B\n  end",
            true,
        )
        .expect("flowchart");
        assert!(svg.contains("<svg") && svg.contains("시작"));
        assert!(!svg.contains("<foreignObject"), "labels must be SVG text");
        assert!(svg.contains("#1c1b1a"), "the app's dark colors");
        let state = render(
            "stateDiagram-v2\n  [*] --> On : 광고주 요청(캠페인이 SUSPENDED일 때만)\n  On --> [*]",
            false,
        )
        .expect("state");
        assert!(!state.contains("<foreignObject"));
        assert!(render("not a diagram at all", false).is_err());
    }
}
