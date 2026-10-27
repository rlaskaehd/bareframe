//! A tab's split layout: a tree of panes, their areas, and splitting, moving and closing
//! within it.

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

impl Direction {
    /// The axis of the split that puts a pane on this side, and whether it comes first.
    pub(crate) fn placement(self) -> (Axis, bool) {
        match self {
            Self::Left => (Axis::Horizontal, true),
            Self::Right => (Axis::Horizontal, false),
            Self::Up => (Axis::Vertical, true),
            Self::Down => (Axis::Vertical, false),
        }
    }
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
    pub(crate) fn split(&mut self, target: PaneId, axis: Axis, new: PaneId) -> bool {
        let side = match axis {
            Axis::Horizontal => Direction::Right,
            Axis::Vertical => Direction::Down,
        };
        self.insert(target, side, Self::Pane(new))
    }

    /// Replaces `target` with an even split holding it and `new` on its `side`.
    pub(crate) fn insert(&mut self, target: PaneId, side: Direction, new: Layout) -> bool {
        match self {
            Self::Pane(id) if *id == target => {
                let (axis, before) = side.placement();
                let (target, new) = (Box::new(Self::Pane(target)), Box::new(new));
                let (first, second) = if before { (new, target) } else { (target, new) };
                *self = Self::Split {
                    axis,
                    ratio: 0.5,
                    first,
                    second,
                };
                true
            }
            Self::Pane(_) => false,
            Self::Split { first, second, .. } => {
                if first.contains(target) {
                    first.insert(target, side, new)
                } else {
                    second.insert(target, side, new)
                }
            }
        }
    }

    /// Removes `target`; its sibling takes the parent split's place.
    /// Returns the pane nearest the gap, or `None` when `target` is absent or the only pane.
    pub(crate) fn remove(&mut self, target: PaneId) -> Option<PaneId> {
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

    /// Gives every pane the same share along each run of same-direction splits, as
    /// Ghostty's `equalize_splits` does: a split's ratio follows its panes on each side.
    pub fn equalize(&mut self) {
        if let Self::Split {
            ratio,
            first,
            second,
            ..
        } = self
        {
            first.equalize();
            second.equalize();
            let (a, b) = (first.panes().len() as f32, second.panes().len() as f32);
            *ratio = (a / (a + b)).clamp(MIN_RATIO, 1.0 - MIN_RATIO);
        }
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
