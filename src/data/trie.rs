use crate::data::config::Service;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct TriePathNode {
    service: Option<Service>,
    children: HashMap<String, TriePathNode>,
    is_end: bool,
}

#[derive(Debug, Default)]
pub struct TriePath {
    root: TriePathNode,
}

impl TriePath {
    pub fn new() -> Self {
        TriePath {
            root: TriePathNode::default(),
        }
    }

    pub fn insert(&mut self, path: &str, service: Service) {
        let mut node = &mut self.root;
        for segment in path.trim_start_matches('/').split('/') {
            node = node
                .children
                .entry(segment.to_string())
                // .or_insert(TriePathNode::default());
                .or_default();
        }
        node.is_end = true;
        node.service = Some(service);
    }

    pub fn search(&self, path: &str) -> Option<&Service> {
        let mut node = &self.root;
        let mut last_service: Option<&Service> = None;

        for segment in path.trim_start_matches('/').split('/') {
            if let Some(next_node) = node.children.get(segment) {
                node = next_node;
                if node.is_end {
                    last_service = node.service.as_ref();
                }
            } else {
                break;
            }
        }
        last_service
    }
}
