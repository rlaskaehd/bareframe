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
/// a gray ground, and edge labels on gray chips. Dark first, then light.
const THEME_VARIABLES: [&str; 2] = [
    r##""background": "#262624", "primaryColor": "#1c1b1a", "mainBkg": "#1c1b1a",
    "nodeBkg": "#1c1b1a", "primaryTextColor": "#ece9e1", "textColor": "#ece9e1",
    "nodeTextColor": "#ece9e1", "titleColor": "#ece9e1", "primaryBorderColor": "#bdbab1",
    "nodeBorder": "#bdbab1", "lineColor": "#bdbab1", "secondaryColor": "#30302e",
    "tertiaryColor": "#3a3936", "clusterBkg": "#3a3936", "clusterBorder": "#5e5c57",
    "edgeLabelBackground": "#4a4945""##,
    r##""background": "#faf9f5", "primaryColor": "#ffffff", "mainBkg": "#ffffff",
    "nodeBkg": "#ffffff", "primaryTextColor": "#141413", "textColor": "#141413",
    "nodeTextColor": "#141413", "titleColor": "#141413", "primaryBorderColor": "#5c5b56",
    "nodeBorder": "#5c5b56", "lineColor": "#5c5b56", "secondaryColor": "#f5f4ed",
    "tertiaryColor": "#f0eee6", "clusterBkg": "#f0eee6", "clusterBorder": "#c9c6bd",
    "edgeLabelBackground": "#e8e6dc""##,
];

/// The diagram in `source` as SVG, in the app's dark or light colors.
///
/// Labels keep Mermaid's HTML layout, then merman's resvg-safe pass turns them into plain
/// SVG text: GPUI draws SVG with resvg, which shows no HTML. (merman's own SVG-text labels
/// wrap when measured but not when written, so long labels spilled out of their boxes.)
pub fn render(source: &str, dark: bool) -> Result<String, String> {
    // An init directive is one line.
    let variables = THEME_VARIABLES[usize::from(!dark)].replace('\n', " ");
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

#[cfg(test)]
mod tests {
    use super::render;

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
