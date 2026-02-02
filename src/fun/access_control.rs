use crate::etc::sub::Subject;
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
};

const DECISION_CACHE_CAPACITY: usize = 1024;

#[derive(Clone, Hash, Eq, PartialEq)]
struct DecisionKey {
    sub_type: String,
    resource: String,
    action: Option<String>,
    attrs_signature: String,
}

struct DecisionCache {
    version: u64,
    entries: HashMap<DecisionKey, bool>,
    order: VecDeque<DecisionKey>,
}

impl DecisionCache {
    fn new(version: u64) -> Self {
        Self {
            version,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn reset(&mut self, version: u64) {
        self.version = version;
        self.entries.clear();
        self.order.clear();
    }

    fn get(&self, key: &DecisionKey) -> Option<bool> {
        self.entries.get(key).copied()
    }

    fn insert(&mut self, key: DecisionKey, allowed: bool) {
        let is_new = self.entries.insert(key.clone(), allowed).is_none();
        if is_new {
            self.order.push_back(key);
        }
        self.evict_if_needed();
    }

    fn evict_if_needed(&mut self) {
        while self.entries.len() > DECISION_CACHE_CAPACITY {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
    }
}

thread_local! {
    static DECISION_CACHE: RefCell<DecisionCache> = RefCell::new(DecisionCache::new(0));
}

fn with_cache<F, R>(version: u64, f: F) -> R
where
    F: FnOnce(&mut DecisionCache) -> R,
{
    DECISION_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.version != version {
            cache.reset(version);
        }
        f(&mut cache)
    })
}

pub fn access_control(
    policy_engine: &ace::PolicyEngine,
    subject: &Subject,
    resource: &str,
) -> bool {
    let sub_type = subject.sub_type.as_str();
    let (resource_name, resource_action) = parse_resource(resource);
    let action_key = resource_action.as_ref().map(|action| action.to_string());
    let attrs_signature = attrs_signature(subject);
    let key = DecisionKey {
        sub_type: sub_type.to_string(),
        resource: resource_name.clone(),
        action: action_key,
        attrs_signature,
    };

    let version = crate::etc::gate::get_config_version();
    if let Some(allowed) = with_cache(version, |cache| cache.get(&key)) {
        return allowed;
    }

    let context = create_context(subject);
    let allowed = match resource_action {
        None => policy_engine.evaluate(&sub_type, &resource_name, &context),
        Some(action) => {
            policy_engine.evaluate_with_action(&sub_type, &resource_name, &action, &context)
        }
    };

    with_cache(version, |cache| cache.insert(key, allowed));
    allowed
}

fn parse_resource(resource: &str) -> (String, Option<ace::ResourceAction>) {
    if let Some(colon_pos) = resource.find(':') {
        let (res_name, action_str) = resource.split_at(colon_pos);
        let action_str = &action_str[1..]; // Remove the colon
        let action = ace::ResourceAction::from_str(action_str);
        return (res_name.to_string(), action);
    }
    (resource.to_string(), None)
}

fn attrs_signature(subject: &Subject) -> String {
    if subject.attrs.is_null() {
        return String::new();
    }
    if let Some(attrs) = subject.attrs.as_object() {
        if attrs.is_empty() {
            return String::new();
        }
    }
    serde_json::to_string(&subject.attrs).unwrap_or_default()
}

fn create_context(sub: &Subject) -> HashMap<String, ace::Value> {
    let attrs = sub.attrs.as_object();
    if attrs.is_none() {
        return ace::ContextBuilder::new().build();
    }
    let attrs = attrs.unwrap();
    let mut entries = HashMap::with_capacity(attrs.len());
    let sub_type = sub.sub_type.as_str();
    attrs.iter().for_each(|(k, v)| {
        let key = format!("{}.{}", sub_type, k);
        let value = ace::Value::from(v.clone());
        entries.insert(key, value);
    });

    entries
}
