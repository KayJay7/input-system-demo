use crate::types::Keycode;
use frozen_collections::FzScalarSet;
use std::cmp::{Ordering, max};
use tinyset::SetUsize;

#[derive(Debug, Clone)]
pub struct Group {
    pub index: usize,                // index of self (for partial ordering)
    pub greater: FzScalarSet<usize>, // supergroups
    pub pred: Range,                 // neighboring subgroups
    pub intersect: Range,            // unordered intersectors
    pub active_combos: SetUsize,     // currently down combos
    pub counter: usize,              // #currently down modifier keys
    pub size: usize,                 // #modifiers
    pub mask_weight: i32,            // #(1?)-masking subgroups
    pub keys: Range,
    pub mask: bool,
}

impl Group {
    #[inline]
    pub fn is_active(&self) -> bool {
        self.counter == self.size
    }

    #[inline]
    pub fn iter_intersect<'a>(
        &self,
        groups_intersect: &'a [usize],
    ) -> impl Iterator<Item = &'a usize> + use<'a> {
        self.intersect.into_iter().map(|i| &groups_intersect[i])
    }

    #[inline]
    pub fn iter_pred<'a>(
        &self,
        groups_pred: &'a [usize],
    ) -> impl Iterator<Item = &'a usize> + use<'a> {
        self.pred.into_iter().map(|i| &groups_pred[i])
    }

    #[inline]
    pub fn iter_keys<'a>(
        &self,
        groups_keys: &'a [usize],
    ) -> impl Iterator<Item = &'a usize> + use<'a> {
        self.keys.into_iter().map(|i| &groups_keys[i])
    }
}
impl PartialEq for Group {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl PartialOrd for Group {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self == other {
            return Some(Ordering::Equal);
        }
        if self.greater.contains(&other.index) {
            return Some(Ordering::Less);
        }
        if other.greater.contains(&self.index) {
            return Some(Ordering::Greater);
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct Key<U: Keycode> {
    // key: Keycode,              // validate mphf
    pub action: Option<U>,      // action key: unmodified action
    pub combos: Range,                // action key: modified mappings
    pub active_combo: Option<usize>, // action key: active action
    pub latching: bool,               // action key: after modifier deactivation
    pub immediate: bool,              // modifier key: keydown immediately
    pub groups: Range,                // modifier key: superset modifier groups
    pub open: bool,                   // modifier key: no action yet
    pub cache_counter: i32,
}
impl<U:Keycode> Key<U> {
    #[inline]
    pub fn is_modifier(&self) -> bool {
        !self.groups.is_empty()
    }

    // #[inline]
    // pub fn is_action(&self) -> bool {
    //     !self.combos.is_empty() || self.action.is_some()
    // }

    #[inline]
    pub fn is_immediate(&self) -> bool {
        !self.is_modifier() || self.immediate
    }

    #[inline]
    pub fn iter_combos<'a, T:Keycode>(
        &self,
        keys_combos: &'a [Combo<T>],
    ) -> impl Iterator<Item = Combo<T>> + use<'a, T, U> {
        self.combos.into_iter().map(|i| keys_combos[i])
    }

    #[inline]
    pub fn iter_groups<'a>(
        &self,
        keys_groups: &'a [usize],
    ) -> impl Iterator<Item = &'a usize> + use<'a, U> {
        self.groups.into_iter().map(|i| &keys_groups[i])
    }

    #[inline]
    pub fn get_combo<T:Keycode>(&self, index: usize, keys_combos: &[Combo<T>]) -> Combo<T> {
        keys_combos[self.combos.ind(index)]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Combo<T:Keycode> {
    pub action: T,       // target action
    pub modifier_group: usize, // modifier group index
}

#[derive(Debug, Clone, Copy)]
pub struct Range {
    start: usize,
    end: usize,
}

impl Range {
    pub fn new(start: usize, end: usize) -> Range {
        Range { start, end }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    #[inline]
    pub fn len(&self) -> usize {
        max(0, self.end - self.start)
    }

    #[inline]
    pub fn ind(&self, index: usize) -> usize {
        assert!(index < self.len());
        self.start + index
    }
}

impl IntoIterator for Range {
    type Item = usize;
    type IntoIter = std::ops::Range<usize>;

    fn into_iter(self) -> Self::IntoIter {
        self.start..self.end
    }
}
