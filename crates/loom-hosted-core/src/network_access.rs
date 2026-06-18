use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

static NETWORK_ACCESS_ALLOWED_CONNECTIONS: AtomicU64 = AtomicU64::new(0);
static NETWORK_ACCESS_DENIED_CONNECTIONS: AtomicU64 = AtomicU64::new(0);
static NETWORK_ACCESS_COUNTERS: LazyLock<
    Mutex<std::collections::BTreeMap<NetworkAccessMetricKey, NetworkAccessCounterValues>>,
> = LazyLock::new(|| Mutex::new(std::collections::BTreeMap::new()));
static NETWORK_ACCESS_DENIED_AUDIT_LIMITS: LazyLock<
    Mutex<std::collections::BTreeMap<NetworkAccessAuditKey, Instant>>,
> = LazyLock::new(|| Mutex::new(std::collections::BTreeMap::new()));
const NETWORK_ACCESS_DENIED_AUDIT_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedNetworkAccessMetrics {
    pub allowed_connections: u64,
    pub denied_connections: u64,
    pub counters: Vec<HostedNetworkAccessCounter>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedNetworkAccessCounter {
    pub listener_id: String,
    pub policy_name: String,
    pub rule_id: Option<String>,
    pub source_family: String,
    pub allowed_connections: u64,
    pub denied_connections: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedNetworkAccessAuditEvent {
    pub listener_id: String,
    pub policy_name: String,
    pub rule_id: Option<String>,
    pub source_family: String,
}

pub type HostedNetworkAccessAuditSink =
    Arc<dyn Fn(HostedNetworkAccessAuditEvent) + Send + Sync + 'static>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedPeerCertificate {
    leaf_der: Vec<u8>,
}

impl HostedPeerCertificate {
    pub fn from_leaf_der(leaf_der: Vec<u8>) -> Self {
        Self { leaf_der }
    }

    pub fn leaf_der(&self) -> &[u8] {
        &self.leaf_der
    }

    #[cfg(feature = "tls")]
    fn subject_text(&self) -> Option<String> {
        use x509_parser::prelude::*;

        let (_, certificate) = X509Certificate::from_der(&self.leaf_der).ok()?;
        Some(certificate.subject().to_string())
    }

    #[cfg(not(feature = "tls"))]
    fn subject_text(&self) -> Option<String> {
        None
    }

    #[cfg(feature = "tls")]
    fn issuer_text(&self) -> Option<String> {
        use x509_parser::prelude::*;

        let (_, certificate) = X509Certificate::from_der(&self.leaf_der).ok()?;
        Some(certificate.issuer().to_string())
    }

    #[cfg(not(feature = "tls"))]
    fn issuer_text(&self) -> Option<String> {
        None
    }

    #[cfg(feature = "tls")]
    fn san_texts(&self) -> Vec<String> {
        use x509_parser::prelude::*;

        let Ok((_, certificate)) = X509Certificate::from_der(&self.leaf_der) else {
            return Vec::new();
        };
        let Ok(Some(san)) = certificate.subject_alternative_name() else {
            return Vec::new();
        };
        san.value
            .general_names
            .iter()
            .map(|name| name.to_string())
            .collect()
    }

    #[cfg(not(feature = "tls"))]
    fn san_texts(&self) -> Vec<String> {
        Vec::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedNetworkAccessPolicy {
    listener_id: Option<String>,
    name: String,
    default_action: loom_store::NetworkAccessAction,
    rules: Vec<CompiledNetworkAccessRule>,
}

impl HostedNetworkAccessPolicy {
    pub fn from_record(record: loom_store::NetworkAccessPolicyRecord) -> Self {
        Self::from_record_for_listener(None, record)
    }

    pub fn from_record_for_listener(
        listener_id: Option<String>,
        record: loom_store::NetworkAccessPolicyRecord,
    ) -> Self {
        Self {
            listener_id,
            name: record.name,
            default_action: record.default_action,
            rules: record
                .rules
                .into_iter()
                .map(CompiledNetworkAccessRule::from_rule)
                .collect(),
        }
    }

    pub fn listener_id(&self) -> Option<&str> {
        self.listener_id.as_deref()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn requires_request_context(&self) -> bool {
        self.rules.iter().any(|rule| {
            rule.trusted_proxy_cidr.is_some()
                || rule.require_mtls
                || rule.client_cert_subject.is_some()
                || rule.client_cert_san.is_some()
                || rule.client_cert_issuer.is_some()
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompiledNetworkAccessRule {
    id: String,
    action: loom_store::NetworkAccessAction,
    source_cidr: Option<CompiledCidr>,
    trusted_proxy_cidr: Option<CompiledCidr>,
    require_mtls: bool,
    client_cert_subject: Option<String>,
    client_cert_san: Option<String>,
    client_cert_issuer: Option<String>,
}

impl CompiledNetworkAccessRule {
    fn from_rule(rule: loom_store::NetworkAccessRule) -> Self {
        Self {
            id: rule.id,
            action: rule.action,
            source_cidr: rule.source_cidr.map(CompiledCidr::from_cidr),
            trusted_proxy_cidr: rule.trusted_proxy_cidr.map(CompiledCidr::from_cidr),
            require_mtls: rule.require_mtls,
            client_cert_subject: rule.client_cert_subject,
            client_cert_san: rule.client_cert_san,
            client_cert_issuer: rule.client_cert_issuer,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompiledCidr {
    V4 { start: u32, end: u32 },
    V6 { start: u128, end: u128 },
}

impl CompiledCidr {
    fn from_cidr(cidr: loom_store::NetworkAccessCidr) -> Self {
        match cidr.addr {
            IpAddr::V4(addr) => {
                let addr = u32::from(addr);
                let size = if cidr.prefix == 0 {
                    u32::MAX
                } else {
                    (1u32 << (32 - u32::from(cidr.prefix))) - 1
                };
                Self::V4 {
                    start: addr,
                    end: addr | size,
                }
            }
            IpAddr::V6(addr) => {
                let addr = u128::from(addr);
                let size = if cidr.prefix == 0 {
                    u128::MAX
                } else {
                    (1u128 << (128 - u32::from(cidr.prefix))) - 1
                };
                Self::V6 {
                    start: addr,
                    end: addr | size,
                }
            }
        }
    }

    fn contains(self, addr: IpAddr) -> bool {
        match (self, addr) {
            (Self::V4 { start, end }, IpAddr::V4(addr)) => {
                let addr = u32::from(addr);
                start <= addr && addr <= end
            }
            (Self::V6 { start, end }, IpAddr::V6(addr)) => {
                let addr = u128::from(addr);
                start <= addr && addr <= end
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct NetworkAccessMetricKey {
    listener_id: String,
    policy_name: String,
    rule_id: Option<String>,
    source_family: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct NetworkAccessAuditKey {
    listener_id: String,
    policy_name: String,
    rule_id: Option<String>,
    source_family: &'static str,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct NetworkAccessCounterValues {
    allowed_connections: u64,
    denied_connections: u64,
}

tokio::task_local! {
    static HOSTED_NETWORK_ACCESS_POLICY: Option<HostedNetworkAccessPolicy>;
}

tokio::task_local! {
    static HOSTED_NETWORK_ACCESS_DENIED_AUDIT: Option<HostedNetworkAccessAuditSink>;
}

pub async fn with_hosted_network_access_policy<F>(
    policy: Option<loom_store::NetworkAccessPolicyRecord>,
    future: F,
) -> F::Output
where
    F: Future,
{
    let policy = policy.map(HostedNetworkAccessPolicy::from_record);
    HOSTED_NETWORK_ACCESS_POLICY
        .scope(
            policy,
            HOSTED_NETWORK_ACCESS_DENIED_AUDIT.scope(None, future),
        )
        .await
}

pub async fn with_hosted_network_access_policy_for_listener<F>(
    listener_id: String,
    policy: Option<loom_store::NetworkAccessPolicyRecord>,
    future: F,
) -> F::Output
where
    F: Future,
{
    let policy = policy.map(|record| {
        HostedNetworkAccessPolicy::from_record_for_listener(Some(listener_id), record)
    });
    HOSTED_NETWORK_ACCESS_POLICY
        .scope(
            policy,
            HOSTED_NETWORK_ACCESS_DENIED_AUDIT.scope(None, future),
        )
        .await
}

pub async fn with_hosted_network_access_policy_for_listener_and_audit<F>(
    listener_id: String,
    policy: Option<loom_store::NetworkAccessPolicyRecord>,
    denied_audit: Option<HostedNetworkAccessAuditSink>,
    future: F,
) -> F::Output
where
    F: Future,
{
    let policy = policy.map(|record| {
        HostedNetworkAccessPolicy::from_record_for_listener(Some(listener_id), record)
    });
    HOSTED_NETWORK_ACCESS_POLICY
        .scope(
            policy,
            HOSTED_NETWORK_ACCESS_DENIED_AUDIT.scope(denied_audit, future),
        )
        .await
}

pub fn current_hosted_network_access_policy() -> Option<HostedNetworkAccessPolicy> {
    HOSTED_NETWORK_ACCESS_POLICY
        .try_with(Clone::clone)
        .ok()
        .flatten()
}

pub fn network_access_allows(
    policy: Option<&HostedNetworkAccessPolicy>,
    peer_addr: SocketAddr,
    peer_certificate: Option<&HostedPeerCertificate>,
    x_forwarded_for: Option<&str>,
    forwarded: Option<&str>,
) -> bool {
    network_access_allows_with_denied_audit(
        policy,
        peer_addr,
        peer_certificate,
        x_forwarded_for,
        forwarded,
        None,
    )
}

pub fn network_access_allows_with_denied_audit(
    policy: Option<&HostedNetworkAccessPolicy>,
    peer_addr: SocketAddr,
    peer_certificate: Option<&HostedPeerCertificate>,
    x_forwarded_for: Option<&str>,
    forwarded: Option<&str>,
    denied_audit: Option<&HostedNetworkAccessAuditSink>,
) -> bool {
    let Some(policy) = policy else {
        return true;
    };
    let decision = network_access_policy_decision(
        policy,
        peer_addr,
        peer_certificate,
        x_forwarded_for,
        forwarded,
    );
    record_network_access_decision(policy, &decision, denied_audit);
    decision.allowed
}

pub fn network_access_metrics() -> HostedNetworkAccessMetrics {
    let counters = NETWORK_ACCESS_COUNTERS
        .lock()
        .map(|counters| {
            counters
                .iter()
                .map(|(key, values)| HostedNetworkAccessCounter {
                    listener_id: key.listener_id.clone(),
                    policy_name: key.policy_name.clone(),
                    rule_id: key.rule_id.clone(),
                    source_family: key.source_family.to_string(),
                    allowed_connections: values.allowed_connections,
                    denied_connections: values.denied_connections,
                })
                .collect()
        })
        .unwrap_or_default();
    HostedNetworkAccessMetrics {
        allowed_connections: NETWORK_ACCESS_ALLOWED_CONNECTIONS.load(Ordering::Relaxed),
        denied_connections: NETWORK_ACCESS_DENIED_CONNECTIONS.load(Ordering::Relaxed),
        counters,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NetworkAccessDecision {
    allowed: bool,
    rule_id: Option<String>,
    source_family: &'static str,
}

fn network_access_policy_decision(
    policy: &HostedNetworkAccessPolicy,
    peer_addr: SocketAddr,
    peer_certificate: Option<&HostedPeerCertificate>,
    x_forwarded_for: Option<&str>,
    forwarded: Option<&str>,
) -> NetworkAccessDecision {
    let forwarded_ip = forwarded_client_ip(x_forwarded_for, forwarded);
    for rule in &policy.rules {
        let candidate_ip = match network_access_rule_candidate_ip(rule, peer_addr, forwarded_ip) {
            NetworkAccessRuleCandidate::Ip(ip) => ip,
            NetworkAccessRuleCandidate::Deny => {
                return NetworkAccessDecision {
                    allowed: false,
                    rule_id: Some(rule.id.clone()),
                    source_family: source_family(peer_addr.ip()),
                };
            }
            NetworkAccessRuleCandidate::Skip => continue,
        };
        if let Some(cidr) = rule.source_cidr
            && !cidr.contains(candidate_ip)
        {
            continue;
        }
        if network_access_rule_certificate_matches(rule, peer_certificate) {
            return NetworkAccessDecision {
                allowed: rule.action == loom_store::NetworkAccessAction::Allow,
                rule_id: Some(rule.id.clone()),
                source_family: source_family(candidate_ip),
            };
        }
    }
    NetworkAccessDecision {
        allowed: policy.default_action == loom_store::NetworkAccessAction::Allow,
        rule_id: None,
        source_family: source_family(peer_addr.ip()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NetworkAccessRuleCandidate {
    Skip,
    Deny,
    Ip(IpAddr),
}

fn network_access_rule_candidate_ip(
    rule: &CompiledNetworkAccessRule,
    peer_addr: SocketAddr,
    forwarded_ip: ForwardedClientIp,
) -> NetworkAccessRuleCandidate {
    match rule.trusted_proxy_cidr {
        Some(proxy_cidr) => {
            if !proxy_cidr.contains(peer_addr.ip()) {
                return NetworkAccessRuleCandidate::Skip;
            }
            match forwarded_ip {
                ForwardedClientIp::Ip(ip) => NetworkAccessRuleCandidate::Ip(ip),
                ForwardedClientIp::Malformed => NetworkAccessRuleCandidate::Deny,
                ForwardedClientIp::Missing => NetworkAccessRuleCandidate::Skip,
            }
        }
        None => NetworkAccessRuleCandidate::Ip(peer_addr.ip()),
    }
}

fn network_access_rule_certificate_matches(
    rule: &CompiledNetworkAccessRule,
    peer_certificate: Option<&HostedPeerCertificate>,
) -> bool {
    if rule.require_mtls && peer_certificate.is_none() {
        return false;
    }
    if let Some(expected) = rule.client_cert_subject.as_deref() {
        let Some(subject) = peer_certificate.and_then(HostedPeerCertificate::subject_text) else {
            return false;
        };
        if !subject.contains(expected) {
            return false;
        }
    }
    if let Some(expected) = rule.client_cert_san.as_deref() {
        let Some(certificate) = peer_certificate else {
            return false;
        };
        if !certificate
            .san_texts()
            .iter()
            .any(|value| value.contains(expected))
        {
            return false;
        }
    }
    if let Some(expected) = rule.client_cert_issuer.as_deref() {
        let Some(issuer) = peer_certificate.and_then(HostedPeerCertificate::issuer_text) else {
            return false;
        };
        if !issuer.contains(expected) {
            return false;
        }
    }
    true
}

fn record_network_access_decision(
    policy: &HostedNetworkAccessPolicy,
    decision: &NetworkAccessDecision,
    denied_audit: Option<&HostedNetworkAccessAuditSink>,
) {
    if decision.allowed {
        NETWORK_ACCESS_ALLOWED_CONNECTIONS.fetch_add(1, Ordering::Relaxed);
    } else {
        NETWORK_ACCESS_DENIED_CONNECTIONS.fetch_add(1, Ordering::Relaxed);
    }
    let key = NetworkAccessMetricKey {
        listener_id: policy
            .listener_id
            .as_deref()
            .unwrap_or("unknown")
            .to_string(),
        policy_name: policy.name.clone(),
        rule_id: decision.rule_id.clone(),
        source_family: decision.source_family,
    };
    if let Ok(mut counters) = NETWORK_ACCESS_COUNTERS.lock() {
        let values = counters.entry(key).or_default();
        if decision.allowed {
            values.allowed_connections = values.allowed_connections.saturating_add(1);
        } else {
            values.denied_connections = values.denied_connections.saturating_add(1);
        }
    }
    if !decision.allowed {
        record_network_access_denied_audit(policy, decision, denied_audit);
    }
}

fn record_network_access_denied_audit(
    policy: &HostedNetworkAccessPolicy,
    decision: &NetworkAccessDecision,
    denied_audit: Option<&HostedNetworkAccessAuditSink>,
) {
    let sink = denied_audit.cloned().or_else(|| {
        HOSTED_NETWORK_ACCESS_DENIED_AUDIT
            .try_with(Clone::clone)
            .ok()
            .flatten()
    });
    let Some(sink) = sink else {
        return;
    };
    let key = NetworkAccessAuditKey {
        listener_id: policy
            .listener_id
            .as_deref()
            .unwrap_or("unknown")
            .to_string(),
        policy_name: policy.name.clone(),
        rule_id: decision.rule_id.clone(),
        source_family: decision.source_family,
    };
    if !network_access_denied_audit_allowed(&key) {
        return;
    }
    sink(HostedNetworkAccessAuditEvent {
        listener_id: key.listener_id,
        policy_name: key.policy_name,
        rule_id: key.rule_id,
        source_family: key.source_family.to_string(),
    });
}

fn network_access_denied_audit_allowed(key: &NetworkAccessAuditKey) -> bool {
    let now = Instant::now();
    let Ok(mut limits) = NETWORK_ACCESS_DENIED_AUDIT_LIMITS.lock() else {
        return false;
    };
    match limits.get(key) {
        Some(last) if now.duration_since(*last) < NETWORK_ACCESS_DENIED_AUDIT_INTERVAL => false,
        _ => {
            limits.insert(key.clone(), now);
            true
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ForwardedClientIp {
    Missing,
    Malformed,
    Ip(IpAddr),
}

fn forwarded_client_ip(
    x_forwarded_for: Option<&str>,
    forwarded: Option<&str>,
) -> ForwardedClientIp {
    if let Some(value) = x_forwarded_for {
        return first_x_forwarded_for_ip(value);
    }
    if let Some(value) = forwarded {
        return first_forwarded_for_ip(value);
    }
    ForwardedClientIp::Missing
}

fn first_x_forwarded_for_ip(value: &str) -> ForwardedClientIp {
    let Some(part) = value.split(',').next().map(str::trim) else {
        return ForwardedClientIp::Malformed;
    };
    if part.is_empty() {
        return ForwardedClientIp::Malformed;
    }
    part.parse::<IpAddr>()
        .map(ForwardedClientIp::Ip)
        .unwrap_or(ForwardedClientIp::Malformed)
}

fn first_forwarded_for_ip(value: &str) -> ForwardedClientIp {
    let Some(first) = value.split(',').next() else {
        return ForwardedClientIp::Malformed;
    };
    for pair in first.split(';') {
        let Some((name, raw_value)) = pair.trim().split_once('=') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("for") {
            continue;
        }
        let value = raw_value.trim().trim_matches('"');
        let value = value
            .strip_prefix('[')
            .and_then(|rest| rest.split_once(']').map(|(ip, _)| ip))
            .unwrap_or(value);
        return value
            .parse::<IpAddr>()
            .map(ForwardedClientIp::Ip)
            .unwrap_or(ForwardedClientIp::Malformed);
    }
    ForwardedClientIp::Malformed
}

fn source_family(addr: IpAddr) -> &'static str {
    match addr {
        IpAddr::V4(_) => "ipv4",
        IpAddr::V6(_) => "ipv6",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn cidr(value: &str) -> loom_store::NetworkAccessCidr {
        loom_store::NetworkAccessCidr::parse(value).unwrap()
    }

    fn rule(
        id: &str,
        action: loom_store::NetworkAccessAction,
        source_cidr: Option<&str>,
        trusted_proxy_cidr: Option<&str>,
    ) -> loom_store::NetworkAccessRule {
        loom_store::NetworkAccessRule {
            id: id.to_string(),
            action,
            source_cidr: source_cidr.map(cidr),
            trusted_proxy_cidr: trusted_proxy_cidr.map(cidr),
            require_mtls: false,
            client_cert_subject: None,
            client_cert_san: None,
            client_cert_issuer: None,
            description: None,
        }
    }

    fn policy(
        default_action: loom_store::NetworkAccessAction,
        rules: Vec<loom_store::NetworkAccessRule>,
    ) -> loom_store::NetworkAccessPolicyRecord {
        loom_store::NetworkAccessPolicyRecord {
            name: "office".to_string(),
            schema_version: 1,
            description: None,
            default_action,
            rules,
            created_audit_seq: None,
            updated_audit_seq: None,
        }
    }

    fn peer(value: [u8; 4]) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::from(value)), 443)
    }

    fn compiled(policy: loom_store::NetworkAccessPolicyRecord) -> HostedNetworkAccessPolicy {
        HostedNetworkAccessPolicy::from_record_for_listener(Some("admin-rest".to_string()), policy)
    }

    #[test]
    fn first_matching_rule_decides_before_default_action() {
        let policy = compiled(policy(
            loom_store::NetworkAccessAction::Deny,
            vec![
                rule(
                    "allow-office",
                    loom_store::NetworkAccessAction::Allow,
                    Some("10.0.0.0/8"),
                    None,
                ),
                rule(
                    "deny-host",
                    loom_store::NetworkAccessAction::Deny,
                    Some("10.1.2.3/32"),
                    None,
                ),
            ],
        ));

        assert!(network_access_allows(
            Some(&policy),
            peer([10, 1, 2, 3]),
            None,
            None,
            None,
        ));
        assert!(!network_access_allows(
            Some(&policy),
            peer([192, 0, 2, 10]),
            None,
            None,
            None,
        ));
    }

    #[test]
    fn trusted_proxy_rule_uses_first_forwarded_client_only_from_trusted_peer() {
        let policy = compiled(policy(
            loom_store::NetworkAccessAction::Deny,
            vec![rule(
                "proxy-office",
                loom_store::NetworkAccessAction::Allow,
                Some("203.0.113.0/24"),
                Some("10.0.0.0/8"),
            )],
        ));

        assert!(network_access_allows(
            Some(&policy),
            peer([10, 1, 2, 3]),
            None,
            Some("203.0.113.44, 198.51.100.1"),
            None,
        ));
        assert!(!network_access_allows(
            Some(&policy),
            peer([10, 1, 2, 3]),
            None,
            Some("198.51.100.1, 203.0.113.44"),
            None,
        ));
        assert!(!network_access_allows(
            Some(&policy),
            peer([198, 51, 100, 10]),
            None,
            Some("203.0.113.44"),
            None,
        ));
    }

    #[test]
    fn trusted_proxy_rule_accepts_forwarded_header_and_fails_closed_when_malformed() {
        let policy = compiled(policy(
            loom_store::NetworkAccessAction::Allow,
            vec![rule(
                "proxy-office",
                loom_store::NetworkAccessAction::Allow,
                Some("203.0.113.0/24"),
                Some("10.0.0.0/8"),
            )],
        ));

        assert!(network_access_allows(
            Some(&policy),
            peer([10, 1, 2, 3]),
            None,
            None,
            Some("for=203.0.113.44;proto=https"),
        ));
        assert!(!network_access_allows(
            Some(&policy),
            peer([10, 1, 2, 3]),
            None,
            Some("not-an-ip"),
            None,
        ));
    }

    #[test]
    fn denied_connections_increment_aggregate_metric() {
        let policy = compiled(policy(loom_store::NetworkAccessAction::Deny, Vec::new()));
        let before = network_access_metrics().denied_connections;

        assert!(!network_access_allows(
            Some(&policy),
            peer([192, 0, 2, 10]),
            None,
            None,
            None,
        ));

        let after = network_access_metrics().denied_connections;
        assert!(after > before);
    }

    #[test]
    fn metrics_include_listener_policy_rule_and_source_family() {
        let policy = compiled(policy(
            loom_store::NetworkAccessAction::Deny,
            vec![rule(
                "allow-office",
                loom_store::NetworkAccessAction::Allow,
                Some("10.0.0.0/8"),
                None,
            )],
        ));

        assert!(network_access_allows(
            Some(&policy),
            peer([10, 1, 2, 3]),
            None,
            None,
            None,
        ));

        let metrics = network_access_metrics();
        assert!(metrics.allowed_connections > 0);
        assert!(metrics.counters.iter().any(|counter| {
            counter.listener_id == "admin-rest"
                && counter.policy_name == "office"
                && counter.rule_id.as_deref() == Some("allow-office")
                && counter.source_family == "ipv4"
                && counter.allowed_connections > 0
        }));
    }

    #[tokio::test]
    async fn denied_connection_audit_is_rate_limited_and_sanitized() {
        let events = Arc::new(Mutex::new(Vec::<HostedNetworkAccessAuditEvent>::new()));
        let audit_events = events.clone();
        let sink: HostedNetworkAccessAuditSink =
            Arc::new(move |event| audit_events.lock().unwrap().push(event));
        let record = policy(
            loom_store::NetworkAccessAction::Deny,
            vec![rule(
                "deny-private",
                loom_store::NetworkAccessAction::Deny,
                Some("10.0.0.0/8"),
                None,
            )],
        );

        with_hosted_network_access_policy_for_listener_and_audit(
            "admin-rest-audit".to_string(),
            Some(record),
            Some(sink),
            async {
                assert!(!network_access_allows(
                    current_hosted_network_access_policy().as_ref(),
                    peer([10, 1, 2, 3]),
                    None,
                    Some("10.1.2.3, 203.0.113.10"),
                    None,
                ));
                assert!(!network_access_allows(
                    current_hosted_network_access_policy().as_ref(),
                    peer([10, 1, 2, 4]),
                    None,
                    Some("10.1.2.4, 203.0.113.10"),
                    None,
                ));
            },
        )
        .await;

        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].listener_id, "admin-rest-audit");
        assert_eq!(events[0].policy_name, "office");
        assert_eq!(events[0].rule_id.as_deref(), Some("deny-private"));
        assert_eq!(events[0].source_family, "ipv4");
    }

    #[cfg(feature = "tls")]
    #[test]
    fn mtls_rule_matches_verified_peer_certificate_san() {
        let rcgen::CertifiedKey { cert, .. } =
            rcgen::generate_simple_self_signed(vec!["client.example".to_string()]).unwrap();
        let certificate = HostedPeerCertificate::from_leaf_der(cert.der().as_ref().to_vec());
        let mut rule = rule(
            "allow-client-cert",
            loom_store::NetworkAccessAction::Allow,
            None,
            None,
        );
        rule.require_mtls = true;
        rule.client_cert_san = Some("client.example".to_string());
        let policy = compiled(policy(loom_store::NetworkAccessAction::Deny, vec![rule]));

        assert!(network_access_allows(
            Some(&policy),
            peer([10, 1, 2, 3]),
            Some(&certificate),
            None,
            None,
        ));
        assert!(!network_access_allows(
            Some(&policy),
            peer([10, 1, 2, 3]),
            None,
            None,
            None,
        ));
    }
}
