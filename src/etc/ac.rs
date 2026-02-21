use crate::etc::sub::Subject;
use lru::LruCache;
use std::{cell::RefCell, collections::HashMap, num::NonZeroUsize};

const DECISION_CACHE_CAPACITY: usize = 1024;

#[derive(Clone, Hash, Eq, PartialEq)]
struct DecisionKey {
    sub_type: Box<str>,
    resource: Box<str>,
    action: Option<Box<str>>,
    attrs_signature: Box<str>,
}

struct DecisionCache {
    version: u64,
    entries: LruCache<DecisionKey, bool>,
}

impl DecisionCache {
    fn new(version: u64) -> Self {
        Self {
            version,
            entries: LruCache::new(NonZeroUsize::new(DECISION_CACHE_CAPACITY).unwrap()),
        }
    }

    fn reset(&mut self, version: u64) {
        self.version = version;
        self.entries.clear();
    }

    fn get(&mut self, key: &DecisionKey) -> Option<bool> {
        self.entries.get(key).copied()
    }

    fn insert(&mut self, key: DecisionKey, allowed: bool) {
        self.entries.put(key, allowed);
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
    env: &Env,
    resource: &str,
) -> bool {
    let sub_type = subject.sub_type.as_str();
    let (resource_name, resource_action) = parse_resource(resource);
    let action_key = resource_action
        .as_ref()
        .map(|action| action.to_string().into_boxed_str());
    let attrs_signature = attrs_signature(subject);
    let key = DecisionKey {
        sub_type: Box::from(sub_type),
        resource: resource_name.clone().into_boxed_str(),
        action: action_key,
        attrs_signature,
    };

    let version = crate::etc::gate::get_config_version();
    if let Some(allowed) = with_cache(version, |cache| cache.get(&key)) {
        return allowed;
    }

    let context = create_context(subject, env);
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

fn attrs_signature(subject: &Subject) -> Box<str> {
    if subject.attrs.is_null() {
        return Box::from("");
    }
    if let Some(attrs) = subject.attrs.as_object() {
        if attrs.is_empty() {
            return Box::from("");
        }
    }
    serde_json::to_string(&subject.attrs)
        .unwrap_or_default()
        .into_boxed_str()
}

fn create_context(sub: &Subject, env: &Env) -> HashMap<String, ace::Value> {
    let attrs = sub.attrs.as_object();
    if attrs.is_none() {
        return ace::ContextBuilder::new().build();
    }
    let attrs = attrs.unwrap();
    let mut entries = HashMap::with_capacity(attrs.len() + env.len());
    let sub_type = sub.sub_type.as_str();
    attrs.iter().for_each(|(k, v)| {
        let key = format!("{}.{}", sub_type, k);
        let value = ace::Value::from(v.clone());
        entries.insert(key, value);
    });

    env.iter().for_each(|(k, v)| {
        let key = format!("env.{}", k);
        // FIXME: this is a bit hacky, we should have a proper way to convert env values to ace::Value
        let value = ace::Value::from(serde_json::Value::String(v.to_string()));
        entries.insert(key, value);
    });

    entries
}

#[derive(Debug, Clone, Default)]
pub struct Env {
    pub ip_address: Box<str>,
    pub user_agent: Box<str>,
    pub country_code: Box<str>,
    pub country_name: Box<str>,
    pub city_name: Box<str>,
}

impl Env {
    fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        [
            ("ip_address", self.ip_address.as_ref()),
            ("user_agent", self.user_agent.as_ref()),
            ("country_code", self.country_code.as_ref()),
            ("country_name", self.country_name.as_ref()),
            ("city_name", self.city_name.as_ref()),
        ]
        .into_iter()
    }

    fn len(&self) -> usize {
        5
    }
}
