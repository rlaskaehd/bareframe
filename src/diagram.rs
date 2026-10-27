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

/// The diagram in `source` as SVG, in Mermaid's dark or neutral theme.
///
/// Labels keep Mermaid's HTML layout, then merman's resvg-safe pass turns them into plain
/// SVG text: GPUI draws SVG with resvg, which shows no HTML. (merman's own SVG-text labels
/// wrap when measured but not when written, so long labels spilled out of their boxes.)
pub fn render(source: &str, dark: bool) -> Result<String, String> {
    // Mermaid's gray "neutral" sits with the ivory light theme better than its purple default.
    let theme = if dark { "dark" } else { "neutral" };
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
        "%%{{init: {{\"theme\": \"{theme}\", \"layout\": \"{layout}\"{family}}}}}%%\n{source}"
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
        let state = render(
            "stateDiagram-v2\n  [*] --> On : 광고주 요청(캠페인이 SUSPENDED일 때만)\n  On --> [*]",
            false,
        )
        .expect("state");
        assert!(!state.contains("<foreignObject"));
        assert!(render("not a diagram at all", false).is_err());
    }
}
