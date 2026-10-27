//! Small pieces of UI shared across panels: buttons, tree marks, icons, diff counts and
//! key caps.
use crate::{
    text_field::TextField,
    theme::{self, Theme, hsla},
};
use bareframe_core::keymap::Trigger;
use gpui::{AnyElement, Div, ElementId, Entity, Stateful, div, prelude::*, px};

/// A bordered button; `primary` fills it with the accent. Callers set the padding.
pub fn button(
    theme: &Theme,
    id: impl Into<ElementId>,
    label: &'static str,
    primary: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .rounded_md()
        .border_1()
        .border_color(hsla(if primary { theme.accent } else { theme.border }))
        .when(primary, |this| {
            this.bg(hsla(theme.accent)).text_color(hsla(theme.chrome))
        })
        .hover(|style| style.opacity(0.85))
        .child(label)
}

/// A plain-text button in the muted color that lights up under the pointer. Callers set
/// the padding and corners.
pub fn ghost_button(
    theme: &Theme,
    id: impl Into<ElementId>,
    label: impl Into<gpui::SharedString>,
) -> Stateful<Div> {
    div()
        .id(id)
        .text_color(hsla(theme.text_muted))
        .hover(|style| style.bg(hsla(theme.tab_hover)).text_color(hsla(theme.text)))
        .child(label.into())
}

/// A bordered one-line input showing `hint` while `show_hint`; a click anywhere on it
/// focuses the input. Callers set the height, margins and background.
pub fn text_box(
    theme: &Theme,
    id: impl Into<ElementId>,
    input: &Entity<TextField>,
    hint: &'static str,
    show_hint: bool,
) -> Stateful<Div> {
    let focus = input.clone();
    div()
        .id(id)
        .relative()
        .px_2()
        .flex()
        .items_center()
        .rounded_md()
        .border_1()
        .border_color(hsla(theme.border))
        .overflow_hidden()
        .cursor_text()
        .on_click(move |_, window, cx| focus.update(cx, |input, cx| input.focus(window, cx)))
        .when(show_hint, |this| {
            this.child(
                div()
                    .absolute()
                    .left(px(9.0))
                    .text_color(hsla(theme.text_muted))
                    .child(hint),
            )
        })
        .child(input.clone())
}

/// The card that follows the cursor while something is dragged.
pub fn lifted(theme: &Theme) -> gpui::Div {
    div()
        .rounded_md()
        .bg(hsla(theme.tab_active))
        .border_1()
        .border_color(hsla(theme.accent).opacity(0.6))
        .shadow_lg()
        .opacity(0.92)
        .font_family(theme::UI_FONT)
        .text_color(hsla(theme.text))
}

/// A small square button holding a glyph, as the × and ⋮ on tabs and pane headers.
pub fn icon_button(theme: &Theme, id: impl Into<ElementId>, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(20.0))
        .rounded_sm()
        .text_color(hsla(theme.text_muted))
        .hover(|style| style.bg(hsla(theme.border)).text_color(hsla(theme.text)))
        .child(label)
}

/// A tree row's fold mark: down when `open`.
pub fn chevron(theme: &Theme, open: bool) -> impl IntoElement {
    gpui::svg()
        .path(if open {
            "icons/chevron-down.svg"
        } else {
            "icons/chevron-right.svg"
        })
        .flex_none()
        .size(px(12.0))
        .text_color(hsla(theme.text_muted))
}

/// A file's type icon: a language logo in its color, else the plain icon in the muted color.
pub fn file_icon(theme: &Theme, icon: crate::file_icons::FileIcon) -> impl IntoElement {
    gpui::svg()
        .path(icon.path)
        .flex_none()
        .size(px(14.0))
        .text_color(hsla(icon.color.unwrap_or(theme.text_muted)))
}

/// One of the bundled icons, in the muted text color.
pub fn icon(theme: &Theme, path: &'static str) -> impl IntoElement {
    gpui::svg()
        .path(path)
        .flex_none()
        .size(px(14.0))
        .text_color(hsla(theme.text_muted))
}

/// `+N −M` in the diff colors.
pub fn counts(theme: &Theme, added: u64, removed: u64) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .gap_1()
        .text_size(px(12.0))
        .child(
            div()
                .text_color(hsla(theme.palette[2]))
                .child(format!("+{added}")),
        )
        .child(
            div()
                .text_color(hsla(theme.palette[1]))
                .child(format!("−{removed}")),
        )
}

/// Key caps as Warp draws them, outlined in orange when the shortcut is taken.
pub fn caps(theme: &Theme, trigger: &Trigger, conflict: bool) -> AnyElement {
    div()
        .flex()
        .gap_1()
        .children(trigger.caps().into_iter().map(|cap| {
            div()
                .min_w(px(22.0))
                .h(px(22.0))
                .px_1()
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .border_1()
                .border_color(hsla(if conflict {
                    theme.group_colors[3]
                } else {
                    theme.border
                }))
                .bg(hsla(theme.terminal_background))
                // Explicit, so caps stay readable on the highlighted row.
                .text_color(hsla(theme.text))
                .text_size(px(12.0))
                .child(cap)
        }))
        .into_any_element()
}
