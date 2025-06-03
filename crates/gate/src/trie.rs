use crate::protocols::Protocols;
use lb::LoadBalancer;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default, Debug)]
pub struct RouteNode {
    pub auth_required: bool,
}

#[derive(Default)]
pub struct ServiceNode {
    pub connect_timeout: Option<u64>,
    pub name: Option<String>,
    pub path: String,
    pub auth_required: Option<bool>,
    pub routes: Option<HashMap<String, matchit::Router<RouteNode>>>,
    pub lb: Option<Arc<dyn LoadBalancer + Send + Sync>>,
}

#[derive(Default)]
pub struct TriePathNode {
    service: Option<ServiceNode>,
    children: HashMap<String, TriePathNode>,
    is_end: bool,
}

#[derive(Default)]
pub struct TriePath {
    root: HashMap<String, TriePathNode>,
}

impl TriePath {
    pub fn new() -> Self {
        TriePath {
            root: HashMap::new(),
        }
    }

    pub fn insert(&mut self, protocol: &str, path: &str, service: ServiceNode) {
        let protocol = Protocols::from_str(protocol);
        if protocol.is_none() {
            return;
        }
        let protocol_node = self
            .root
            .entry(protocol.unwrap().as_str().to_string())
            .or_default();

        let mut node = protocol_node;
        for segment in path.trim_start_matches('/').split('/') {
            node = node.children.entry(segment.to_string()).or_default();
        }
        node.is_end = true;
        node.service = Some(service);
    }

    pub fn search(&self, protocol: &str, path: &str) -> Option<&ServiceNode> {
        if let Some(mut node) = self.root.get(protocol) {
            let mut last: Option<&ServiceNode> = None;

            for segment in path.trim_start_matches('/').split('/') {
                if let Some(next_node) = node.children.get(segment) {
                    node = next_node;
                    if node.is_end {
                        last = node.service.as_ref();
                    }
                } else {
                    break;
                }
            }
            return last;
        }
        None
    }
}
