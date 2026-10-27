//! Drag and drop, as in Warp: a pane by its header or tabs from the sidebar, dropped beside
//! another pane (splitting it) or onto the sidebar (reordering tabs, or a pane becoming a
//! tab of its own).
use super::{
    WorkspaceView,
    sidebar::{tab_label, terminal_icon},
};
use crate::{
    theme::{Theme, hsla},
    widgets,
};
use bareframe_core::{
    layout::{Direction, PaneId},
    workspace::{GroupId, TabId},
};
use gpui::{
    Bounds, Context, DragMoveEvent, InteractiveElement, Pixels, Point, Window, div, prelude::*, px,
    relative,
};

/// A pane lifted by its header: a small card with its title follows the cursor.
#[derive(Clone)]
pub(super) struct DraggedPane {
    pub(super) pane: PaneId,
    pub(super) title: String,
    pub(super) theme: &'static Theme,
}

impl Render for DraggedPane {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        widgets::lifted(self.theme)
            .px(px(12.0))
            .py(px(6.0))
            .text_size(px(12.0))
            .child(self.title.clone())
    }
}

/// Tabs being dragged in the sidebar (the selection, or one tab); renders as the whole
/// card following the cursor, as in Warp.
#[derive(Clone)]
pub(super) struct DraggedTab {
    /// Ids, not indices: a shell can exit and close a tab while the drag is under way.
    pub(super) tabs: Vec<TabId>,
    pub(super) title: String,
    pub(super) subtitle: Option<String>,
    pub(super) width: f32,
    pub(super) theme: &'static Theme,
    pub(super) icon: Option<crate::file_icons::FileIcon>,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        // Same padding as a row, so the card stays where it was grabbed.
        div().w(px(self.width)).px(px(6.0)).py(px(3.0)).child(
            widgets::lifted(theme)
                .flex()
                .items_center()
                .gap(px(10.0))
                .px(px(10.0))
                .h(px(46.0))
                .child(terminal_icon(theme, true, None, self.icon))
                .child(tab_label(theme, self.title.clone(), self.subtitle.clone())),
        )
    }
}

/// The side of `bounds` whose edge is nearest `position`.
fn nearest_side(bounds: Bounds<Pixels>, position: Point<Pixels>) -> Direction {
    let x = f32::from(position.x - bounds.left()) / f32::from(bounds.size.width);
    let y = f32::from(position.y - bounds.top()) / f32::from(bounds.size.height);
    [
        (x, Direction::Left),
        (1.0 - x, Direction::Right),
        (y, Direction::Up),
        (1.0 - y, Direction::Down),
    ]
    .into_iter()
    .min_by(|a, b| a.0.total_cmp(&b.0))
    .map_or(Direction::Right, |(_, side)| side)
}

/// The half of a pane a drop would give the dragged pane or tab.
fn drop_zone(side: Direction, theme: &Theme) -> impl IntoElement {
    let zone = div()
        .absolute()
        .rounded_sm()
        .border_2()
        .border_color(hsla(theme.accent).opacity(0.7))
        .bg(hsla(theme.accent).opacity(0.18));
    match side {
        Direction::Left => zone.top_0().bottom_0().left_0().w(relative(0.5)),
        Direction::Right => zone.top_0().bottom_0().right_0().w(relative(0.5)),
        Direction::Up => zone.left_0().right_0().top_0().h(relative(0.5)),
        Direction::Down => zone.left_0().right_0().bottom_0().h(relative(0.5)),
    }
}

impl WorkspaceView {
    /// A drag begins (`tabs` from the sidebar, none for a pane): their rows show an empty
    /// slot, and the terminals freeze so drop zones can be drawn over them.
    pub(super) fn start_drag(&mut self, tabs: Vec<TabId>, cx: &mut Context<Self>) {
        self.dragging = tabs;
        if !self.frozen {
            self.set_frozen(true, cx);
            self.drag_frozen = true;
        }
        cx.notify();
    }

    /// A drag ends, dropped on a target or not.
    pub(super) fn end_drag(&mut self, cx: &mut Context<Self>) {
        self.dragging.clear();
        self.drop_target = None;
        if std::mem::take(&mut self.drag_frozen) {
            self.set_frozen(false, cx);
        }
        cx.notify();
    }

    /// Lets pane `id`'s element take dragged panes and tabs beside it, with the zone a drop
    /// would fill drawn over it; call it last in the element's chain.
    pub(super) fn pane_drop_target(
        &self,
        id: PaneId,
        pane: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let zone = self
            .drop_target
            .filter(|(pane, _)| *pane == id)
            .map(|(_, side)| drop_zone(side, self.theme()));
        pane.on_drag_move(
            cx.listener(move |view, event: &DragMoveEvent<DraggedPane>, _, cx| {
                let allowed = event.drag(cx).pane != id;
                view.hover_drop(id, allowed, event.bounds, event.event.position, cx)
            }),
        )
        .on_drag_move(
            cx.listener(move |view, event: &DragMoveEvent<DraggedTab>, _, cx| {
                let own = view.model.active().id;
                let allowed = !event.drag(cx).tabs.contains(&own);
                view.hover_drop(id, allowed, event.bounds, event.event.position, cx)
            }),
        )
        .on_drop(cx.listener(move |view, dragged: &DraggedPane, window, cx| {
            view.drop_pane(id, dragged.pane, window, cx)
        }))
        .on_drop(cx.listener(move |view, dragged: &DraggedTab, window, cx| {
            view.drop_tabs(id, &dragged.tabs, window, cx)
        }))
        .children(zone)
    }

    /// Lets a sidebar row take dragged tabs, moved before index `to` into `group`, and
    /// panes, each becoming a tab there.
    pub(super) fn tab_drop_target<E: InteractiveElement>(
        &self,
        row: E,
        to: usize,
        group: Option<GroupId>,
        cx: &mut Context<Self>,
    ) -> E {
        row.on_drop(cx.listener(move |view, dragged: &DraggedTab, window, cx| {
            view.drop_tab(dragged, to, group, window, cx)
        }))
        .on_drop(cx.listener(move |view, dragged: &DraggedPane, window, cx| {
            view.drop_pane_tab(dragged, to, group, window, cx)
        }))
    }

    /// Follows the drag over pane `id`: a drop there lands on the side nearest the cursor.
    fn hover_drop(
        &mut self,
        id: PaneId,
        allowed: bool,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let target =
            (allowed && bounds.contains(&position)).then(|| (id, nearest_side(bounds, position)));
        if target.is_some() && self.drop_target != target
            || target.is_none() && self.drop_target.is_some_and(|(pane, _)| pane == id)
        {
            self.drop_target = target;
            cx.notify();
        }
    }

    /// The side of pane `id` a drop lands on, ending the drag either way.
    fn take_drop(&mut self, id: PaneId, cx: &mut Context<Self>) -> Option<Direction> {
        let side = self
            .drop_target
            .filter(|(pane, _)| *pane == id)
            .map(|(_, side)| side);
        self.end_drag(cx);
        side
    }

    fn drop_pane(&mut self, id: PaneId, pane: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(side) = self.take_drop(id, cx) {
            self.model.move_pane(pane, id, side);
        }
        self.sync(window, cx);
    }

    fn drop_tabs(
        &mut self,
        id: PaneId,
        tabs: &[TabId],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(side) = self.take_drop(id, cx) {
            self.model.merge_tabs(tabs, id, side);
        }
        self.sync(window, cx);
    }

    /// Drops `dragged` before the tab at `to`, joining `group`.
    fn drop_tab(
        &mut self,
        dragged: &DraggedTab,
        to: usize,
        group: Option<GroupId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let indices: Vec<usize> = dragged
            .tabs
            .iter()
            .filter_map(|&id| self.model.tab_index(id))
            .collect();
        self.model.move_tabs(&indices, to, group);
        self.end_drag(cx);
        self.sync(window, cx);
    }

    /// A pane dropped on the sidebar leaves its tab for a new one before `to`, in `group`.
    fn drop_pane_tab(
        &mut self,
        dragged: &DraggedPane,
        to: usize,
        group: Option<GroupId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.end_drag(cx);
        self.model.detach_pane(dragged.pane, to, group);
        self.sync(window, cx);
    }
}
