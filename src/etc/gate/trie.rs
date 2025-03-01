use crate::etc::gate::config::Service;
use crate::etc::gate::protocols::Protocols;
use std::collections::HashMap;

/// TriePathNode is a node in the trie path tree.
#[derive(Debug, Default)]
pub struct TriePathNode {
    service: Option<Service>,
    children: HashMap<String, TriePathNode>,
    is_end: bool,
}

/// TrieProtocol is a node in the trie protocol tree.
#[derive(Debug, Default)]
pub struct TrieProtocol {
    protocols: HashMap<String, TriePathNode>,
}

/// TriePath is a trie tree for path.
/// The structure is like this:
/// ```rust
/// root
/// |- http
/// |  |- v1
/// |  |  |- users
/// |  |  |  |- is_end: true
/// |  |  |  |- service: Some(Service)
/// |  |  |- is_end: false
/// |  |- is_end: false
/// |- ws
/// |  |- v1
/// |  |  |- users
/// |  |  |  |- is_end: true
/// |  |  |  |- service: Some(Service)
/// |  |  |- is_end: false
/// |  |- is_end: false
/// |- is_end: false
/// ```
/// The trie tree is used to search the service by the path.
#[derive(Debug, Default)]
pub struct TriePath {
    root: TrieProtocol,
}

impl TriePath {
    pub fn new() -> Self {
        TriePath {
            root: TrieProtocol::default(),
        }
    }

    /// Insert a service into the trie tree.
    /// The first parameter is the protocol, the second parameter is the path, and the third parameter is the service.
    /// The function will insert the service into the trie tree.
    pub fn insert(&mut self, protocol: &str, path: &str, service: Service) {
        let protocol = Protocols::from_str(protocol);
        if protocol.is_none() {
            return;
        }
        let protocol_node = self
            .root
            .protocols
            .entry(protocol.unwrap().as_str().to_string())
            .or_default();

        let mut node = protocol_node;
        for segment in path.trim_start_matches('/').split('/') {
            node = node.children.entry(segment.to_string()).or_default();
        }
        node.is_end = true;
        node.service = Some(service);
    }

    /// Search a service from the trie tree.
    /// the first parameter is the protocol, the second parameter is the path.
    /// The function returns the service if found, otherwise None.
    pub fn search(&self, protocol: &str, path: &str) -> Option<&Service> {
        if let Some(protocol_node) = self.root.protocols.get(protocol) {
            let mut node = protocol_node;
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
            return last_service;
        }
        None
    }
}

// use crate::etc::gate::config::Service;
// use std::collections::HashMap;

// #[derive(Debug, Default)]
// pub struct TriePathNode {
//     service: Option<Service>,
//     children: HashMap<String, TriePathNode>,
//     is_end: bool,
// }

// #[derive(Debug, Default)]
// pub struct TriePath {
//     root: TriePathNode,
// }

// impl TriePath {
//     pub fn new() -> Self {
//         TriePath {
//             root: TriePathNode::default(),
//         }
//     }

//     pub fn insert(&mut self, path: &str, service: Service) {
//         let mut node = &mut self.root;
//         for segment in path.trim_start_matches('/').split('/') {
//             node = node.children.entry(segment.to_string()).or_default();
//         }
//         node.is_end = true;
//         node.service = Some(service);
//     }

//     pub fn search(&self, path: &str) -> Option<&Service> {
//         let mut node = &self.root;
//         let mut last_service: Option<&Service> = None;

//         for segment in path.trim_start_matches('/').split('/') {
//             if let Some(next_node) = node.children.get(segment) {
//                 node = next_node;
//                 if node.is_end {
//                     last_service = node.service.as_ref();
//                 }
//             } else {
//                 break;
//             }
//         }
//         last_service
//     }
// }
