use crate::etc::auth::subject::Subject;
use chrono::{NaiveDate, NaiveTime};
use lru::LruCache;
use std::{
    cell::RefCell,
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    num::NonZeroUsize,
};

const DECISION_CACHE_CAPACITY: usize = 1024;

#[derive(Clone, Hash, Eq, PartialEq)]
struct DecisionKey {
    sub_type: Box<str>,
    resource: Box<str>,
    action: Option<ace::ResourceAction>,
    // Org context must be part of the key: two subjects with identical attrs
    // but different orgs (or the same subject after an org switch) must not
    // share a cached decision.
    org_id: Option<Box<str>>,
    org_role: Option<Box<str>>,
    attrs_hash: u64,
    env_hash: u64,
}

struct DecisionCache {
    revision: Box<str>,
    entries: LruCache<DecisionKey, bool>,
}

impl DecisionCache {
    fn new(revision: &str) -> Self {
        Self {
            revision: Box::from(revision),
            entries: LruCache::new(NonZeroUsize::new(DECISION_CACHE_CAPACITY).unwrap()),
        }
    }

    fn reset(&mut self, revision: &str) {
        self.revision = Box::from(revision);
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
    static DECISION_CACHE: RefCell<DecisionCache> = RefCell::new(DecisionCache::new(""));
}

fn with_cache<F, R>(revision: &str, f: F) -> R
where
    F: FnOnce(&mut DecisionCache) -> R,
{
    DECISION_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.revision.as_ref() != revision {
            cache.reset(revision);
        }
        f(&mut cache)
    })
}

pub fn access_control(
    policy_engine: &ace::PolicyEngine,
    policy_revision: &str,
    subject: &Subject,
    env: &Env,
    resource: &str,
) -> bool {
    let sub_type = subject.sub_type.as_str();
    let (resource_name, resource_action) = parse_resource(resource);
    let key = DecisionKey {
        sub_type: Box::from(sub_type),
        resource: Box::from(resource_name),
        action: resource_action,
        org_id: subject.org_id.as_deref().map(Box::from),
        org_role: subject.org_role.as_deref().map(Box::from),
        attrs_hash: attrs_hash(subject),
        env_hash: env_hash(env),
    };

    if let Some(allowed) = with_cache(policy_revision, |cache| cache.get(&key)) {
        return allowed;
    }

    let context = create_context(subject, env);
    let allowed = match resource_action {
        None => policy_engine.evaluate(sub_type, resource_name, &context),
        Some(action) => {
            policy_engine.evaluate_with_action(sub_type, resource_name, &action, &context)
        }
    };

    with_cache(policy_revision, |cache| cache.insert(key, allowed));
    allowed
}

fn parse_resource(resource: &str) -> (&str, Option<ace::ResourceAction>) {
    if let Some((res_name, action_str)) = resource.split_once(':') {
        return (res_name, ace::ResourceAction::parse(action_str));
    }
    (resource, None)
}

fn attrs_hash(subject: &Subject) -> u64 {
    if subject.attrs.is_null() {
        return 0;
    }
    if let Some(attrs) = subject.attrs.as_object()
        && attrs.is_empty()
    {
        return 0;
    }
    let mut hasher = DefaultHasher::new();
    hash_json_value(&subject.attrs, &mut hasher);
    hasher.finish()
}

fn env_hash(env: &Env) -> u64 {
    let mut hasher = DefaultHasher::new();
    for (key, value) in env.iter() {
        key.hash(&mut hasher);
        value.hash(&mut hasher);
    }
    hasher.finish()
}

fn hash_json_value(value: &serde_json::Value, hasher: &mut DefaultHasher) {
    match value {
        serde_json::Value::Null => {
            0_u8.hash(hasher);
        }
        serde_json::Value::Bool(v) => {
            1_u8.hash(hasher);
            v.hash(hasher);
        }
        serde_json::Value::Number(v) => {
            2_u8.hash(hasher);
            if let Some(i) = v.as_i64() {
                i.hash(hasher);
            } else if let Some(u) = v.as_u64() {
                u.hash(hasher);
            } else if let Some(f) = v.as_f64() {
                f.to_bits().hash(hasher);
            } else {
                v.to_string().hash(hasher);
            }
        }
        serde_json::Value::String(v) => {
            3_u8.hash(hasher);
            v.hash(hasher);
        }
        serde_json::Value::Array(v) => {
            4_u8.hash(hasher);
            v.len().hash(hasher);
            for item in v {
                hash_json_value(item, hasher);
            }
        }
        serde_json::Value::Object(v) => {
            5_u8.hash(hasher);
            v.len().hash(hasher);
            let mut entries: Vec<_> = v.iter().collect();
            entries.sort_unstable_by_key(|(ka, _)| *ka);
            for (k, item) in entries {
                k.hash(hasher);
                hash_json_value(item, hasher);
            }
        }
    }
}

pub(crate) fn create_context(sub: &Subject, env: &Env) -> HashMap<String, ace::Value> {
    let attrs = sub.attrs.as_object();
    let attrs_len = attrs.map(|a| a.len()).unwrap_or(0);
    let env_len = env.len();
    let org_len = usize::from(sub.org_id.is_some()) + usize::from(sub.org_role.is_some());
    if attrs_len == 0 && env_len == 0 && org_len == 0 {
        return HashMap::new();
    }

    let mut entries = HashMap::with_capacity(attrs_len + env_len + org_len);
    let sub_type = sub.sub_type.as_str();

    if let Some(attrs) = attrs {
        for (k, v) in attrs {
            let mut key = String::with_capacity(sub_type.len() + 1 + k.len());
            key.push_str(sub_type);
            key.push('.');
            key.push_str(k);
            entries.insert(key, ace::Value::from(v));
        }
    }

    // Org context comes from the validated membership, not from subject
    // attrs — inserted after them so an attrs key of the same name cannot
    // shadow it. Yields `user.org_id`/`user.org_role` for user subjects and
    // `api_key.org_id` for key subjects.
    if let Some(org_id) = sub.org_id.as_deref() {
        entries.insert(
            format!("{sub_type}.org_id"),
            ace::Value::String(org_id.to_string()),
        );
    }
    if let Some(org_role) = sub.org_role.as_deref() {
        entries.insert(
            format!("{sub_type}.org_role"),
            ace::Value::String(org_role.to_string()),
        );
    }

    for (k, v) in env.iter() {
        let mut key = String::with_capacity(4 + k.len());
        key.push_str("env.");
        key.push_str(k);
        entries.insert(key, normalize_env_value(k, v));
    }

    entries
}

fn normalize_env_value(key: &str, value: &str) -> ace::Value {
    match key {
        "date" => NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(ace::Value::Date)
            .unwrap_or_else(|_| ace::Value::String(value.to_string())),
        "time" => NaiveTime::parse_from_str(value, "%H:%M:%S")
            .or_else(|_| NaiveTime::parse_from_str(value, "%H:%M"))
            .map(ace::Value::Time)
            .unwrap_or_else(|_| ace::Value::String(value.to_string())),
        _ => ace::Value::String(value.to_string()),
    }
}

#[derive(Debug, Clone, Default)]
pub struct Env {
    pub ip_address: Box<str>,
    pub user_agent: Box<str>,
    pub country_code: Box<str>,
    pub country_name: Box<str>,
    pub city_name: Box<str>,
    pub date: Box<str>,
    pub time: Box<str>,
    pub day_of_week: Box<str>,
}

impl Env {
    fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        [
            ("ip_address", self.ip_address.as_ref()),
            ("user_agent", self.user_agent.as_ref()),
            ("country_code", self.country_code.as_ref()),
            ("country_name", self.country_name.as_ref()),
            ("city_name", self.city_name.as_ref()),
            ("date", self.date.as_ref()),
            ("time", self.time.as_ref()),
            ("day_of_week", self.day_of_week.as_ref()),
        ]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
    }

    fn len(&self) -> usize {
        self.iter().count()
    }
}

#[cfg(test)]
mod tests;
