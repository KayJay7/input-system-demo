use crate::types::Keycode;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config<A:Keycode, Z: Keycode> {
    #[serde(default = "Vec::default")]
    pub modifiers: Vec<ModifierDecl<A>>,
    #[serde(default = "Vec::default")]
    pub actions: Vec<Action<A, Z>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Action<A:Keycode, Z:Keycode> {
    pub key: A,
    #[serde(default = "Option::default")]
    pub action: Option<Z>,
    #[serde(default = "bool::default")]
    pub immediate: bool,
    #[serde(default = "Vec::default")]
    pub modified: Vec<Combo<Z>>,
    #[serde(default = "bool::default")]
    pub latching: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Combo<Z:Keycode> {
    pub modifier: String,
    pub action: Z,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ModifierDecl<A:Keycode> {
    #[serde(rename = "name")]
    pub id: String,
    pub keys: HashSet<A>,
    #[serde(default = "bool::default")]
    pub masking: bool,
}

impl<A:Keycode, Z:Keycode> Config<A, Z> {
    pub fn validate(&self) -> Result<(), String> {
        let mut ids: HashMap<_, Vec<_>> = HashMap::new();
        let mut modifier_keys: HashSet<A> = HashSet::new();
        let mut groups = HashSet::new();
        for modifier in &self.modifiers {
            if ids
                .insert(modifier.id.clone(), modifier.keys.iter().collect())
                .is_some()
            {
                Err(format!("duplicate modifiers for \"{}\"", modifier.id))?;
            }
            let mut group: Vec<A> = modifier.keys.iter().cloned().collect();
            group.sort_unstable();
            if !groups.insert(group) {
                Err(format!("duplicate modifiers for \"{}\"", modifier.id))?;
            }
            modifier_keys.extend(modifier.keys.iter());
        }

        let mut keys = HashSet::new();
        for action in &self.actions {
            if !keys.insert(action.key) {
                Err(format!("duplicate action for \"{:?}\"", action.key))?;
            }
            if action.immediate && !modifier_keys.contains(&action.key) {
                Err(format!(
                    "`immediate` only applies to modifier in key \"{:?}\"",
                    action.key
                ))?;
            }
            let mut modifiers = HashSet::new();
            for combo in &action.modified {
                if let Some(group) = ids.get(&combo.modifier) {
                    if group.contains(&&action.key) {
                        Err(format!("key \"{:?}\" is a modifier to itself", action.key))?;
                    }
                } else {
                    Err(format!(
                        "undefined modifier \"{}\" in key \"{:?}\"",
                        &combo.modifier, action.key
                    ))?;
                }
                if !modifiers.insert(combo.modifier.clone()) {
                    Err(format!(
                        "duplicate modifier \"{}\" in key \"{:?}\"",
                        &combo.modifier, action.key
                    ))?;
                }
            }
        }
        Ok(())
    }
}
