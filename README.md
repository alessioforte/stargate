## Roadmap

- [x] Send Email
- [ ] Send SMS
- [ ] Time-based One-Time Password (TOTP) - Authenticator App
- [ ] MFA (Multi-Factor Authentication)
- [x] Cookie-based authentication
- [x] Password Policies
- [ ] Audit Logs
- [ ] Consider to decouple the authentication from the gateway
- [ ] ? Load Balancer - Implement Round Robin, Least Connections, IP Hash, URL Hash

- [ ] ? Login - verify access from another device and notify user - Handle sessions
- [ ] ? Single Sign-On (SSO) and Single Log-Out (SLO)
- [ ] ? act as oAuth provider
- [ ] ? QR Code - Generate and Scan
- [ ] ? Generate PDF as Infisical does
- [ ] ? Bio-metric Authentication

OAUTH2 PROVIDERS
- [x] oAuth Github
- [x] oAuth Google
- [ ] oAuth Facebook
- [ ] oAuth Instagram
- [ ] oAuth Apple
- [ ] oAuth Linkedin
- [ ] oAuth Microsoft

PROTOCOLS
- [x] HTTP/HTTPS
- [x] WebSockets
- [ ] MQTT - try with rumqtt crate
- [ ] gRPC - try with tonic crate
- [ ] ? FTP/FTPS
- [ ] ? TCP/UDP
- [ ] ? AMQP
- [ ] ? SSE
- [ ] ? SOAP

ACCESS CONTROL
- [ ] Implement Role-Based Access Control (RBAC)
- [ ] Implement Attribute-Based Access Control (ABAC)
- [ ] Implement Policy-Based Access Control (PBAC)
- [ ] Implement Rule-Based Access Control (RBAC)

- [ ] API Keys - Generate, Revoke, List
- [ ] Rate Limiting Global and Per User

PRICING PLANS
- [ ] Implement Pricing Plans Configuration

ADMIN FEATURES
- [x] √ Init basic configuration
- [x] √ Save configuration to a file
- [x] √ Download configuration in json or yaml format
- [x] √ Load configuration from a file at runtime

- [ ] Create tenant - Use SurrealDB namespace
- [ ] Create tenant admin
- [ ] Assign permissions to tenant admin
- [ ] Implement Admin Panel
- [ ] Super Admin create access control for tenant

TESTING AND PERFORMANCE
- [x] √ Implement Trie Data Structure for fast path search
- [ ] Unit Testing
- [ ] Integration Testing
- [ ] Performance Testing
- [ ] Load Testing and API Gateway Performance

DOCUMENTATION
- [ ] Documentation Portal
- [ ] Implement Swagger
- [ ] Implement OpenAPI

MONITORING
- [ ] Implement Health Check
- [ ] Implement Metrics
- [ ] Implement Tracing
- [ ] Implement Logging
- [ ] Implement Alerting
