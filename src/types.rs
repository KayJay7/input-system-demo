use frozen_collections::Scalar;
use std::fmt::Debug;
use std::hash::Hash;

#[derive(Copy, Clone, Debug)]
pub enum Kind {
    Up,
    Down,
    Axis,
}

#[derive(Copy, Clone, Debug)]
pub struct Event<T: Keycode> {
    pub keycode: T,
    pub kind: Kind,
    pub value: i16,
}

pub trait Keycode: Clone + Copy + PartialOrd + Ord + PartialEq + Eq + Hash + Scalar + Default + Debug {}

impl<T: Clone + Copy + PartialOrd + Ord + PartialEq + Eq + Hash + Scalar + Default + Debug> Keycode for T {}