//! Mermaid diagrams for the Markdown viewer, drawn as SVG by merman.
use merman::{OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};

/// The diagram in `source` as SVG, in Mermaid's dark or default theme.
///
/// Labels are plain SVG text: merman's default HTML labels (`foreignObject`) are not drawn
/// by the SVG renderer GPUI uses, so the boxes would come out empty.
pub fn render(source: &str, dark: bool) -> Result<String, String> {
    let theme = if dark { "dark" } else { "default" };
    let source = format!(
        "%%{{init: {{\"theme\": \"{theme}\", \"htmlLabels\": false, \"flowchart\": {{\"htmlLabels\": false}}}}}}%%\n{source}"
    );
    let request = RenderRequest::svg(&source, OperationControl::new(), SvgRequest::default());
    match Renderer::new().render(request) {
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
        assert!(render("stateDiagram-v2\n  [*] --> On : 켜기\n  On --> [*]", false).is_ok());
        assert!(render("not a diagram at all", false).is_err());
    }
}
