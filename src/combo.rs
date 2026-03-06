use crate::types::keys::Keycode;
use crate::types::{Config, Event, State};
use frozen_collections::{FzScalarMap, FzScalarSet};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use tinyset::SetUsize;

const EVENT_BUFFER_WARMUP: usize = 16;

#[derive(Debug, Clone)]
struct Group {
    index: usize,                // index of self (for partial ordering)
    greater: FzScalarSet<usize>, // supergroups
    pred: Box<[usize]>,          // neighboring subgroups
    intersect: Box<[usize]>,     // unordered intersectors
    active_combos: SetUsize,     // currently down combos
    counter: usize,              // #currently down modifier keys
    size: usize,                 // #modifiers
    mask_weight: i32,            // #(1?)-masking subgroups
    keys: Box<[usize]>,
    mask: bool,
}

impl Group {
    fn is_active(&self) -> bool {
        self.counter == self.size
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

#[derive(Debug, Clone, Default)]
struct Key {
    // key: Keycode,                 // validate mphf
    action: Option<Keycode>,      // action key: unmodified action
    combos: Box<[Combo]>,         // action key: modified mappings
    active_action: Option<usize>, // action key: active action
    latching: bool,               // action key: after modifier deactivation
    immediate: bool,              // modifier key: keydown immediately
    groups: Box<[usize]>,         // modifier key: superset modifier groups
    open: bool,                   // modifier key: no action yet
}

impl Key {
    #[inline]
    fn is_modifier(&self) -> bool {
        !self.groups.is_empty()
    }

    #[inline]
    fn is_action(&self) -> bool {
        !self.combos.is_empty() || self.action.is_some()
    }

    #[inline]
    fn is_immediate(&self) -> bool {
        !self.is_modifier() || self.immediate
    }
}

#[derive(Debug, Clone, Copy)]
struct Combo {
    action: Keycode,       // target action
    modifier_group: usize, // modifier group index
}

pub struct ComboHandler {
    domain: FzScalarMap<Keycode, usize>, // keycode to key index
    keys: Box<[Key]>,                    // keys
    groups: Box<[Group]>,                // modifier groups graph
    masks: i32,                          // #active masks
    events: VecDeque<Event>,             // output event queue
}

impl ComboHandler {
    #[inline]
    fn is_masking(&self) -> bool {
        self.masks > 0
    }

    pub fn new(config: Config) -> ComboHandler {
        #[derive(Default)]
        struct MutKey {
            action: Option<Keycode>,
            combos: Vec<Combo>,
            latching: bool,
            immediate: bool,
            groups: Vec<usize>,
        }
        impl MutKey {
            fn freeze(mut self, groups: &Box<[Group]>) -> Key {
                self.combos.sort_unstable_by(|x, y| {
                    groups[y.modifier_group]
                        .size
                        .cmp(&groups[x.modifier_group].size)
                });
                Key {
                    action: self.action,
                    combos: self.combos.into_boxed_slice(),
                    active_action: None,
                    latching: self.latching,
                    immediate: self.immediate,
                    groups: self.groups.into_boxed_slice(),
                    open: false,
                }
            }
        }

        struct MutGroup {
            index: usize,
            greater: Vec<usize>,
            pred: Vec<usize>,
            intersect: Vec<usize>,
            keys: Vec<usize>,
            mask: bool,
        }

        impl Into<Group> for MutGroup {
            fn into(self) -> Group {
                Group {
                    index: self.index,
                    greater: self.greater.into_iter().collect(),
                    pred: self.pred.into_boxed_slice(),
                    intersect: self.intersect.into_boxed_slice(),
                    active_combos: SetUsize::new(),
                    counter: 0,
                    size: self.keys.len(),
                    keys: self.keys.into_boxed_slice(),
                    mask: self.mask,
                    mask_weight: 0,
                }
            }
        }

        // graph build
        let (named_groups, groups): (HashMap<String, usize>, Vec<HashSet<Keycode>>) = config
            .modifiers
            .iter()
            .enumerate()
            .map(|(i, modifier_decl)| ((modifier_decl.id.clone(), i), modifier_decl.keys.clone()))
            .unzip();
        let mut edges = vec![(vec![], vec![], vec![]); groups.len()];
        for (a_index, a) in groups.iter().enumerate() {
            for (b_index, b) in groups.iter().enumerate() {
                if a_index == b_index || a.is_disjoint(b) || a.is_superset(b) {
                    // ignore self loops and symmetry
                    continue;
                }
                if a.is_subset(b) {
                    // a ⊆ b
                    edges[a_index].0.push(b_index);

                    if !edges[b_index]
                        .1
                        .iter()
                        .any(|below: &usize| groups[*below].is_superset(a))
                    {
                        // b ∈ succ(a)
                        edges[b_index]
                            .1
                            // drop all belows ⊆ a
                            .retain(|below| !groups[*below].is_subset(a));
                        edges[b_index].1.push(a_index);
                    }
                    continue;
                }
                // unordered intersection
                edges[a_index].2.push(b_index);
            }
        }

        edges

        let mut domain: HashMap<Keycode, usize> = HashMap::new();
        let mut temp_keys: Vec<MutKey> = vec![];
        // domain: populate modifiers
        for (i, group) in groups.into_iter().enumerate() {
            for keycode in group {
                if let Some(key) = domain.get(&keycode) {
                    temp_keys[*key].groups.push(i);
                } else {
                    domain.insert(keycode, temp_keys.len());
                    let mut temp_key = MutKey::default();
                    temp_key.groups.push(i);
                    temp_keys.push(temp_key);
                }
            }
        }

        let mut groups: Box<[Group]> = edges
            .into_iter()
            .enumerate()
            .zip(config.modifiers)
            .map(|((index, (above, below, intersect)), modifier_decl)| {
                // collect modifier keys
                let mut keys = Vec::new();
                for key in modifier_decl.keys {
                    keys.push(domain[&key]);
                }
                MutGroup {
                    index,
                    greater: above,
                    pred: below,
                    intersect,
                    keys,
                    mask: modifier_decl.masking,
                }
            })
            .map(MutGroup::into)
            .collect();

        for group in 0..groups.len() {
            groups[group].mask_weight = groups[group].mask as i32
                - groups[group]
                    .pred
                    .iter()
                    .map(|group| groups[*group].mask as i32)
                    .sum::<i32>();
        }

        // domain: populate action keys
        for action in config.actions.iter() {
            let temp_key: &mut MutKey;
            if let Some(i) = domain.get(&action.key) {
                temp_key = &mut temp_keys[*i];
            } else {
                let i = temp_keys.len();
                domain.insert(action.key, i);
                temp_keys.push(MutKey::default());
                temp_key = &mut temp_keys[i];
            }

            temp_key.immediate = action.immediate;
            temp_key.latching = action.latching;
            temp_key.action = action.action;
            for combo in &action.modified {
                temp_key.combos.insert(
                    temp_key.combos.partition_point(|x| {
                        groups[x.modifier_group] <= groups[named_groups[&combo.modifier]]
                    }),
                    Combo {
                        action: combo.action,
                        modifier_group: named_groups[&combo.modifier],
                    },
                )
            }
        }

        ComboHandler {
            domain: FzScalarMap::new(domain.into_iter().collect()),
            keys: temp_keys
                .into_iter()
                .map(|key| key.freeze(&groups))
                .collect(),
            groups,
            events: VecDeque::with_capacity(EVENT_BUFFER_WARMUP),
            masks: 0,
        }
    }

    pub fn handle(&mut self, event: Event) -> &mut VecDeque<Event> {
        let key = *if let Some(key) = self.domain.get(&event.keycode) {
            key
        } else {
            return &mut self.events;
        };
        match event.state {
            State::Down => {
                // modifier key
                self.keys[key].open = true;
                for group in &self.keys[key].groups {
                    // increase group counter
                    self.groups[*group].counter += 1;
                    if self.groups[*group].is_active() {
                        // for every just activated group
                        self.masks += self.groups[*group].mask_weight;
                        for key in &self.groups[*group].keys {
                            // close all modifier keys
                            self.keys[*key].open = false;
                        }
                        for group in &self.groups[*group].pred {
                            for key in self.groups[*group].active_combos.drain() {
                                // terminate the actions it modified
                                if let Some(action) = self.keys[key].active_action
                                    && (!self.keys[key].is_modifier() || self.keys[key].immediate)
                                {
                                    // ignore modifiers with keyup action
                                    self.events.push_back(Event {
                                        keycode: self.keys[key].combos[action].action,
                                        state: State::Up,
                                        value: 0,
                                    });
                                    self.keys[key].active_action = None;
                                }
                            }
                        }
                    }
                }

                // optimization: skip conflict resolution on closed keyup modifier keys
                if !self.keys[key].is_immediate() && !self.keys[key].open {
                    return &mut self.events;
                }

                // action key
                let mut i: usize = 0;
                let combos = self.keys[key].combos.len();
                while i < combos
                    && !self.groups[self.keys[key].combos[i].modifier_group].is_active()
                {
                    i += 1;
                }
                // ALTERNATIVE BELOW
                // let i = self.keys[key]
                //     .combos
                //     .iter()
                //     .position(|combo| self.groups[combo.modifier_group].is_active())
                //     .unwrap_or(combos);
                if i == combos {
                    // not modified
                    if let Some(action) = self.keys[key].action
                        && self.keys[key].is_immediate()
                    {
                        self.events.push_back(Event {
                            keycode: action,
                            state: State::Down,
                            value: 0,
                        })
                    }
                    return &mut self.events;
                }

                let candidate = i;

                // search action key conflicts
                while i < combos {
                    if self.groups[self.keys[key].combos[i].modifier_group].is_active()
                        && !(self.groups[self.keys[key].combos[i].modifier_group]
                            <= self.groups[self.keys[key].combos[candidate].modifier_group])
                    {
                        if let Some(action) = self.keys[key].action
                            && !self.is_masking()
                            && self.keys[key].is_immediate()
                        {
                            self.events.push_back(Event {
                                keycode: action,
                                state: State::Down,
                                value: 0,
                            })
                        }
                        return &mut self.events;
                    }
                    i += 1;
                }

                // search modifier key conflicts
                let conflict: bool = self.groups[self.keys[key].combos[candidate].modifier_group]
                    .greater
                    .iter()
                    .any(|group| self.groups[*group].is_active()) // no active supergroups
                    || self.groups[self.keys[key].combos[candidate].modifier_group]
                    .intersect
                    .iter()
                    .any(|group| self.groups[*group].is_active()); // no active intersecting groups
                if conflict
                    && let Some(action) = self.keys[key].action
                    && !self.is_masking()
                    && self.keys[key].is_immediate()
                {
                    self.events.push_back(Event {
                        keycode: action,
                        state: State::Down,
                        value: 0,
                    });
                    return &mut self.events;
                }

                // activate combo
                self.groups[self.keys[key].combos[candidate].modifier_group]
                    .active_combos
                    .insert(key);
                self.keys[key].active_action = Some(candidate);
                if self.keys[key].is_immediate() {
                    self.events.push_back(Event {
                        keycode: self.keys[key].combos[candidate].action,
                        state: State::Down,
                        value: 0,
                    });
                }
            }
            State::Up => {
                if self.keys[key].active_action.is_some() || !self.keys[key].is_modifier() {
                    let mut action = self.keys[key].action;
                    if let Some(active_action) = self.keys[key].active_action {
                        self.groups[self.keys[key].combos[active_action].modifier_group]
                            .active_combos
                            .remove(key);
                        self.keys[key].active_action = None;
                    } else if let Some(action) = self.keys[key].action {
                    }

                    self.events.push_back(Event {
                        keycode: self.keys[key].combos[action].action,
                        state: State::Up,
                        value: 0,
                    });
                } else if self.keys[key].is_modifier() {
                }
            }
            State::Axis => {}
        }
        &mut self.events
    }
}
