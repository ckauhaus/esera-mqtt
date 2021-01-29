use crate::MqttMsg;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

pub type Token = i32;
pub type Id<I> = (I, Token);

// I: Index type, e.g. i32 or (usize, usize)
#[derive(Debug)]
pub struct Routes<I: Debug> {
    by_topic: HashMap<String, Vec<Id<I>>>,
}

impl<I: Debug> Routes<I> {
    pub fn new() -> Self {
        Self {
            by_topic: HashMap::new(),
        }
    }
}

impl<I: Eq + Hash + Debug> Routes<I> {
    /// Adds subscription topic to the registry. If a specific topic has been mentioned for the
    /// first time, a MQTT subscribe message is emitted.
    pub fn register(&mut self, topic: String, id: Id<I>) -> Option<MqttMsg> {
        if let Some(e) = self.by_topic.get_mut(&topic) {
            e.push(id);
            None
        } else {
            self.by_topic.insert(topic.clone(), vec![id]);
            Some(MqttMsg::Sub { topic })
        }
    }

    /// Returns recipients list if a topic is found and an empty slice otherwise.
    ///
    /// # Example
    /// for (idx, token) in routes.lookup(topic) {
    ///   dev[idx].process(token)
    /// }
    pub fn lookup(&self, topic: &str) -> &[Id<I>] {
        if let Some(elem) = self.by_topic.get(topic) {
            elem
        } else {
            &[]
        }
    }

    pub fn subscriptions(&self) -> impl Iterator<Item = MqttMsg> + '_ {
        self.by_topic.keys().map(|t| MqttMsg::Sub {
            topic: t.to_owned(),
        })
    }
}

impl<I: Debug> Default for Routes<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: Eq + Hash + Debug> Deref for Routes<I> {
    type Target = HashMap<String, Vec<Id<I>>>;

    fn deref(&self) -> &Self::Target {
        &self.by_topic
    }
}

impl<I: Eq + Hash + Debug> DerefMut for Routes<I> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.by_topic
    }
}
