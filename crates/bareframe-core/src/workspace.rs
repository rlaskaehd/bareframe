//! Tabs, groups and the selection; each tab holds a split layout of panes. Pane identity
//! belongs to the workspace, not the terminal backend.

use crate::layout::{Axis, Direction, Layout, PaneId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GroupId(pub u64);

/// Stable tab identity; indices shift whenever tabs move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TabId(pub u64);

/// A named set of adjacent tabs, like a browser tab group.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub id: GroupId,
    /// Unnamed groups are shown as "New Group", as in Warp.
    pub name: Option<String>,
    /// Index into the UI's group color list.
    pub color: usize,
    pub collapsed: bool,
    /// A pinned group sits with the pinned tabs at the top of the list.
    pub pinned: bool,
}

pub const GROUP_COLORS: usize = 6;

#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub id: TabId,
    pub layout: Layout,
    pub active_pane: PaneId,
    pub group: Option<GroupId>,
    /// Part of the sidebar's multi-selection (shift-click range or cmd-click).
    pub selected: bool,
    /// A user-chosen title that replaces the shell's.
    pub title: Option<String>,
    /// Only meaningful outside a group; a grouped tab follows its group's pin.
    pub pinned: bool,
    /// The active pane fills the tab; any layout change ends it, as in Warp.
    pub maximized: bool,
    /// Index into the UI's color list; tints the tab's sidebar row.
    pub color: Option<usize>,
}

impl Tab {
    /// Takes `pane` out of this tab's splits, un-maximizing and focusing its neighbour if it
    /// had focus; `false` when it is the tab's only pane.
    fn remove_pane(&mut self, pane: PaneId) -> bool {
        let Some(nearest) = self.layout.remove(pane) else {
            return false;
        };
        self.maximized = false;
        if self.active_pane == pane {
            self.active_pane = nearest;
        }
        true
    }

    /// A saved tab with default flags; the caller fills in the rest.
    pub(crate) fn restored(id: TabId, layout: Layout, active_pane: PaneId) -> Self {
        Self {
            layout,
            active_pane,
            ..Self::new(id, active_pane)
        }
    }

    fn new(id: TabId, pane: PaneId) -> Self {
        Self {
            id,
            layout: Layout::Pane(pane),
            active_pane: pane,
            group: None,
            selected: false,
            title: None,
            pinned: false,
            maximized: false,
            color: None,
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
#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    pub groups: Vec<Group>,
    /// Panes sharing one place in a layout as tabs within it, as in cmux: the layout holds
    /// the one shown and the rest wait behind it. Two or more panes each, in tab order; a
    /// place's stack goes with it wherever the layout moves it.
    pub stacks: Vec<Vec<PaneId>>,
    next_pane: u64,
    next_group: u64,
    next_tab: u64,
}

impl Workspace {
    pub fn new() -> (Self, PaneId) {
        let pane = PaneId(1);
        let workspace = Self {
            tabs: vec![Tab::new(TabId(1), pane)],
            active_tab: 0,
            groups: Vec::new(),
            stacks: Vec::new(),
            next_pane: 2,
            next_group: 1,
            next_tab: 2,
        };
        (workspace, pane)
    }

    /// Rebuilds a saved workspace. `None` if it is inconsistent: no tabs, a pane used twice,
    /// an active pane outside its tab, a tab in a missing group, or a stack not shown in
    /// exactly one place.
    pub fn from_parts(
        tabs: Vec<Tab>,
        groups: Vec<Group>,
        active_tab: usize,
        stacks: Vec<Vec<PaneId>>,
    ) -> Option<Self> {
        let shown: Vec<PaneId> = tabs.iter().flat_map(|tab| tab.layout.panes()).collect();
        let hidden = stacks
            .iter()
            .flat_map(|stack| stack.iter().filter(|pane| !shown.contains(pane)));
        let panes: Vec<PaneId> = shown.iter().copied().chain(hidden.copied()).collect();
        let mut unique = panes.clone();
        unique.sort();
        unique.dedup();
        let mut members: Vec<PaneId> = stacks.iter().flatten().copied().collect();
        let stacked = members.len();
        members.sort();
        members.dedup();
        let consistent = !tabs.is_empty()
            && unique.len() == panes.len()
            && members.len() == stacked
            && unique.len() + stacks.len() == shown.len() + stacked
            && stacks.iter().all(|stack| {
                stack.len() > 1 && stack.iter().filter(|pane| shown.contains(pane)).count() == 1
            })
            && groups.iter().all(|group| group.color < GROUP_COLORS)
            && tabs.iter().all(|tab| {
                tab.layout.contains(tab.active_pane)
                    && tab.color.is_none_or(|color| color < GROUP_COLORS)
                    && tab
                        .group
                        .is_none_or(|id| groups.iter().any(|group| group.id == id))
            });
        if !consistent {
            return None;
        }
        let next = |ids: &mut dyn Iterator<Item = u64>| ids.max().unwrap_or(0) + 1;
        let mut workspace = Self {
            next_pane: next(&mut panes.iter().map(|pane| pane.0)),
            next_group: next(&mut groups.iter().map(|group| group.id.0)),
            next_tab: next(&mut tabs.iter().map(|tab| tab.id.0)),
            active_tab: active_tab.min(tabs.len() - 1),
            tabs,
            groups,
            stacks,
        };
        workspace.normalize();
        Some(workspace)
    }

    fn allocate(&mut self) -> PaneId {
        let pane = PaneId(self.next_pane);
        self.next_pane += 1;
        pane
    }

    fn fresh_tab(&mut self, pane: PaneId) -> Tab {
        let id = TabId(self.next_tab);
        self.next_tab += 1;
        Tab::new(id, pane)
    }

    pub fn tab_index(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    fn is_pinned(&self, tab: &Tab) -> bool {
        match tab.group {
            Some(group) => self.group(group).is_some_and(|group| group.pinned),
            None => tab.pinned,
        }
    }

    pub fn tab_pinned(&self, index: usize) -> bool {
        self.tabs.get(index).is_some_and(|tab| self.is_pinned(tab))
    }

    /// An empty title clears the custom title.
    pub fn rename_tab(&mut self, id: TabId, title: &str) {
        if let Some(index) = self.tab_index(id) {
            let title = title.trim();
            self.tabs[index].title = (!title.is_empty()).then(|| title.to_owned());
        }
    }

    pub fn rename_group(&mut self, id: GroupId, name: &str) {
        if let Some(group) = self.group_mut(id) {
            let name = name.trim();
            group.name = (!name.is_empty()).then(|| name.to_owned());
        }
    }

    /// Pinning an ungrouped tab; a grouped tab is pinned through its group.
    pub fn set_tab_pinned(&mut self, index: usize, pinned: bool) {
        if let Some(tab) = self.tabs.get_mut(index).filter(|tab| tab.group.is_none()) {
            tab.pinned = pinned;
            self.normalize();
        }
    }

    pub fn set_tab_color(&mut self, index: usize, color: Option<usize>) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.color = color.filter(|&color| color < GROUP_COLORS);
        }
    }

    /// The neighbour a tab can swap with: same group and same pinned region.
    pub fn can_move_tab(&self, index: usize, up: bool) -> bool {
        let other = if up {
            index.checked_sub(1)
        } else {
            index.checked_add(1)
        };
        let Some(other) = other else {
            return false;
        };
        match (self.tabs.get(index), self.tabs.get(other)) {
            (Some(tab), Some(neighbour)) => {
                tab.group == neighbour.group && self.tab_pinned(index) == self.tab_pinned(other)
            }
            _ => false,
        }
    }

    pub fn move_tab(&mut self, index: usize, up: bool) {
        if self.can_move_tab(index, up) {
            let group = self.tabs[index].group;
            self.move_tabs(&[index], if up { index - 1 } else { index + 2 }, group);
        }
    }

    pub fn set_group_pinned(&mut self, id: GroupId, pinned: bool) {
        if let Some(group) = self.group_mut(id) {
            group.pinned = pinned;
            self.normalize();
        }
    }

    /// Swaps a group with the neighbouring tab or group, staying within its pin region.
    pub fn move_group(&mut self, id: GroupId, up: bool) {
        let active = self.active_pane();
        let mut units: Vec<Vec<Tab>> = Vec::new();
        for tab in std::mem::take(&mut self.tabs) {
            match units.last_mut() {
                Some(unit) if tab.group.is_some() && unit[0].group == tab.group => unit.push(tab),
                _ => units.push(vec![tab]),
            }
        }
        if let Some(at) = units.iter().position(|unit| unit[0].group == Some(id)) {
            let other = if up { at.checked_sub(1) } else { Some(at + 1) };
            if let Some(other) = other.filter(|&other| other < units.len())
                && self.is_pinned(&units[other][0]) == self.is_pinned(&units[at][0])
            {
                units.swap(at, other);
            }
        }
        self.tabs = units.into_iter().flatten().collect();
        self.active_tab = self.tab_of(active).unwrap_or(0);
    }

    pub fn active(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    pub fn active_pane(&self) -> PaneId {
        self.active().active_pane
    }

    /// The tab holding `pane`, shown or waiting in its place's stack.
    pub fn tab_of(&self, pane: PaneId) -> Option<usize> {
        let shown = self.shown(pane);
        self.tabs.iter().position(|tab| tab.layout.contains(shown))
    }

    /// The panes sharing `pane`'s place, in tab order, if it has more than one.
    pub fn stack(&self, pane: PaneId) -> Option<&[PaneId]> {
        self.stacks
            .iter()
            .find(|stack| stack.contains(&pane))
            .map(Vec::as_slice)
    }

    /// The pane shown in `pane`'s place: `pane` itself unless it waits in a stack.
    fn shown(&self, pane: PaneId) -> PaneId {
        self.stack(pane)
            .and_then(|stack| {
                stack
                    .iter()
                    .copied()
                    .find(|member| self.tabs.iter().any(|tab| tab.layout.contains(*member)))
            })
            .unwrap_or(pane)
    }

    /// Every pane of tab `index`, those waiting in stacks included, in layout and tab order.
    pub fn tab_panes(&self, index: usize) -> Vec<PaneId> {
        self.tabs.get(index).map_or_else(Vec::new, |tab| {
            tab.layout
                .panes()
                .into_iter()
                .flat_map(|pane| self.stack(pane).map_or_else(|| vec![pane], <[_]>::to_vec))
                .collect()
        })
    }

    /// A new pane in the active pane's place, as a tab after it, shown and focused.
    pub fn add_pane_tab(&mut self) -> PaneId {
        let pane = self.allocate();
        let shown = self.active_pane();
        match self.stacks.iter_mut().find(|stack| stack.contains(&shown)) {
            Some(stack) => {
                let at = stack
                    .iter()
                    .position(|member| *member == shown)
                    .unwrap_or(0);
                stack.insert(at + 1, pane);
            }
            None => self.stacks.push(vec![shown, pane]),
        }
        self.focus(pane);
        pane
    }

    /// Shows the next or previous tab of the active pane's place, wrapping.
    pub fn cycle_pane_tab(&mut self, forward: bool) -> bool {
        let shown = self.active_pane();
        let Some(stack) = self.stack(shown) else {
            return false;
        };
        let at = stack
            .iter()
            .position(|member| *member == shown)
            .unwrap_or(0);
        let next = if forward {
            stack[(at + 1) % stack.len()]
        } else {
            stack[(at + stack.len() - 1) % stack.len()]
        };
        self.focus(next)
    }

    /// Drag and drop onto a pane's tabs: `pane`, with any tabs of its place, joins
    /// `target`'s place after it, shown and focused. A tab left without panes goes.
    pub fn stack_onto(&mut self, pane: PaneId, target: PaneId) -> bool {
        let (Some(from), Some(_)) = (self.tab_of(pane), self.tab_of(target)) else {
            return false;
        };
        let moving = self.stack(pane).map_or_else(|| vec![pane], <[_]>::to_vec);
        if moving.contains(&target) {
            return false;
        }
        let place = self.shown(pane);
        if !self.tabs[from].remove_pane(place) {
            self.take(&[from]);
        }
        self.stacks.retain(|stack| !stack.contains(&pane));
        let index = match self.stacks.iter().position(|stack| stack.contains(&target)) {
            Some(index) => index,
            None => {
                self.stacks.push(vec![target]);
                self.stacks.len() - 1
            }
        };
        let stack = &mut self.stacks[index];
        let at = stack
            .iter()
            .position(|member| *member == target)
            .unwrap_or(0)
            + 1;
        stack.splice(at..at, moving);
        // A group left without tabs goes, as after any move.
        self.active_tab = self.tab_of(target).unwrap_or(0);
        self.clear_selection();
        self.normalize();
        self.focus(pane)
    }

    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.groups.iter().find(|group| group.id == id)
    }

    fn group_mut(&mut self, id: GroupId) -> Option<&mut Group> {
        self.groups.iter_mut().find(|group| group.id == id)
    }

    /// Opens a tab after the active one, in the same group, and selects it. A pin is not
    /// inherited: from a pinned tab the new one lands first after the pinned region.
    pub fn new_tab(&mut self) -> PaneId {
        let pane = self.allocate();
        let mut tab = self.fresh_tab(pane);
        tab.group = self.active().group;
        self.active_tab += 1;
        self.tabs.insert(self.active_tab, tab);
        self.normalize();
        pane
    }

    /// Splits the active pane and focuses the new one.
    pub fn split(&mut self, axis: Axis) -> PaneId {
        let pane = self.allocate();
        let tab = &mut self.tabs[self.active_tab];
        let split = tab.layout.split(tab.active_pane, axis, pane);
        debug_assert!(split, "active pane belongs to the active tab");
        tab.active_pane = pane;
        tab.maximized = false;
        pane
    }

    pub fn close(&mut self, pane: PaneId) -> Option<Closed> {
        let index = self.tab_of(pane)?;
        // A tab of a place: the next one (else the one before) takes its place if it showed.
        if let Some(at) = self.stacks.iter().position(|stack| stack.contains(&pane)) {
            let stack = &mut self.stacks[at];
            let gone = stack.iter().position(|member| *member == pane).unwrap_or(0);
            stack.remove(gone);
            let next = stack[gone.min(stack.len() - 1)];
            if stack.len() < 2 {
                self.stacks.remove(at);
            }
            let tab = &mut self.tabs[index];
            if tab.layout.replace(pane, next) && tab.active_pane == pane {
                tab.active_pane = next;
            }
            return Some(Closed::Pane);
        }
        if self.tabs[index].remove_pane(pane) {
            return Some(Closed::Pane);
        }
        if self.tabs.len() == 1 {
            let fresh = self.allocate();
            self.tabs[0] = self.fresh_tab(fresh);
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

    /// The multi-selection, which always includes the active tab once anything is selected.
    pub fn selected_indices(&self) -> Vec<usize> {
        if !self.tabs.iter().any(|tab| tab.selected) {
            return Vec::new();
        }
        (0..self.tabs.len())
            .filter(|&index| self.tabs[index].selected || index == self.active_tab)
            .collect()
    }

    /// The tabs a group command applies to: the selection, else the active tab.
    pub fn target_tabs(&self) -> Vec<usize> {
        let selected = self.selected_indices();
        if selected.is_empty() {
            vec![self.active_tab]
        } else {
            selected
        }
    }

    /// Shift-click: adds every tab between the active tab and `index`, expanding
    /// collapsed groups in the range so the user sees what is selected.
    pub fn select_range(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let (low, high) = (index.min(self.active_tab), index.max(self.active_tab));
        for tab in &mut self.tabs[low..=high] {
            tab.selected = true;
            if let Some(group) = tab.group
                && let Some(group) = self.groups.iter_mut().find(|g| g.id == group)
            {
                group.collapsed = false;
            }
        }
    }

    /// Cmd-click: adds or removes one tab.
    pub fn toggle_selected(&mut self, index: usize) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.selected = !tab.selected;
        }
    }

    pub fn clear_selection(&mut self) {
        for tab in &mut self.tabs {
            tab.selected = false;
        }
    }

    /// Removes the tabs at `indices` and returns them in order.
    fn take(&mut self, indices: &[usize]) -> Vec<Tab> {
        let mut indices: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|&index| index < self.tabs.len())
            .collect();
        indices.sort_unstable();
        indices.dedup();
        let mut taken: Vec<Tab> = indices
            .iter()
            .rev()
            .map(|&index| self.tabs.remove(index))
            .collect();
        taken.reverse();
        taken
    }

    /// Re-inserts `block` at `at`, keeping the active tab and group adjacency.
    fn put(&mut self, at: usize, block: Vec<Tab>, active: PaneId) {
        let at = at.min(self.tabs.len());
        self.tabs.splice(at..at, block);
        self.active_tab = self.tab_of(active).unwrap_or(0);
        self.clear_selection();
        self.normalize();
    }

    /// Puts the tabs at `indices` in a new group, anchored at the earliest one.
    pub fn new_group(&mut self, indices: &[usize]) -> Option<GroupId> {
        let first = indices
            .iter()
            .copied()
            .filter(|&i| i < self.tabs.len())
            .min()?;
        let active = self.active_pane();
        let id = GroupId(self.next_group);
        self.next_group += 1;
        // A group made from pinned tabs stays pinned.
        let pinned = indices.iter().all(|&index| self.tab_pinned(index));
        self.groups.push(Group {
            id,
            name: None,
            color: (id.0 as usize - 1) % GROUP_COLORS,
            collapsed: false,
            pinned,
        });
        let mut block = self.take(indices);
        for tab in &mut block {
            tab.group = Some(id);
        }
        self.put(first, block, active);
        Some(id)
    }

    /// Moves the tabs at `indices` to the end of `group`.
    pub fn move_to_group(&mut self, indices: &[usize], group: GroupId) {
        if self.group(group).is_none() {
            return;
        }
        let active = self.active_pane();
        let mut block = self.take(indices);
        for tab in &mut block {
            tab.group = Some(group);
        }
        let at = self
            .tabs
            .iter()
            .rposition(|tab| tab.group == Some(group))
            .map_or(self.tabs.len(), |last| last + 1);
        self.put(at, block, active);
    }

    /// Takes the tabs at `indices` out of their group and places them right after it.
    pub fn remove_from_group(&mut self, indices: &[usize]) {
        let Some(group) = indices
            .iter()
            .find_map(|&index| self.tabs.get(index).and_then(|tab| tab.group))
        else {
            return;
        };
        let first = indices.iter().copied().min().unwrap_or(0);
        let active = self.active_pane();
        let mut block = self.take(indices);
        for tab in &mut block {
            tab.group = None;
            // Tabs leaving a group are unpinned, as in Warp.
            tab.pinned = false;
        }
        let at = self
            .tabs
            .iter()
            .rposition(|tab| tab.group == Some(group))
            .map_or(first.min(self.tabs.len()), |last| last + 1);
        self.put(at, block, active);
    }

    /// The groups the tabs at `indices` could move to: every group they are not all in already.
    pub fn destination_groups(&self, indices: &[usize]) -> Vec<GroupId> {
        let shared = self.shared_group(indices);
        self.groups
            .iter()
            .map(|group| group.id)
            .filter(|id| Some(*id) != shared)
            .collect()
    }

    /// The group every tab at `indices` belongs to, if they share one.
    pub fn shared_group(&self, indices: &[usize]) -> Option<GroupId> {
        let mut groups = indices
            .iter()
            .map(|&index| self.tabs.get(index).and_then(|tab| tab.group));
        let first = groups.next()??;
        groups.all(|group| group == Some(first)).then_some(first)
    }

    /// Moves the tabs at `indices` to sit before index `to` (`to == len` appends), in
    /// their order, and sets their group.
    pub fn move_tabs(&mut self, indices: &[usize], to: usize, group: Option<GroupId>) {
        if to > self.tabs.len() || indices.iter().any(|&index| index >= self.tabs.len()) {
            return;
        }
        let group = group.filter(|id| self.group(*id).is_some());
        let active = self.active_pane();
        let before = indices.iter().filter(|&&index| index < to).count();
        let mut block = self.take(indices);
        for tab in &mut block {
            if tab.group != group {
                tab.pinned = false;
            }
            tab.group = group;
        }
        self.put(to - before, block, active);
    }

    pub fn toggle_group(&mut self, id: GroupId) {
        if let Some(group) = self.group_mut(id) {
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
        let tabs = &self.tabs;
        self.groups
            .retain(|group| tabs.iter().any(|tab| tab.group == Some(group.id)));
        // Pinned tabs and groups lead the list; the partition keeps groups contiguous
        // because every member shares its group's pin.
        let (pinned, rest): (Vec<Tab>, Vec<Tab>) = std::mem::take(&mut self.tabs)
            .into_iter()
            .partition(|tab| self.is_pinned(tab));
        self.tabs = pinned.into_iter().chain(rest).collect();
        self.active_tab = self.tab_of(active).unwrap_or(0);
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

    /// Focuses `pane`, showing it in its place if it waited behind another tab there. A
    /// maximized place stays maximized on any of its tabs.
    pub fn focus(&mut self, pane: PaneId) -> bool {
        let Some(index) = self.tab_of(pane) else {
            return false;
        };
        let shown = self.shown(pane);
        self.active_tab = index;
        let tab = &mut self.tabs[index];
        tab.maximized &= tab.active_pane == shown;
        tab.layout.replace(shown, pane);
        tab.active_pane = pane;
        true
    }

    /// Maximizing needs another pane to hide.
    pub fn toggle_maximized(&mut self) {
        let tab = &mut self.tabs[self.active_tab];
        tab.maximized = !tab.maximized && tab.layout.panes().len() > 1;
    }

    /// Moves to the next or previous pane of the active tab in layout order, wrapping. A
    /// maximized tab stays maximized, now on that pane (as `focus_direction`).
    pub fn focus_cycle(&mut self, forward: bool) -> bool {
        let tab = &mut self.tabs[self.active_tab];
        let panes = tab.layout.panes();
        let Some(index) = panes.iter().position(|pane| *pane == tab.active_pane) else {
            return false;
        };
        if panes.len() < 2 {
            return false;
        }
        let count = panes.len();
        let next = if forward {
            (index + 1) % count
        } else {
            (index + count - 1) % count
        };
        tab.active_pane = panes[next];
        true
    }

    /// Moves to the pane on `direction`'s side; a maximized tab shows that one maximized.
    pub fn focus_direction(&mut self, direction: Direction) -> bool {
        let Some(pane) = self.active().neighbor(direction) else {
            return false;
        };
        self.tabs[self.active_tab].active_pane = pane;
        true
    }

    /// Drag and drop: moves `pane` to `side` of `target`, from its own tab or another one.
    /// A tab left without panes goes; the moved pane takes focus.
    pub fn move_pane(&mut self, pane: PaneId, target: PaneId, side: Direction) -> bool {
        let (Some(from), Some(_)) = (self.tab_of(pane), self.tab_of(target)) else {
            return false;
        };
        if pane == target {
            return false;
        }
        if !self.tabs[from].remove_pane(pane) {
            self.take(&[from]);
        }
        self.place(target, side, Layout::Pane(pane), pane)
    }

    /// Drag and drop: moves every pane of `tabs` to `side` of `target`, in tab order, and
    /// those tabs go. The tab holding `target` itself is left alone.
    pub fn merge_tabs(&mut self, tabs: &[TabId], target: PaneId, side: Direction) -> bool {
        let Some(into) = self.tab_of(target) else {
            return false;
        };
        let indices: Vec<usize> = tabs
            .iter()
            .filter_map(|&id| self.tab_index(id))
            .filter(|&index| index != into)
            .collect();
        let mut taken = self.take(&indices).into_iter();
        let Some(first) = taken.next() else {
            return false;
        };
        let (axis, _) = side.placement();
        let focus = first.active_pane;
        let layout = taken.fold(first.layout, |first, tab| Layout::Split {
            axis,
            ratio: 0.5,
            first: Box::new(first),
            second: Box::new(tab.layout),
        });
        self.place(target, side, layout, focus)
    }

    /// Drag and drop onto the sidebar: `pane` leaves its tab for a new one placed before
    /// index `to` (`to == len` appends) in `group`, and takes focus. A pane alone in its
    /// tab moves with that tab.
    pub fn detach_pane(&mut self, pane: PaneId, to: usize, group: Option<GroupId>) -> bool {
        let Some(from) = self.tab_of(pane) else {
            return false;
        };
        if to > self.tabs.len() {
            return false;
        }
        if !self.tabs[from].remove_pane(pane) {
            self.move_tabs(&[from], to, group);
            return self.focus(pane);
        }
        let mut tab = self.fresh_tab(pane);
        tab.group = group.filter(|id| self.group(*id).is_some());
        self.tabs.insert(to, tab);
        self.active_tab = to;
        self.clear_selection();
        self.normalize();
        true
    }

    /// Puts `layout` at `side` of `target` and focuses `focus` there.
    /// A new pane beside `target` on `side`, focused; `None` if `target` is gone.
    pub fn split_beside(&mut self, target: PaneId, side: Direction) -> Option<PaneId> {
        self.tab_of(target)?;
        let pane = self.allocate();
        self.place(target, side, Layout::Pane(pane), pane)
            .then_some(pane)
    }

    fn place(&mut self, target: PaneId, side: Direction, layout: Layout, focus: PaneId) -> bool {
        let Some(index) = self.tab_of(target) else {
            return false;
        };
        let tab = &mut self.tabs[index];
        let placed = tab.layout.insert(target, side, layout);
        tab.active_pane = focus;
        tab.maximized = false;
        self.active_tab = index;
        self.clear_selection();
        self.normalize();
        placed
    }

    /// Every pane, those waiting in stacks included.
    pub fn panes(&self) -> impl Iterator<Item = PaneId> + '_ {
        (0..self.tabs.len()).flat_map(|index| self.tab_panes(index))
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
    fn a_pane_opens_beside_another() {
        let (mut ws, first) = Workspace::new();
        let left = ws.split_beside(first, Direction::Left).unwrap();
        assert_eq!(ws.active().layout.panes(), [left, first]);
        assert_eq!(ws.active_pane(), left);
        assert_eq!(ws.split_beside(PaneId(99), Direction::Up), None);
    }

    #[test]
    fn panes_move_beside_others_and_tabs_merge_into_one() {
        let (mut ws, a) = Workspace::new();
        let b = ws.split(Axis::Horizontal);
        // Within a tab: b goes above a.
        assert!(ws.move_pane(b, a, Direction::Up));
        assert_eq!(
            ws.active().layout,
            Layout::Split {
                axis: Axis::Vertical,
                ratio: 0.5,
                first: Box::new(Layout::Pane(b)),
                second: Box::new(Layout::Pane(a)),
            }
        );
        assert!(!ws.move_pane(a, a, Direction::Left));
        // From another tab: its only pane moves and the emptied tab goes.
        let c = ws.new_tab();
        assert_eq!(ws.tabs.len(), 2);
        assert!(ws.move_pane(c, a, Direction::Left));
        assert_eq!(ws.tabs.len(), 1);
        assert_eq!(ws.active().layout.panes(), [b, c, a]);
        assert_eq!(ws.active_pane(), c);
        // Whole tabs: two split ones merge beside `a`, keeping their own layouts.
        let d = ws.new_tab();
        let e = ws.split(Axis::Vertical);
        let f = ws.new_tab();
        let (first, second) = (ws.tabs[1].id, ws.tabs[2].id);
        ws.tabs[1].selected = true;
        assert!(ws.merge_tabs(&[first, second, ws.tabs[0].id], a, Direction::Right));
        assert_eq!(ws.tabs.len(), 1);
        assert_eq!(ws.active().layout.panes(), [b, c, a, d, e, f]);
        assert_eq!(ws.active_pane(), e);
        assert!(ws.tabs.iter().all(|tab| !tab.selected));
        assert!(!ws.merge_tabs(&[ws.tabs[0].id], a, Direction::Right));
    }

    #[test]
    fn a_pane_dropped_on_the_sidebar_becomes_its_own_tab() {
        let (mut ws, a) = Workspace::new();
        let b = ws.split(Axis::Horizontal);
        let c = ws.new_tab();
        // Before the first tab: b leaves a's tab and is focused there.
        assert!(ws.detach_pane(b, 0, None));
        assert_eq!(ws.tabs.len(), 3);
        assert_eq!(ws.tabs[0].layout, Layout::Pane(b));
        assert_eq!(ws.tabs[1].layout, Layout::Pane(a));
        assert_eq!(ws.active_pane(), b);
        // A pane alone in its tab just moves that tab, here to the end.
        assert!(ws.detach_pane(b, 3, None));
        assert_eq!(ws.tabs.len(), 3);
        assert_eq!(ws.tabs[2].layout, Layout::Pane(b));
        assert_eq!(ws.active_pane(), b);
        assert!(!ws.detach_pane(c, 9, None));
        assert!(!ws.detach_pane(PaneId(99), 0, None));
    }

    #[test]
    fn move_tab_stays_inside_its_group_and_pin_region() {
        let (mut ws, a) = Workspace::new();
        let b = ws.new_tab();
        let c = ws.new_tab();
        ws.set_tab_pinned(0, true);
        assert!(
            !ws.can_move_tab(0, false),
            "pinned tab can't pass unpinned ones"
        );
        assert!(!ws.can_move_tab(0, true));
        ws.move_tab(2, true);
        assert_eq!(ws.panes().collect::<Vec<_>>(), [a, c, b]);
        assert_eq!(ws.active_pane(), c, "the active tab follows its pane");
        ws.new_group(&[1]);
        assert!(!ws.can_move_tab(2, true), "can't leave its group by moving");
        ws.set_tab_color(2, Some(1));
        assert_eq!(ws.tabs[2].color, Some(1));
        ws.set_tab_color(2, Some(99));
        assert_eq!(ws.tabs[2].color, None);
    }

    #[test]
    fn panes_cycle_and_equalize_by_count() {
        let (mut ws, first) = Workspace::new();
        let second = ws.split(Axis::Horizontal);
        let third = ws.split(Axis::Horizontal);
        assert!(ws.focus_cycle(true));
        assert_eq!(ws.active_pane(), first, "wraps after the last pane");
        assert!(ws.focus_cycle(false));
        assert_eq!(ws.active_pane(), third);
        ws.tabs[0].layout.set_ratio(&[], 0.9);
        ws.tabs[0].layout.equalize();
        let widths: Vec<f32> = ws
            .active()
            .layout
            .rects()
            .iter()
            .map(|(_, r)| r.width)
            .collect();
        for width in widths {
            assert!((width - 1.0 / 3.0).abs() < 1e-4, "{width}");
        }
        let _ = second;
    }

    #[test]
    fn tabs_in_a_pane_share_its_place_and_close_into_each_other() {
        let (mut ws, first) = Workspace::new();
        let right = ws.split(Axis::Horizontal);
        let second = ws.add_pane_tab();
        assert_eq!(
            ws.active().layout.panes(),
            [first, second],
            "the new tab shows in place"
        );
        assert_eq!(ws.stack(right), Some(&[right, second][..]));
        assert_eq!(
            ws.tab_of(right),
            Some(0),
            "a waiting tab still belongs to its tab"
        );
        assert_eq!(ws.panes().collect::<Vec<_>>(), [first, right, second]);
        assert!(ws.focus(right));
        assert_eq!(ws.active().layout.panes(), [first, right]);
        assert!(ws.cycle_pane_tab(true));
        assert_eq!(ws.active_pane(), second);
        assert!(
            !ws.focus_direction(Direction::Right),
            "one place, not two panes"
        );
        // Closing the shown tab shows its neighbour; one left is a plain pane again.
        assert_eq!(ws.close(second), Some(Closed::Pane));
        assert_eq!(ws.active().layout.panes(), [first, right]);
        assert_eq!(ws.active_pane(), right);
        assert!(ws.stacks.is_empty());
        // A waiting tab closes without disturbing the one shown.
        let third = ws.add_pane_tab();
        ws.focus(right);
        assert_eq!(ws.close(third), Some(Closed::Pane));
        assert_eq!(ws.active().layout.panes(), [first, right]);
        assert!(ws.stacks.is_empty());
    }

    #[test]
    fn a_pane_dropped_on_another_joins_its_tabs() {
        let (mut ws, first) = Workspace::new();
        let other = ws.new_tab();
        let beside = ws.split(Axis::Horizontal);
        // A pane alone in its tab takes the tab with it.
        assert!(ws.stack_onto(first, beside));
        assert_eq!(ws.tabs.len(), 1);
        assert_eq!(ws.active().layout.panes(), [other, first]);
        assert_eq!(ws.stack(first), Some(&[beside, first][..]));
        assert_eq!(ws.active_pane(), first);
        // A place's own tabs come along, after the target.
        let lone = ws.split(Axis::Vertical);
        assert!(ws.stack_onto(first, other));
        assert_eq!(ws.active().layout.panes(), [first, lone]);
        assert_eq!(ws.stack(other), Some(&[other, beside, first][..]));
        assert!(!ws.stack_onto(beside, first), "already together");
    }

    #[test]
    fn a_group_emptied_by_joining_tabs_goes() {
        let (mut ws, first) = Workspace::new();
        let grouped = ws.new_tab();
        let group = ws.new_group(&[1]).unwrap();
        assert!(ws.stack_onto(grouped, first));
        assert!(ws.group(group).is_none());
        assert_eq!(ws.tabs.len(), 1);
        assert_eq!(ws.active_pane(), grouped);
    }

    #[test]
    fn maximize_ends_on_layout_changes_and_follows_pane_moves() {
        let (mut ws, first) = Workspace::new();
        ws.toggle_maximized();
        assert!(!ws.active().maximized, "a lone pane has nothing to hide");
        let right = ws.split(Axis::Horizontal);
        ws.toggle_maximized();
        assert!(ws.active().maximized);
        ws.focus(right);
        assert!(ws.active().maximized, "refocusing the same pane keeps it");
        ws.focus(first);
        assert!(!ws.active().maximized);
        ws.toggle_maximized();
        assert!(ws.focus_direction(Direction::Right));
        assert!(
            ws.active().maximized,
            "moving between panes keeps the tab maximized"
        );
        assert_eq!(ws.active().active_pane, right);
        assert!(ws.focus_cycle(true));
        assert!(ws.active().maximized);
        assert_eq!(ws.active().active_pane, first);
        ws.split(Axis::Vertical);
        assert!(!ws.active().maximized);
        ws.toggle_maximized();
        ws.close(first);
        assert!(!ws.active().maximized);
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
        let group = ws.new_group(&[0]).unwrap();
        // c joins by being dropped onto the group; it moves next to a.
        ws.move_tabs(&[2], 3, Some(group));
        let order = |ws: &Workspace| ws.tabs.iter().map(|t| t.active_pane).collect::<Vec<_>>();
        assert_eq!(order(&ws), [a, c, b]);
        assert_eq!(ws.active_pane(), c);
        // A new tab from a grouped tab joins the group.
        let d = ws.new_tab();
        assert_eq!(ws.active().group, Some(group));
        assert_eq!(ws.group_tabs(group).len(), 3);
        // Ungrouping places the tab after the group's block.
        ws.remove_from_group(&[0]);
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
    fn shift_and_cmd_selection_group_together() {
        let (mut ws, a) = Workspace::new();
        let b = ws.new_tab();
        let c = ws.new_tab();
        let d = ws.new_tab();
        let order = |ws: &Workspace| ws.tabs.iter().map(|t| t.active_pane).collect::<Vec<_>>();
        // Active is d (index 3); shift-click b selects b..=d.
        ws.select_range(1);
        assert_eq!(ws.selected_indices(), [1, 2, 3]);
        // Cmd-click c out of the selection; the active tab always stays in.
        ws.toggle_selected(2);
        assert_eq!(ws.selected_indices(), [1, 3]);
        let group = ws.new_group(&ws.selected_indices()).unwrap();
        assert_eq!(order(&ws), [a, b, d, c]);
        assert_eq!(ws.group_tabs(group).len(), 2);
        assert_eq!(ws.active_pane(), d);
        assert!(ws.selected_indices().is_empty());
        // Move a and c into the group; they join at its end.
        ws.select_tab(0);
        ws.toggle_selected(3);
        assert_eq!(ws.destination_groups(&ws.selected_indices()), [group]);
        ws.move_to_group(&ws.selected_indices(), group);
        assert_eq!(order(&ws), [b, d, a, c]);
        assert_eq!(ws.shared_group(&[0, 1, 2, 3]), Some(group));
        // Removing two from the group places them after it.
        ws.remove_from_group(&[0, 1]);
        assert_eq!(order(&ws), [a, c, b, d]);
        assert_eq!(ws.shared_group(&[0, 1]), Some(group));
        assert_eq!(ws.tabs[3].group, None);
    }

    #[test]
    fn shift_range_expands_collapsed_groups() {
        let (mut ws, _) = Workspace::new();
        ws.new_tab();
        let group = ws.new_group(&[1]).unwrap();
        ws.toggle_group(group);
        ws.select_tab(0);
        ws.select_range(1);
        assert!(!ws.group(group).unwrap().collapsed);
    }

    #[test]
    fn pins_lead_the_list_and_groups_move_within_their_region() {
        let (mut ws, a) = Workspace::new();
        let b = ws.new_tab();
        let c = ws.new_tab();
        let d = ws.new_tab();
        let order = |ws: &Workspace| ws.tabs.iter().map(|t| t.active_pane).collect::<Vec<_>>();
        ws.set_tab_pinned(2, true);
        assert_eq!(order(&ws), [c, a, b, d]);
        assert_eq!(ws.active_pane(), d);
        let group = ws.new_group(&[2, 3]).unwrap();
        assert_eq!(order(&ws), [c, a, b, d]);
        ws.move_group(group, true);
        assert_eq!(order(&ws), [c, b, d, a]);
        // The pinned tab blocks moving further up.
        ws.move_group(group, true);
        assert_eq!(order(&ws), [c, b, d, a]);
        ws.set_group_pinned(group, true);
        assert!(ws.tab_pinned(2));
        // Leaving a pinned group unpins the tab and sends it after the pinned region.
        ws.remove_from_group(&[1]);
        assert_eq!(order(&ws), [c, d, b, a]);
        assert!(!ws.tab_pinned(2));
    }

    #[test]
    fn new_tab_from_a_pinned_tab_is_not_pinned() {
        let (mut ws, a) = Workspace::new();
        let b = ws.new_tab();
        ws.select_tab(0);
        ws.set_tab_pinned(0, true);
        let c = ws.new_tab();
        let order = |ws: &Workspace| ws.tabs.iter().map(|t| t.active_pane).collect::<Vec<_>>();
        assert_eq!(order(&ws), [a, c, b]);
        assert!(!ws.tab_pinned(1));
        assert_eq!(ws.active_pane(), c);
        // A pinned group's new tab joins the group, so it shares the group's pin.
        let group = ws.new_group(&[1]).unwrap();
        ws.set_group_pinned(group, true);
        ws.focus(c);
        ws.new_tab();
        assert!(ws.tab_pinned(ws.active_tab));
    }

    #[test]
    fn several_tabs_move_together_and_titles_can_be_renamed() {
        let (mut ws, a) = Workspace::new();
        let b = ws.new_tab();
        let c = ws.new_tab();
        let d = ws.new_tab();
        let order = |ws: &Workspace| ws.tabs.iter().map(|t| t.active_pane).collect::<Vec<_>>();
        ws.move_tabs(&[0, 2], 4, None);
        assert_eq!(order(&ws), [b, d, a, c]);
        ws.move_tabs(&[2, 3], 0, None);
        assert_eq!(order(&ws), [a, c, b, d]);
        let id = ws.tabs[0].id;
        ws.rename_tab(id, "  server  ");
        assert_eq!(ws.tabs[0].title.as_deref(), Some("server"));
        ws.rename_tab(id, " ");
        assert_eq!(ws.tabs[0].title, None);
        let group = ws.new_group(&[0]).unwrap();
        assert_eq!(ws.group(group).unwrap().name, None);
        ws.rename_group(group, "작업");
        assert_eq!(ws.group(group).unwrap().name.as_deref(), Some("작업"));
    }

    #[test]
    fn last_tab_is_replaced_instead_of_closing_the_window() {
        let (mut ws, a) = Workspace::new();
        ws.new_group(&[0]);
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
