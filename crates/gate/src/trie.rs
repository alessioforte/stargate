//! Trie-based path routing for service discovery.
//!
//! This module implements a prefix tree (trie) data structure optimized for
//! API gateway routing. It enables efficient longest-prefix matching of URL paths
//! to backend services, with support for multiple protocols (HTTP, HTTPS, WS, WSS).
//!
//! # Algorithm Characteristics
//!
//! - **Time Complexity**: O(m) for both insert and search, where m is the number of path segments
//! - **Space Complexity**: O(n * m) where n is the number of services and m is average path depth
//! - **Thread Safety**: Read-only operations are thread-safe when wrapped in Arc
//! - **Matching Strategy**: Longest prefix matching (returns the most specific matching service)
//!
//! # Example
//!
//! ```ignore
//! use gate::trie::TriePath;
//!
//! let mut trie = TriePath::new();
//!
//! // Register services
//! trie.insert("http", "/api", api_service);
//! trie.insert("http", "/api/v1", api_v1_service);
//! trie.insert("http", "/api/v1/users", users_service);
//!
//! // Search returns longest matching prefix
//! assert_eq!(trie.search("http", "/api/v1/users/123").unwrap().name, "users");
//! assert_eq!(trie.search("http", "/api/v1/orders").unwrap().name, "api_v1");
//! assert_eq!(trie.search("http", "/api/v2").unwrap().name, "api");
//! ```

use crate::protocol::Protocol;
use lb::LoadBalancer;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::warn;

/// Metadata for a specific route within a service.
///
/// Used for fine-grained routing control at the HTTP method + path level.
#[derive(Default, Debug)]
pub struct RouteNode {
    /// Whether authentication is required for this specific route
    pub auth_required: bool,
    /// Optional resource identifier for authorization (e.g., "users:read")
    pub resource: Option<String>,
}

/// A service registration containing routing and load balancing configuration.
///
/// This structure represents a backend service that can handle requests
/// matching a specific path prefix.
pub struct Service {
    /// Human-readable service name for logging and debugging
    pub name: String,
    /// The path prefix this service handles (e.g., "/api/v1")
    pub path: String,
    /// Load balancer for distributing requests across backend instances
    pub lb: Option<Arc<dyn LoadBalancer + Send + Sync>>,
    /// Whether authentication is required at the service level
    pub auth_required: Option<bool>,
    /// Optional resource identifier for service-level authorization
    pub resource: Option<String>,
    /// Fine-grained routing rules by HTTP method and path
    pub routes: Option<HashMap<String, matchit::Router<RouteNode>>>,
}

/// Internal node in the trie structure.
///
/// Each node represents a path segment and may contain a service
/// registration if it marks the end of a registered path.
#[derive(Default)]
struct TriePathNode {
    /// Service registered at this path (if any)
    service: Option<Service>,
    /// Child nodes indexed by path segment
    children: HashMap<String, TriePathNode>,
    /// Whether this node marks the end of a registered path
    is_end: bool,
}

/// A trie-based data structure for efficient path-to-service routing.
///
/// `TriePath` implements a prefix tree that organizes services by protocol and path,
/// enabling O(m) lookup time where m is the number of path segments.
///
/// # Protocol Separation
///
/// Services are organized first by protocol (http, https, ws, wss) at the root level,
/// then by path segments. This ensures clean separation and prevents protocol confusion.
///
/// # Longest Prefix Matching
///
/// The search algorithm returns the **longest matching prefix** rather than requiring
/// an exact match. This is intentional for API gateway use cases where a service
/// registered at `/api` should handle all requests to `/api/*`.
///
/// # Path Handling
///
/// - Leading slashes are trimmed: "/api" and "api" are treated the same
/// - Trailing slashes create different structures: "/api" vs "/api/"
/// - Empty segments (from "//") are preserved as empty string keys
/// - Case-sensitive: "/API" and "/api" are different paths
///
/// # Examples
///
/// ```ignore
/// let mut trie = TriePath::new();
///
/// // Register a catch-all service
/// trie.insert("http", "/api", api_service);
///
/// // More specific services take precedence
/// trie.insert("http", "/api/v1", v1_service);
/// trie.insert("http", "/api/v1/users", users_service);
///
/// // Longest prefix wins
/// trie.search("http", "/api/v1/users/123");  // -> users_service
/// trie.search("http", "/api/v1/orders");     // -> v1_service
/// trie.search("http", "/api/v2");            // -> api_service
/// trie.search("http", "/other");             // -> None
/// ```
#[derive(Default)]
pub struct TriePath {
    /// Root nodes indexed by protocol (http, https, ws, wss)
    root: HashMap<String, TriePathNode>,
}

impl TriePath {
    /// Creates a new empty trie.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let trie = TriePath::new();
    /// ```
    pub fn new() -> Self {
        TriePath {
            root: HashMap::new(),
        }
    }

    /// Inserts a service into the trie at the specified protocol and path.
    ///
    /// If a service already exists at the exact same protocol and path,
    /// it will be silently replaced with the new service.
    ///
    /// # Arguments
    ///
    /// * `protocol` - Protocol string: "http", "https", "ws", or "wss"
    /// * `path` - URL path like "/api/v1/users" (leading slash optional)
    /// * `service` - Service configuration to register
    ///
    /// # Behavior
    ///
    /// - Invalid protocols are logged and ignored (no panic)
    /// - Path is split by '/' after trimming leading slashes
    /// - Each segment becomes a node in the trie
    /// - Empty segments (from "//") create empty string nodes
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut trie = TriePath::new();
    /// trie.insert("http", "/api/users", users_service);
    /// trie.insert("https", "/api/users", secure_users_service);
    /// ```
    pub fn insert(&mut self, protocol: &str, path: &str, service: Service) {
        let p = Protocol::from_str(protocol);
        if p.is_none() {
            warn!("Unsupported protocol: {}", protocol);
            return;
        }
        let protocol_node = self
            .root
            .entry(p.unwrap().as_str().to_string())
            .or_default();

        let mut node = protocol_node;
        for segment in path.trim_start_matches('/').split('/') {
            node = node.children.entry(segment.to_string()).or_default();
        }
        node.is_end = true;
        node.service = Some(service);
    }

    /// Searches for a service matching the given protocol and path.
    ///
    /// Returns the service with the **longest matching prefix**. This means
    /// if you search for "/api/v1/users/123" and services are registered at
    /// "/api", "/api/v1", and "/api/v1/users", it will return the service
    /// at "/api/v1/users" (the most specific match).
    ///
    /// # Arguments
    ///
    /// * `protocol` - Protocol to search: "http", "https", "ws", or "wss"
    /// * `path` - URL path to search for
    ///
    /// # Returns
    ///
    /// * `Some(&Service)` - The service with the longest matching prefix
    /// * `None` - No matching service found for this protocol/path combination
    ///
    /// # Matching Behavior
    ///
    /// The search walks the trie node-by-node, keeping track of the last
    /// service encountered. When it can't match any more segments, it returns
    /// the last service found. This implements prefix matching.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Given these registrations:
    /// trie.insert("http", "/api", api_service);
    /// trie.insert("http", "/api/v1", v1_service);
    ///
    /// // These searches return:
    /// trie.search("http", "/api");        // -> Some(api_service)
    /// trie.search("http", "/api/v1");     // -> Some(v1_service)
    /// trie.search("http", "/api/v1/foo"); // -> Some(v1_service) (prefix match)
    /// trie.search("http", "/api/v2");     // -> Some(api_service) (prefix match)
    /// trie.search("http", "/other");      // -> None
    /// trie.search("https", "/api");       // -> None (different protocol)
    /// ```
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

#[cfg(test)]
mod tests {
    use super::*;
    use lb::{BaseLoadBalancer, RoundRobin, Upstream};

    // Helper function to create a test service
    fn create_test_service(name: &str, path: &str) -> Service {
        let upstreams = vec![Upstream::new("http://localhost:8080".to_string(), None)];
        let lb = BaseLoadBalancer::new(RoundRobin::new(), upstreams);
        Service {
            name: name.to_string(),
            path: path.to_string(),
            lb: Some(lb),
            auth_required: Some(false),
            resource: None,
            routes: None,
        }
    }

    #[test]
    fn test_new_trie_is_empty() {
        let trie = TriePath::new();
        assert!(trie.search("http", "/api").is_none());
        assert!(trie.search("https", "/api").is_none());
    }

    #[test]
    fn test_insert_and_search_basic() {
        let mut trie = TriePath::new();
        let service = create_test_service("api", "/api");
        trie.insert("http", "/api", service);

        let result = trie.search("http", "/api");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "api");
    }

    #[test]
    fn test_insert_multiple_protocols() {
        let mut trie = TriePath::new();
        let http_service = create_test_service("http-api", "/api");
        let https_service = create_test_service("https-api", "/api");

        trie.insert("http", "/api", http_service);
        trie.insert("https", "/api", https_service);

        let http_result = trie.search("http", "/api");
        let https_result = trie.search("https", "/api");

        assert!(http_result.is_some());
        assert!(https_result.is_some());
        assert_eq!(http_result.unwrap().name, "http-api");
        assert_eq!(https_result.unwrap().name, "https-api");
    }

    #[test]
    fn test_insert_websocket_protocols() {
        let mut trie = TriePath::new();
        let ws_service = create_test_service("ws-api", "/ws");
        let wss_service = create_test_service("wss-api", "/wss");

        trie.insert("ws", "/ws", ws_service);
        trie.insert("wss", "/wss", wss_service);

        assert!(trie.search("ws", "/ws").is_some());
        assert!(trie.search("wss", "/wss").is_some());
        assert_eq!(trie.search("ws", "/ws").unwrap().name, "ws-api");
        assert_eq!(trie.search("wss", "/wss").unwrap().name, "wss-api");
    }

    #[test]
    fn test_insert_invalid_protocol() {
        let mut trie = TriePath::new();
        let service = create_test_service("api", "/api");

        // Should not panic, just log warning
        trie.insert("ftp", "/api", service);

        // Verify it wasn't inserted
        assert!(trie.search("ftp", "/api").is_none());
    }

    #[test]
    fn test_search_non_existent_protocol() {
        let trie = TriePath::new();
        assert!(trie.search("http", "/api").is_none());
    }

    #[test]
    fn test_search_non_existent_path() {
        let mut trie = TriePath::new();
        let service = create_test_service("api", "/api");
        trie.insert("http", "/api", service);

        // Paths with no matching prefix
        assert!(trie.search("http", "/users").is_none());

        // /api/v2 actually matches /api due to prefix matching
        // This is intentional behavior for API gateway routing
        assert!(trie.search("http", "/api/v2").is_some());
        assert_eq!(trie.search("http", "/api/v2").unwrap().name, "api");
    }

    #[test]
    fn test_prefix_matching() {
        let mut trie = TriePath::new();
        let service = create_test_service("api", "/api");
        trie.insert("http", "/api", service);

        // Should return /api service for any path starting with /api
        let result = trie.search("http", "/api/users");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "api");

        let result = trie.search("http", "/api/users/123");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "api");
    }

    #[test]
    fn test_longest_prefix_matching() {
        let mut trie = TriePath::new();
        let api_service = create_test_service("api", "/api");
        let api_v1_service = create_test_service("api-v1", "/api/v1");
        let api_v1_users_service = create_test_service("api-v1-users", "/api/v1/users");

        trie.insert("http", "/api", api_service);
        trie.insert("http", "/api/v1", api_v1_service);
        trie.insert("http", "/api/v1/users", api_v1_users_service);

        // Should return most specific match
        assert_eq!(trie.search("http", "/api").unwrap().name, "api");
        assert_eq!(trie.search("http", "/api/v1").unwrap().name, "api-v1");
        assert_eq!(
            trie.search("http", "/api/v1/users").unwrap().name,
            "api-v1-users"
        );

        // Should return longest prefix for non-exact matches
        assert_eq!(trie.search("http", "/api/v2").unwrap().name, "api");
        assert_eq!(
            trie.search("http", "/api/v1/orders").unwrap().name,
            "api-v1"
        );
        assert_eq!(
            trie.search("http", "/api/v1/users/123").unwrap().name,
            "api-v1-users"
        );
    }

    #[test]
    fn test_shared_prefix_efficiency() {
        let mut trie = TriePath::new();

        // All share /api/v1 prefix
        trie.insert(
            "http",
            "/api/v1/users",
            create_test_service("users", "/api/v1/users"),
        );
        trie.insert(
            "http",
            "/api/v1/orders",
            create_test_service("orders", "/api/v1/orders"),
        );
        trie.insert(
            "http",
            "/api/v1/products",
            create_test_service("products", "/api/v1/products"),
        );

        assert_eq!(trie.search("http", "/api/v1/users").unwrap().name, "users");
        assert_eq!(
            trie.search("http", "/api/v1/orders").unwrap().name,
            "orders"
        );
        assert_eq!(
            trie.search("http", "/api/v1/products").unwrap().name,
            "products"
        );
    }

    #[test]
    fn test_trailing_slash_behavior() {
        let mut trie = TriePath::new();
        let service = create_test_service("api", "/api");
        trie.insert("http", "/api", service);

        // Without trailing slash
        assert!(trie.search("http", "/api").is_some());

        // With trailing slash - this creates different path structure
        // /api/ splits into ["api", ""] vs /api splits into ["api"]
        let result = trie.search("http", "/api/");
        assert!(result.is_some()); // Still matches due to prefix behavior
    }

    #[test]
    fn test_root_path() {
        let mut trie = TriePath::new();
        let service = create_test_service("root", "/");
        trie.insert("http", "/", service);

        let result = trie.search("http", "/");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "root");
    }

    #[test]
    fn test_empty_path() {
        let mut trie = TriePath::new();
        let service = create_test_service("empty", "");
        trie.insert("http", "", service);

        let result = trie.search("http", "");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "empty");
    }

    #[test]
    fn test_path_without_leading_slash() {
        let mut trie = TriePath::new();
        let service = create_test_service("api", "api");
        trie.insert("http", "api", service);

        // Should work the same as /api
        let result = trie.search("http", "api");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "api");
    }

    #[test]
    fn test_overwrite_existing_service() {
        let mut trie = TriePath::new();
        let service1 = create_test_service("api-v1", "/api");
        let service2 = create_test_service("api-v2", "/api");

        trie.insert("http", "/api", service1);
        trie.insert("http", "/api", service2);

        // Should have the second service
        let result = trie.search("http", "/api");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "api-v2");
    }

    #[test]
    fn test_case_sensitivity() {
        let mut trie = TriePath::new();
        let lower_service = create_test_service("lower", "/api");
        let upper_service = create_test_service("upper", "/API");

        trie.insert("http", "/api", lower_service);
        trie.insert("http", "/API", upper_service);

        // Paths are case-sensitive
        assert_eq!(trie.search("http", "/api").unwrap().name, "lower");
        assert_eq!(trie.search("http", "/API").unwrap().name, "upper");
        assert!(trie.search("http", "/Api").is_none());
    }

    #[test]
    fn test_special_characters_in_path() {
        let mut trie = TriePath::new();
        let service = create_test_service("special", "/api/v1.0/users-list");
        trie.insert("http", "/api/v1.0/users-list", service);

        let result = trie.search("http", "/api/v1.0/users-list");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "special");
    }

    #[test]
    fn test_deep_nesting() {
        let mut trie = TriePath::new();
        let deep_path = "/a/b/c/d/e/f/g/h/i/j";
        let service = create_test_service("deep", deep_path);
        trie.insert("http", deep_path, service);

        let result = trie.search("http", deep_path);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "deep");

        // Prefix should still work
        let result = trie.search("http", "/a/b/c/d/e/f/g/h/i/j/k");
        assert!(result.is_some());
    }

    #[test]
    fn test_multiple_services_different_depths() {
        let mut trie = TriePath::new();

        trie.insert("http", "/", create_test_service("root", "/"));
        trie.insert("http", "/api", create_test_service("api", "/api"));
        trie.insert("http", "/api/v1", create_test_service("v1", "/api/v1"));
        trie.insert(
            "http",
            "/api/v1/users",
            create_test_service("users", "/api/v1/users"),
        );
        trie.insert(
            "http",
            "/api/v1/users/profile",
            create_test_service("profile", "/api/v1/users/profile"),
        );

        // Test exact matches
        assert_eq!(trie.search("http", "/").unwrap().name, "root");
        assert_eq!(trie.search("http", "/api").unwrap().name, "api");
        assert_eq!(trie.search("http", "/api/v1").unwrap().name, "v1");
        assert_eq!(trie.search("http", "/api/v1/users").unwrap().name, "users");
        assert_eq!(
            trie.search("http", "/api/v1/users/profile").unwrap().name,
            "profile"
        );

        // Test prefix matches - these return the longest matching prefix
        // Note: "/unknown" doesn't start with any registered path after "/"
        // The root "/" is registered, so paths split as ["unknown"]
        // which don't match any children of root, but root itself matches empty path
        let result = trie.search("http", "/unknown");
        // Actually, "/unknown" splits to ["unknown"] which doesn't match root's children
        // Root is at [""] path, so this won't match
        assert!(result.is_none());

        assert_eq!(trie.search("http", "/api/unknown").unwrap().name, "api");
        assert_eq!(trie.search("http", "/api/v1/orders").unwrap().name, "v1");
        assert_eq!(
            trie.search("http", "/api/v1/users/settings").unwrap().name,
            "users"
        );
    }

    #[test]
    fn test_no_match_with_similar_paths() {
        let mut trie = TriePath::new();
        trie.insert(
            "http",
            "/api/users",
            create_test_service("users", "/api/users"),
        );

        // /api/user (without 's') should not match
        assert!(trie.search("http", "/api/user").is_none());

        // /ap should not match /api
        assert!(trie.search("http", "/ap").is_none());
    }

    #[test]
    fn test_service_with_auth_and_resource() {
        let mut trie = TriePath::new();
        let upstreams = vec![Upstream::new("http://localhost:8080".to_string(), None)];
        let lb = BaseLoadBalancer::new(RoundRobin::new(), upstreams);

        let service = Service {
            name: "protected".to_string(),
            path: "/protected".to_string(),
            lb: Some(lb),
            auth_required: Some(true),
            resource: Some("admin:read".to_string()),
            routes: None,
        };

        trie.insert("http", "/protected", service);

        let result = trie.search("http", "/protected");
        assert!(result.is_some());
        let found = result.unwrap();
        assert_eq!(found.name, "protected");
        assert_eq!(found.auth_required, Some(true));
        assert_eq!(found.resource, Some("admin:read".to_string()));
    }

    #[test]
    fn test_concurrent_reads() {
        use std::sync::Arc;
        use std::thread;

        let mut trie = TriePath::new();
        trie.insert("http", "/api", create_test_service("api", "/api"));
        trie.insert("http", "/users", create_test_service("users", "/users"));

        let trie = Arc::new(trie);
        let mut handles = vec![];

        // Spawn multiple reader threads
        for i in 0..10 {
            let trie_clone = Arc::clone(&trie);
            let handle = thread::spawn(move || {
                let path = if i % 2 == 0 { "/api" } else { "/users" };
                let result = trie_clone.search("http", path);
                assert!(result.is_some());
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_path_with_consecutive_slashes() {
        let mut trie = TriePath::new();
        let service = create_test_service("api", "/api");
        trie.insert("http", "/api", service);

        // Search with double slash - creates empty segment
        let result = trie.search("http", "/api//users");
        // This will not match /api/users but will still match /api prefix
        assert!(result.is_some());
    }

    #[test]
    fn test_empty_segment_in_middle_of_path() {
        let mut trie = TriePath::new();
        // Insert path with empty segment (double slash)
        let service = create_test_service("weird", "/api//users");
        trie.insert("http", "/api//users", service);

        let result = trie.search("http", "/api//users");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "weird");
    }
}
