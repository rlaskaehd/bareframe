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
    /// Returns the pane nearest the gap, or `None` when `target` is absent or the only pane.
    fn remove(&mut self, target: PaneId) -> Option<PaneId> {
        let Self::Split { first, second, .. } = self else {
            return None;
        };
        let (survivor, nearest) = if **first == Self::Pane(target) {
            let nearest = second.panes().first().copied();
            (second, nearest)
        } else if **second == Self::Pane(target) {
            let nearest = first.panes().last().copied();
            (first, nearest)
        } else {
            return first.remove(target).or_else(|| second.remove(target));
        };
        *self = std::mem::replace(survivor.as_mut(), Self::Pane(target));
        nearest
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GroupId(pub u64);

/// A named set of adjacent tabs, like a browser tab group.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    /// Index into the UI's group color list.
    pub color: usize,
    pub collapsed: bool,
}

pub const GROUP_COLORS: usize = 6;

#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub layout: Layout,
    pub active_pane: PaneId,
    pub group: Option<GroupId>,
}

impl Tab {
    fn new(pane: PaneId) -> Self {
        Self {
            layout: Layout::Pane(pane),
            active_pane: pane,
            group: None,
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
    /// The window keeps one tab: the last one is replaced by a fresh pane.
    Replaced(PaneId),
}

/// Tabs of one group are always adjacent; every public mutation restores that.
#[derive(Clone, Debug)]
pub struct Workspace {
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    pub groups: Vec<Group>,
    next_pane: u64,
    next_group: u64,
}

impl Workspace {
    pub fn new() -> (Self, PaneId) {
        let pane = PaneId(1);
        let workspace = Self {
            tabs: vec![Tab::new(pane)],
            active_tab: 0,
            groups: Vec::new(),
            next_pane: 2,
            next_group: 1,
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

    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.groups.iter().find(|group| group.id == id)
    }

    /// Opens a tab after the active one, in the same group, and selects it.
    pub fn new_tab(&mut self) -> PaneId {
        let pane = self.allocate();
        let mut tab = Tab::new(pane);
        tab.group = self.active().group;
        self.active_tab += 1;
        self.tabs.insert(self.active_tab, tab);
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
        if let Some(nearest) = tab.layout.remove(pane) {
            if tab.active_pane == pane {
                tab.active_pane = nearest;
            }
            return Some(Closed::Pane);
        }
        if self.tabs.len() == 1 {
            let fresh = self.allocate();
            self.tabs[0] = Tab::new(fresh);
            self.normalize();
            return Some(Closed::Replaced(fresh));
        }
        self.tabs.remove(index);
        if self.active_tab > index || self.active_tab == self.tabs.len() {
            self.active_tab -= 1;
        }
        self.normalize();
        Some(Closed::Tab)
    }

    /// Puts `tab` in a new group and returns it.
    pub fn create_group(&mut self, tab: usize) -> Option<GroupId> {
        self.tabs.get(tab)?;
        let id = GroupId(self.next_group);
        self.next_group += 1;
        self.groups.push(Group {
            id,
            name: format!("Group {}", id.0),
            color: (id.0 as usize - 1) % GROUP_COLORS,
            collapsed: false,
        });
        self.tabs[tab].group = Some(id);
        self.normalize();
        Some(id)
    }

    /// Takes `tab` out of its group and places it right after the group's other tabs.
    pub fn ungroup(&mut self, tab: usize) {
        let Some(group) = self.tabs.get(tab).and_then(|tab| tab.group) else {
            return;
        };
        let end = self
            .tabs
            .iter()
            .rposition(|t| t.group == Some(group))
            .unwrap_or(tab);
        self.move_tab(tab, end + 1, None);
    }

    /// Moves the tab at `from` to sit before index `to` (`to == len` appends) and sets its group.
    pub fn move_tab(&mut self, from: usize, to: usize, group: Option<GroupId>) {
        if from >= self.tabs.len() || to > self.tabs.len() {
            return;
        }
        let active = self.active_pane();
        let mut tab = self.tabs.remove(from);
        tab.group = group.filter(|id| self.group(*id).is_some());
        let to = if from < to { to - 1 } else { to };
        self.tabs.insert(to, tab);
        self.active_tab = self.tab_of(active).unwrap_or(0);
        self.normalize();
    }

    pub fn toggle_group(&mut self, id: GroupId) {
        if let Some(group) = self.groups.iter_mut().find(|group| group.id == id) {
            group.collapsed = !group.collapsed;
        }
    }

    pub fn group_tabs(&self, id: GroupId) -> Vec<usize> {
        (0..self.tabs.len())
            .filter(|&index| self.tabs[index].group == Some(id))
            .collect()
    }

    /// Gathers each group's tabs at its first tab's position and drops empty groups.
    fn normalize(&mut self) {
        let active = self.active_pane();
        let mut remaining: Vec<Option<Tab>> = std::mem::take(&mut self.tabs)
            .into_iter()
            .map(Some)
            .collect();
        for index in 0..remaining.len() {
            let Some(tab) = remaining[index].take() else {
                continue;
            };
            let group = tab.group;
            self.tabs.push(tab);
            if group.is_some() {
                for later in remaining.iter_mut().skip(index + 1) {
                    if later.as_ref().is_some_and(|tab| tab.group == group) {
                        self.tabs.extend(later.take());
                    }
                }
            }
        }
        self.active_tab = self.tab_of(active).unwrap_or(0);
        let tabs = &self.tabs;
        self.groups
            .retain(|group| tabs.iter().any(|tab| tab.group == Some(group.id)));
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
        // Closing the active pane focuses the one that filled its place, not the tab's first.
        let right = ws.split(Axis::Vertical);
        assert_eq!(ws.close(right), Some(Closed::Pane));
        assert_eq!(ws.active_pane(), below);
        assert_eq!(ws.close(below), Some(Closed::Pane));
        assert_eq!(ws.active().layout, Layout::Pane(first));
        assert_eq!(ws.active_pane(), first);
        assert_eq!(ws.close(first), Some(Closed::Replaced(PaneId(5))));
        assert_eq!(ws.active().layout, Layout::Pane(PaneId(5)));
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
    fn groups_stay_contiguous_and_empty_groups_disappear() {
        let (mut ws, a) = Workspace::new();
        let b = ws.new_tab();
        let c = ws.new_tab();
        let group = ws.create_group(0).unwrap();
        // c joins by being dropped onto the group; it moves next to a.
        ws.move_tab(2, 3, Some(group));
        let order = |ws: &Workspace| ws.tabs.iter().map(|t| t.active_pane).collect::<Vec<_>>();
        assert_eq!(order(&ws), [a, c, b]);
        assert_eq!(ws.active_pane(), c);
        // A new tab from a grouped tab joins the group.
        let d = ws.new_tab();
        assert_eq!(ws.active().group, Some(group));
        assert_eq!(ws.group_tabs(group).len(), 3);
        // Ungrouping places the tab after the group's block.
        ws.ungroup(0);
        assert_eq!(order(&ws), [c, d, a, b]);
        assert_eq!(ws.tabs[2].group, None);
        // Closing the last tabs of a group removes the group.
        assert_eq!(ws.close(c), Some(Closed::Tab));
        assert_eq!(ws.close(d), Some(Closed::Tab));
        assert!(ws.groups.is_empty());
        ws.toggle_group(group);
        assert_eq!(order(&ws), [a, b]);
    }

    #[test]
    fn last_tab_is_replaced_instead_of_closing_the_window() {
        let (mut ws, a) = Workspace::new();
        ws.create_group(0);
        let Some(Closed::Replaced(fresh)) = ws.close(a) else {
            panic!("the last tab must be replaced");
        };
        assert_eq!(ws.tabs.len(), 1);
        assert_eq!(ws.active_pane(), fresh);
        assert_eq!(ws.active().group, None);
        assert!(ws.groups.is_empty());
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
