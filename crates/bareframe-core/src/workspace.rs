//! Tabs and split layout. Pane identity belongs to the workspace, not the terminal backend.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaneId(pub u64);

/// `Horizontal` places children side by side; `Vertical` stacks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    Pane(PaneId),
    Split {
        axis: Axis,
        /// Share of the first child, kept within `MIN_RATIO..=1 - MIN_RATIO`.
        ratio: f32,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}

pub const MIN_RATIO: f32 = 0.1;

/// A pane's area within its tab, in units of the tab's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Layout {
    pub fn panes(&self) -> Vec<PaneId> {
        self.rects().into_iter().map(|(pane, _)| pane).collect()
    }

    pub fn contains(&self, pane: PaneId) -> bool {
        match self {
            Self::Pane(id) => *id == pane,
            Self::Split { first, second, .. } => first.contains(pane) || second.contains(pane),
        }
    }

    pub fn rects(&self) -> Vec<(PaneId, Rect)> {
        let mut out = Vec::new();
        self.collect_rects(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            &mut out,
        );
        out
    }

    fn collect_rects(&self, area: Rect, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Self::Pane(id) => out.push((*id, area)),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let (a, b) = match axis {
                    Axis::Horizontal => {
                        let width = area.width * ratio;
                        (
                            Rect { width, ..area },
                            Rect {
                                x: area.x + width,
                                width: area.width - width,
                                ..area
                            },
                        )
                    }
                    Axis::Vertical => {
                        let height = area.height * ratio;
                        (
                            Rect { height, ..area },
                            Rect {
                                y: area.y + height,
                                height: area.height - height,
                                ..area
                            },
                        )
                    }
                };
                first.collect_rects(a, out);
                second.collect_rects(b, out);
            }
        }
    }

    /// Replaces `target` with a split holding it first and `new` second.
    fn split(&mut self, target: PaneId, axis: Axis, new: PaneId) -> bool {
        match self {
            Self::Pane(id) if *id == target => {
                *self = Self::Split {
                    axis,
                    ratio: 0.5,
                    first: Box::new(Self::Pane(target)),
                    second: Box::new(Self::Pane(new)),
                };
                true
            }
            Self::Pane(_) => false,
            Self::Split { first, second, .. } => {
                first.split(target, axis, new) || second.split(target, axis, new)
            }
        }
    }

    /// Removes `target`; its sibling takes the parent split's place.
    /// Returns false when `target` is absent or is the only pane.
    fn remove(&mut self, target: PaneId) -> bool {
        let Self::Split { first, second, .. } = self else {
            return false;
        };
        let survivor = if **first == Self::Pane(target) {
            second
        } else if **second == Self::Pane(target) {
            first
        } else {
            return first.remove(target) || second.remove(target);
        };
        *self = std::mem::replace(survivor.as_mut(), Self::Pane(target));
        true
    }

    /// Addresses a split by the child choices from the root (`false` = first).
    pub fn set_ratio(&mut self, path: &[bool], value: f32) -> bool {
        match (self, path.split_first()) {
            (Self::Split { ratio, .. }, None) => {
                *ratio = value.clamp(MIN_RATIO, 1.0 - MIN_RATIO);
                true
            }
            (Self::Split { first, second, .. }, Some((&go_second, rest))) => {
                if go_second { second } else { first }.set_ratio(rest, value)
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub layout: Layout,
    pub active_pane: PaneId,
}

impl Tab {
    fn new(pane: PaneId) -> Self {
        Self {
            layout: Layout::Pane(pane),
            active_pane: pane,
        }
    }

    /// The nearest pane in `direction` whose span overlaps the active pane's.
    fn neighbor(&self, direction: Direction) -> Option<PaneId> {
        const EPSILON: f32 = 1e-4;
        let rects = self.layout.rects();
        let (_, from) = *rects.iter().find(|(pane, _)| *pane == self.active_pane)?;
        let overlaps = |a0: f32, a1: f32, b0: f32, b1: f32| a0 < b1 - EPSILON && b0 < a1 - EPSILON;
        rects
            .iter()
            .filter_map(|&(pane, to)| {
                let gap = match direction {
                    Direction::Left => from.x - (to.x + to.width),
                    Direction::Right => to.x - (from.x + from.width),
                    Direction::Up => from.y - (to.y + to.height),
                    Direction::Down => to.y - (from.y + from.height),
                };
                let aligned = match direction {
                    Direction::Left | Direction::Right => {
                        overlaps(from.y, from.y + from.height, to.y, to.y + to.height)
                    }
                    Direction::Up | Direction::Down => {
                        overlaps(from.x, from.x + from.width, to.x, to.x + to.width)
                    }
                };
                (pane != self.active_pane && aligned && gap > -EPSILON).then(|| {
                    let offset = match direction {
                        Direction::Left | Direction::Right => (to.y - from.y).abs(),
                        Direction::Up | Direction::Down => (to.x - from.x).abs(),
                    };
                    (gap, offset, pane)
                })
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)))
            .map(|(_, _, pane)| pane)
    }
}

/// What closing a pane removed beyond the pane itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Closed {
    Pane,
    Tab,
    LastTab,
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    next_pane: u64,
}

impl Workspace {
    pub fn new() -> (Self, PaneId) {
        let pane = PaneId(1);
        let workspace = Self {
            tabs: vec![Tab::new(pane)],
            active_tab: 0,
            next_pane: 2,
        };
        (workspace, pane)
    }

    fn allocate(&mut self) -> PaneId {
        let pane = PaneId(self.next_pane);
        self.next_pane += 1;
        pane
    }

    pub fn active(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    pub fn active_pane(&self) -> PaneId {
        self.active().active_pane
    }

    pub fn tab_of(&self, pane: PaneId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.layout.contains(pane))
    }

    /// Opens a tab after the active one and selects it.
    pub fn new_tab(&mut self) -> PaneId {
        let pane = self.allocate();
        self.active_tab += 1;
        self.tabs.insert(self.active_tab, Tab::new(pane));
        pane
    }

    /// Splits the active pane and focuses the new one.
    pub fn split(&mut self, axis: Axis) -> PaneId {
        let pane = self.allocate();
        let tab = &mut self.tabs[self.active_tab];
        let split = tab.layout.split(tab.active_pane, axis, pane);
        debug_assert!(split, "active pane belongs to the active tab");
        tab.active_pane = pane;
        pane
    }

    pub fn close(&mut self, pane: PaneId) -> Option<Closed> {
        let index = self.tab_of(pane)?;
        let tab = &mut self.tabs[index];
        if tab.layout.remove(pane) {
            if tab.active_pane == pane {
                tab.active_pane = tab.layout.panes()[0];
            }
            return Some(Closed::Pane);
        }
        if self.tabs.len() == 1 {
            return Some(Closed::LastTab);
        }
        self.tabs.remove(index);
        if self.active_tab > index || self.active_tab == self.tabs.len() {
            self.active_tab -= 1;
        }
        Some(Closed::Tab)
    }

    pub fn select_tab(&mut self, index: usize) -> bool {
        let valid = index < self.tabs.len();
        if valid {
            self.active_tab = index;
        }
        valid
    }

    /// Moves to the next (`forward`) or previous tab, wrapping around.
    pub fn cycle_tab(&mut self, forward: bool) {
        let count = self.tabs.len();
        self.active_tab = if forward {
            (self.active_tab + 1) % count
        } else {
            (self.active_tab + count - 1) % count
        };
    }

    pub fn focus(&mut self, pane: PaneId) -> bool {
        let Some(index) = self.tab_of(pane) else {
            return false;
        };
        self.active_tab = index;
        self.tabs[index].active_pane = pane;
        true
    }

    pub fn focus_direction(&mut self, direction: Direction) -> bool {
        let Some(pane) = self.active().neighbor(direction) else {
            return false;
        };
        self.tabs[self.active_tab].active_pane = pane;
        true
    }

    pub fn panes(&self) -> impl Iterator<Item = PaneId> + '_ {
        self.tabs.iter().flat_map(|tab| tab.layout.panes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_close_promotes_sibling_and_keeps_focus_valid() {
        let (mut ws, first) = Workspace::new();
        let right = ws.split(Axis::Horizontal);
        let below = ws.split(Axis::Vertical);
        assert_eq!(ws.active_pane(), below);
        assert_eq!(ws.active().layout.panes(), [first, right, below]);

        assert_eq!(ws.close(right), Some(Closed::Pane));
        assert_eq!(ws.active().layout.panes(), [first, below]);
        assert_eq!(ws.active_pane(), below);
        assert_eq!(ws.close(below), Some(Closed::Pane));
        assert_eq!(ws.active().layout, Layout::Pane(first));
        assert_eq!(ws.active_pane(), first);
        assert_eq!(ws.close(first), Some(Closed::LastTab));
        assert_eq!(ws.close(PaneId(99)), None);
    }

    #[test]
    fn closing_tabs_keeps_the_selection_on_a_neighbour() {
        let (mut ws, a) = Workspace::new();
        let b = ws.new_tab();
        let c = ws.new_tab();
        assert_eq!(ws.active_tab, 2);
        assert!(ws.select_tab(0));
        assert_eq!(ws.close(b), Some(Closed::Tab));
        assert_eq!((ws.active_tab, ws.active_pane()), (0, a));
        ws.cycle_tab(false);
        assert_eq!(ws.active_pane(), c);
        assert_eq!(ws.close(c), Some(Closed::Tab));
        assert_eq!((ws.active_tab, ws.active_pane()), (0, a));
        assert!(!ws.select_tab(3));
    }

    #[test]
    fn directional_focus_follows_geometry() {
        // a | b
        //   |---
        //   | c
        let (mut ws, a) = Workspace::new();
        let b = ws.split(Axis::Horizontal);
        let c = ws.split(Axis::Vertical);
        assert!(ws.focus_direction(Direction::Up));
        assert_eq!(ws.active_pane(), b);
        assert!(!ws.focus_direction(Direction::Up));
        assert!(ws.focus_direction(Direction::Left));
        assert_eq!(ws.active_pane(), a);
        // From the tall left pane, Right picks the vertically closest (top) pane.
        assert!(ws.focus_direction(Direction::Right));
        assert_eq!(ws.active_pane(), b);
        assert!(ws.focus_direction(Direction::Down));
        assert_eq!(ws.active_pane(), c);
        assert!(ws.focus(a));
        assert_eq!(ws.active_pane(), a);
    }

    #[test]
    fn ratios_are_addressed_by_path_and_clamped() {
        let (mut ws, _) = Workspace::new();
        ws.split(Axis::Horizontal);
        ws.split(Axis::Vertical);
        let layout = &mut ws.tabs[0].layout;
        assert!(layout.set_ratio(&[], 0.95));
        assert!(layout.set_ratio(&[true], 0.25));
        assert!(!layout.set_ratio(&[false], 0.5));
        let rects = layout.rects();
        assert!((rects[0].1.width - 0.9).abs() < 1e-6);
        assert!((rects[1].1.height - 0.25).abs() < 1e-6);
        assert!((rects[2].1.y - 0.25).abs() < 1e-6);
    }
}
