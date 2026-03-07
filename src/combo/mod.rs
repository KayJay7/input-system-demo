use crate::combo::types::{Combo, Group, Key, Range};
use crate::config::Config;
use crate::types::Keycode;
use crate::types::{Event, Kind};
use frozen_collections::FzScalarMap;
use std::collections::{HashMap, HashSet, VecDeque};
use tinyset::SetUsize;

mod types;

const EVENT_BUFFER_WARMUP: usize = 16;

pub struct ComboHandler<A:Keycode, Z:Keycode> {
    domain: FzScalarMap<A, usize>, // keycode to key index
    keys: Box<[Key<Z>]>,                    // keys
    keys_combos: Box<[Combo<Z>]>,           // optimization: packed key combos
    keys_groups: Box<[usize]>,           // optimization: packed key groups
    groups: Box<[Group]>,                // modifier groups graph
    groups_keys: Box<[usize]>,           // optimization: packed group keys
    groups_pred: Box<[usize]>,           // optimization: packed group pred
    groups_intersect: Box<[usize]>,      // optimization: packed group intersect
    masks: i32,                          // #active masks
    events: VecDeque<Event<Z>>,             // output event queue
    cache_counter: i32,
}

impl<A:Keycode, Z:Keycode> ComboHandler<A, Z> {
    #[inline]
    fn is_masking(&self) -> bool {
        self.masks > 0
    }

    pub fn new(config: Config<A, Z>) -> ComboHandler<A, Z> {
        #[derive(Default)]
        struct MutKey<B: Keycode> {
            action: Option<B>,
            combos: Vec<Combo<B>>,
            latching: bool,
            immediate: bool,
            groups: Vec<usize>,
        }
        impl<B:Keycode> MutKey<B> {
            fn freeze(
                mut self,
                groups: &[Group],
                keys_combos: &mut Vec<Combo<B>>,
                keys_groups: &mut Vec<usize>,
            ) -> Key<B> {
                self.combos.sort_unstable_by(|x, y| {
                    groups[y.modifier_group]
                        .size
                        .cmp(&groups[x.modifier_group].size)
                });
                let combos_start = keys_combos.len();
                keys_combos.extend(self.combos);
                let combos_end = keys_combos.len();

                let groups_start = keys_groups.len();
                keys_groups.extend(self.groups);
                let groups_end = keys_groups.len();

                Key {
                    action: self.action,
                    combos: Range::new(combos_start, combos_end),
                    active_combo: None,
                    latching: self.latching,
                    immediate: self.immediate,
                    groups: Range::new(groups_start, groups_end),
                    open: false,
                    cache_counter: 0,
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

        impl MutGroup {
            fn freeze(
                self,
                groups_pred: &mut Vec<usize>,
                groups_intersect: &mut Vec<usize>,
                groups_keys: &mut Vec<usize>,
            ) -> Group {
                let pred_start = groups_pred.len();
                groups_pred.extend(self.pred);
                let pred_end = groups_pred.len();

                let intersect_start = groups_intersect.len();
                groups_intersect.extend(self.intersect);
                let intersect_end = groups_intersect.len();

                let keys_start = groups_keys.len();
                groups_keys.extend(self.keys);
                let keys_end = groups_keys.len();
                let keys = Range::new(keys_start, keys_end);

                Group {
                    index: self.index,
                    greater: self.greater.into_iter().collect(),
                    pred: Range::new(pred_start, pred_end),
                    intersect: Range::new(intersect_start, intersect_end),
                    active_combos: SetUsize::new(),
                    counter: 0,
                    size: keys.len(),
                    keys,
                    mask: self.mask,
                    mask_weight: 0,
                }
            }
        }

        // graph build
        let (named_groups, groups): (HashMap<String, usize>, Vec<HashSet<A>>) = config
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

        let mut domain: HashMap<A, usize> = HashMap::new();
        let mut temp_keys: Vec<MutKey<Z>> = vec![];
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

        let mut groups_keys = vec![];
        let mut pred_adjacency = vec![];
        let mut intersect_adjacency = vec![];
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
            .map(|group| {
                group.freeze(
                    &mut pred_adjacency,
                    &mut intersect_adjacency,
                    &mut groups_keys,
                )
            })
            .collect();

        for group in 0..groups.len() {
            groups[group].mask_weight = groups[group].mask as i32
                - groups[group]
                    .iter_pred(&pred_adjacency)
                    .map(|group| groups[*group].mask as i32)
                    .sum::<i32>();
        }

        // domain: populate action keys
        for action in config.actions.iter() {
            let temp_key: &mut MutKey<Z>;
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
        let mut keys_combos = vec![];
        let mut keys_groups = vec![];

        ComboHandler {
            domain: FzScalarMap::new(domain.into_iter().collect()),
            keys: temp_keys
                .into_iter()
                .map(|key| key.freeze(&groups, &mut keys_combos, &mut keys_groups))
                .collect(),
            keys_combos: keys_combos.into_boxed_slice(),
            keys_groups: keys_groups.into_boxed_slice(),
            groups,
            groups_keys: groups_keys.into_boxed_slice(),
            groups_pred: pred_adjacency.into_boxed_slice(),
            groups_intersect: intersect_adjacency.into_boxed_slice(),
            events: VecDeque::with_capacity(EVENT_BUFFER_WARMUP),
            masks: 0,
            cache_counter: 1,
        }
    }

    pub fn handle(&mut self, event: Event<A>) -> &mut VecDeque<Event<Z>> {
        let key = *if let Some(key) = self.domain.get(&event.keycode) {
            key
        } else {
            return &mut self.events;
        };
        match event.kind {
            Kind::Down => {
                let mut invalidate_cache = false;
                // modifier key
                self.keys[key].open = true;
                for group in self.keys[key].iter_groups(&self.keys_groups) {
                    // increase group counter
                    self.groups[*group].counter += 1;
                    if self.groups[*group].is_active() {
                        // for every just activated group
                        self.masks += self.groups[*group].mask_weight;
                        invalidate_cache = true;
                        if self.groups[*group].keys.len() > 1 {
                            // singletons do not close themselves
                            for key in self.groups[*group].iter_keys(&self.groups_keys) {
                                // close all delayed modifier keys
                                self.keys[*key].open &= self.keys[*key].is_immediate();
                            }
                        }
                        for group in self.groups[*group].iter_pred(&self.groups_pred) {
                            close_active_combos(
                                &mut self.groups[*group],
                                &self.keys,
                                &self.keys_combos,
                                &mut self.events,
                            );
                        }
                    }
                }

                if invalidate_cache {
                    self.cache_counter = self.cache_counter.wrapping_add(1);
                }

                // optimization: skip conflict resolution on closed keyup modifier keys
                if !self.keys[key].is_immediate() && !self.keys[key].open {
                    return &mut self.events;
                }

                self.keys[key].open &= !self.is_masking();

                if self.keys[key].cache_counter == self.cache_counter {
                    if !self.is_masking()
                        && self.keys[key].is_immediate()
                        && let Some(action) = self.keys[key]
                            .active_combo
                            .map(|i| self.keys[key].get_combo(i, &self.keys_combos).action)
                            .or(self.keys[key].action)
                    {
                        self.events.push_back(Event {
                            keycode: action,
                            kind: Kind::Down,
                            value: 0,
                        });
                        self.keys[key].open = true;
                    }
                    return &mut self.events;
                }
                self.keys[key].cache_counter = self.cache_counter;

                // action key
                let combos = self.keys[key].combos.len();
                let mut i = self.keys[key]
                    .iter_combos(&self.keys_combos)
                    .position(|combo| self.groups[combo.modifier_group].is_active())
                    .unwrap_or(combos);
                if i == combos {
                    // not modified
                    self.maybe_action(key);
                    return &mut self.events;
                }

                let candidate = i;

                // search action key conflicts
                while i < combos {
                    if self.groups[self.keys[key]
                        .get_combo(i, &self.keys_combos)
                        .modifier_group]
                        .is_active()
                        && !(self.groups[self.keys[key]
                            .get_combo(i, &self.keys_combos)
                            .modifier_group]
                            <= self.groups[self.keys[key]
                                .get_combo(candidate, &self.keys_combos)
                                .modifier_group])
                    {
                        self.maybe_action(key);
                        return &mut self.events;
                    }
                    i += 1;
                }

                // search modifier key conflicts
                let conflict: bool = self.groups[self.keys[key].get_combo(candidate, &self.keys_combos).modifier_group]
                    .greater
                    .iter()
                    .any(|group| self.groups[*group].is_active()) // no active supergroups
                    || self.groups[self.keys[key].get_combo(candidate, &self.keys_combos).modifier_group]
                    .iter_intersect(&self.groups_intersect)
                    .any(|group| self.groups[*group].is_active()); // no active intersecting groups
                if conflict {
                    if self.is_masking()
                        && self.keys[key].is_immediate()
                        && let Some(action) = self.keys[key].action
                    {
                        self.events.push_back(Event {
                            keycode: action,
                            kind: Kind::Down,
                            value: 0,
                        });
                        self.keys[key].active_combo = None;
                        self.keys[key].open = true;
                    }
                    return &mut self.events;
                }

                // activate combo
                if !self.keys[key].latching {
                    self.groups[self.keys[key]
                        .get_combo(candidate, &self.keys_combos)
                        .modifier_group]
                        .active_combos
                        .insert(key);
                }
                if self.keys[key].is_immediate() {
                    self.events.push_back(Event {
                        keycode: self.keys[key]
                            .get_combo(candidate, &self.keys_combos)
                            .action,
                        kind: Kind::Down,
                        value: 0,
                    });
                    self.keys[key].open = true;
                }
                self.keys[key].active_combo = Some(candidate);
            }
            Kind::Up => {
                for group in self.keys[key].iter_groups(&self.keys_groups) {
                    if self.groups[*group].is_active() {
                        self.masks -= self.groups[*group].mask_weight;
                    }
                    close_active_combos(
                        &mut self.groups[*group],
                        &self.keys,
                        &self.keys_combos,
                        &mut self.events,
                    );
                    self.groups[*group].counter -= 1;
                }
                if self.keys[key].open
                    && let Some(action) = self.keys[key]
                        .active_combo
                        .map(|i| self.keys[key].get_combo(i, &self.keys_combos).action)
                        .or(self.keys[key].action)
                {
                    if !self.keys[key].is_immediate() {
                        self.events.push_back(Event {
                            keycode: action,
                            kind: Kind::Down,
                            value: 0,
                        });
                    }
                    self.events.push_back(Event {
                        keycode: action,
                        kind: Kind::Up,
                        value: 0,
                    });
                }
            }
            Kind::Axis => {}
        }
        &mut self.events
    }

    fn maybe_action(&mut self, key: usize) {
        if !self.is_masking()
            && self.keys[key].is_immediate()
            && let Some(action) = self.keys[key].action
        {
            self.events.push_back(Event {
                keycode: action,
                kind: Kind::Down,
                value: 0,
            });
            self.keys[key].active_combo = None;
            self.keys[key].open = true;
        }
    }
}

fn close_active_combos<Z: Keycode>(
    group: &mut Group,
    keys: &[Key<Z>],
    keys_combos: &[Combo<Z>],
    events: &mut VecDeque<Event<Z>>,
) {
    for key in group.active_combos.drain() {
        // terminate the actions it modified
        // keyup modifiers did not produce a keydown yet
        if keys[key].is_immediate()
            && let Some(action) = keys[key].active_combo
        {
            // ignore modifiers with keyup action
            events.push_back(Event {
                keycode: keys[key].get_combo(action, keys_combos).action,
                kind: Kind::Up,
                value: 0,
            });
        }
    }
}
