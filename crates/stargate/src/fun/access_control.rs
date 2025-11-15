use crate::etc::sub::Subject;
use std::collections::HashMap;

pub fn access_control(
    policy_engine: &ace::PolicyEngine,
    subject: &Subject,
    resource: &str,
) -> bool {
    // TODO: add local cache for access control decisions

    let sub_type = subject.sub_type.clone();
    let context = create_context(subject);
    let mut resource_action = None;
    let resource_name = if let Some(colon_pos) = resource.find(':') {
        let (res_name, action_str) = resource.split_at(colon_pos);
        let action_str = &action_str[1..]; // Remove the colon
        resource_action = ace::ResourceAction::from_str(action_str);
        res_name.to_string()
    } else {
        resource.to_string()
    };
    let allowed = match resource_action {
        None => policy_engine.evaluate(&sub_type, &resource_name, &context),
        Some(action) => {
            policy_engine.evaluate_with_action(&sub_type, &resource_name, &action, &context)
        }
    };

    allowed
}

pub fn create_context(sub: &Subject) -> HashMap<String, ace::Value> {
    let mut entries = HashMap::new();
    let attrs = sub.attrs.as_object();
    if attrs.is_none() {
        return ace::ContextBuilder::new().build();
    }
    let attrs = attrs.unwrap();
    let sub_type = sub.sub_type.clone();
    attrs.iter().for_each(|(k, v)| {
        let key = format!("{}.{}", sub_type, k);
        let value = ace::Value::from(v.clone());
        entries.insert(key, value);
    });

    entries
}
