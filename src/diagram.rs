//! Mermaid diagrams for the Markdown viewer, drawn as SVG by merman.
use merman::{
    OperationControl, RenderOutput, RenderRequest, RenderTarget, Renderer, SvgEnvironment,
    SvgRequest,
    svg::{SvgPipeline, export::NativeFontContext, export::NativeFontOptions},
};
use std::sync::OnceLock;

/// A system font with both Latin and Hangul. Labels are measured with it and drawn in it,
/// so a box fits its text; measuring with stand-in metrics left Korean labels overflowing.
const FONT_FILE: &str = "/System/Library/Fonts/AppleSDGothicNeo.ttc";
const FONT_FAMILY: &str = "Apple SD Gothic Neo";

/// Loaded once: reading the font file takes tens of milliseconds.
fn fonts() -> Option<&'static NativeFontContext> {
    static FONTS: OnceLock<Option<NativeFontContext>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            NativeFontOptions::default()
                .with_font_file(FONT_FILE)
                .load(&OperationControl::new())
                .inspect_err(|error| eprintln!("bareframe: diagram font {FONT_FILE}: {error}"))
                .ok()
        })
        .as_ref()
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
    let start = svg.find("<svg")?;
    let end = start + svg[start..].find('>')?;
    let mut root = svg[start..end].to_owned();
    let [x, y, width, height] = attribute(&root, "viewBox")?
        .split([' ', ','])
        .filter(|word| !word.is_empty())
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?[..]
    else {
        return None;
    };
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

/// The value of the attribute `name` in a start tag (a space before it, so `stroke-width`
/// is not `width`).
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(&tag[at..at + tag[at..].find('"')?])
}

#[cfg(test)]
mod tests {
    use super::{crop, render};

    #[test]
    fn a_crop_shows_that_part_at_that_size() {
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
