use crate::protocol::Protocol;
use lb::LoadBalancer;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default, Debug)]
pub struct RouteNode {
    pub auth_required: bool,
    pub resource: Option<String>,
}

#[derive(Default)]
pub struct Service {
    pub name: String,
    pub path: String,
    pub lb: Option<Arc<dyn LoadBalancer + Send + Sync>>,
    pub auth_required: Option<bool>,
    pub resource: Option<String>,
    pub routes: Option<HashMap<String, matchit::Router<RouteNode>>>,
}

#[derive(Default)]
pub struct TriePathNode {
    service: Option<Service>,
    children: HashMap<String, TriePathNode>,
    is_end: bool,
}

/// A TriePath is a data structure that allows for efficient storage and retrieval of paths associated with services.
/// It uses a trie (prefix tree) to store paths, where each node represents a segment of the path.
/// It supports insertion of paths with associated services and searching for services based on protocol and path.
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

    pub fn insert(&mut self, protocol: &str, path: &str, service: Service) {
        let protocol = Protocol::from_str(protocol);
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

    pub fn search(&self, protocol: &str, path: &str) -> Option<&Service> {
        if let Some(mut node) = self.root.get(protocol) {
            let mut last: Option<&Service> = None;

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
