//! Dynamic network keep-alive and connection pool recycling for high-latency WAN streaming.
//!
//! Mitigates idle socket timeouts, eliminates handshake latency over high-RTT connections (>200ms),
//! and recycles stale sockets with automatic keep-alive probing.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Connection pool configuration parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Maximum idle connections preserved per endpoint `(protocol, host, port)`.
    pub max_idle_per_endpoint: usize,
    /// Maximum total connections across all remote hosts.
    pub max_total_connections: usize,
    /// Duration after which an unused idle connection is considered stale and evicted.
    pub idle_timeout_secs: u64,
    /// Interval between background keepalive heartbeat probes.
    pub keepalive_interval_secs: u64,
    /// RTT threshold in milliseconds above which connection is considered a high-latency WAN link.
    pub wan_latency_threshold_ms: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_idle_per_endpoint: 6,
            max_total_connections: 32,
            idle_timeout_secs: 30,
            keepalive_interval_secs: 10,
            wan_latency_threshold_ms: 200,
        }
    }
}

/// Endpoint identifier for grouping pooled connections.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EndpointKey {
    pub protocol: String,
    pub host: String,
    pub port: u16,
}

impl EndpointKey {
    pub fn new(protocol: &str, host: &str, port: u16) -> Self {
        Self {
            protocol: protocol.trim().to_lowercase(),
            host: host.trim().to_lowercase(),
            port,
        }
    }
}

/// Lifecycle state of a pooled connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Active,
    Idle,
    Recycled,
    Closed,
}

/// Internal tracked connection metadata inside the pool.
#[derive(Debug, Clone)]
pub struct TrackedConnection {
    pub id: u64,
    pub endpoint: EndpointKey,
    pub created_at: Instant,
    pub last_used_at: Instant,
    pub last_probed_at: Instant,
    pub reuse_count: u32,
    pub state: ConnectionState,
    pub rtt_ms: u64,
}

/// Handle held by an active caller while using a pooled connection.
#[derive(Debug)]
pub struct PooledHandle {
    pub id: u64,
    pub endpoint: EndpointKey,
    pub acquired_at: Instant,
    pub reuse_count: u32,
}

/// Summary report returned after performing a keep-alive sweep across the pool.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeepaliveSweepReport {
    pub probes_dispatched: usize,
    pub probes_successful: usize,
    pub probes_failed: usize,
    pub connections_recycled: usize,
    pub active_retained: usize,
}

/// Real-time connection pool telemetry counters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnectionPoolTelemetry {
    pub active_connections: usize,
    pub idle_connections: usize,
    pub recycled_count: usize,
    pub probes_sent: usize,
    pub probes_successful: usize,
    pub probes_failed: usize,
    pub socket_reuse_count: usize,
    pub average_rtt_ms: f64,
}

/// Dynamic Connection Pool managing socket lifecycle, keep-alive, and recycling.
#[derive(Debug)]
pub struct ConnectionPool {
    config: PoolConfig,
    connections: Mutex<HashMap<u64, TrackedConnection>>,
    next_id: AtomicU64,
    recycled_counter: AtomicUsize,
    probes_sent_counter: AtomicUsize,
    probes_successful_counter: AtomicUsize,
    reuse_counter: AtomicUsize,
    total_rtt_samples_ms: AtomicU64,
    total_rtt_samples_count: AtomicU64,
}

impl ConnectionPool {
    /// Creates a new connection pool with default configuration.
    pub fn new() -> Self {
        Self::with_config(PoolConfig::default())
    }

    /// Creates a new connection pool with custom configuration.
    pub fn with_config(config: PoolConfig) -> Self {
        Self {
            config,
            connections: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            recycled_counter: AtomicUsize::new(0),
            probes_sent_counter: AtomicUsize::new(0),
            probes_successful_counter: AtomicUsize::new(0),
            reuse_counter: AtomicUsize::new(0),
            total_rtt_samples_ms: AtomicU64::new(0),
            total_rtt_samples_count: AtomicU64::new(0),
        }
    }

    /// Acquires an idle connection for the given endpoint or registers a newly opened connection.
    pub fn acquire(&self, protocol: &str, host: &str, port: u16) -> PooledHandle {
        let key = EndpointKey::new(protocol, host, port);
        let now = Instant::now();
        let mut map = self.connections.lock().unwrap();

        // 1. Look for an existing idle connection for this endpoint
        let idle_candidate = map
            .values_mut()
            .find(|c| c.endpoint == key && c.state == ConnectionState::Idle);

        if let Some(conn) = idle_candidate {
            conn.state = ConnectionState::Active;
            conn.last_used_at = now;
            conn.reuse_count += 1;
            self.reuse_counter.fetch_add(1, Ordering::Relaxed);

            return PooledHandle {
                id: conn.id,
                endpoint: key,
                acquired_at: now,
                reuse_count: conn.reuse_count,
            };
        }

        // 2. No idle connection available: create a new connection record
        let new_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let new_conn = TrackedConnection {
            id: new_id,
            endpoint: key.clone(),
            created_at: now,
            last_used_at: now,
            last_probed_at: now,
            reuse_count: 0,
            state: ConnectionState::Active,
            rtt_ms: 0,
        };

        map.insert(new_id, new_conn);

        PooledHandle {
            id: new_id,
            endpoint: key,
            acquired_at: now,
            reuse_count: 0,
        }
    }

    /// Releases a previously acquired connection back into the pool.
    pub fn release(&self, handle: PooledHandle) {
        let now = Instant::now();
        let mut map = self.connections.lock().unwrap();

        // Count existing idle connections for this endpoint
        let idle_count = map
            .values()
            .filter(|c| c.endpoint == handle.endpoint && c.state == ConnectionState::Idle)
            .count();

        if let Some(conn) = map.get_mut(&handle.id) {
            if idle_count < self.config.max_idle_per_endpoint {
                conn.state = ConnectionState::Idle;
                conn.last_used_at = now;
            } else {
                // Exceeded idle pool limit for this endpoint: recycle immediately
                conn.state = ConnectionState::Recycled;
                self.recycled_counter.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Records round-trip latency (RTT) measurement for an endpoint.
    pub fn record_rtt(&self, protocol: &str, host: &str, port: u16, rtt: Duration) {
        let key = EndpointKey::new(protocol, host, port);
        let rtt_ms = rtt.as_millis().min(u64::MAX as u128) as u64;

        self.total_rtt_samples_ms
            .fetch_add(rtt_ms, Ordering::Relaxed);
        self.total_rtt_samples_count.fetch_add(1, Ordering::Relaxed);

        let mut map = self.connections.lock().unwrap();
        for conn in map.values_mut() {
            if conn.endpoint == key {
                conn.rtt_ms = rtt_ms;
            }
        }
    }

    /// Evaluates if the given endpoint operates across a high-latency WAN connection (>200ms).
    pub fn is_wan_connection(&self, protocol: &str, host: &str, port: u16) -> bool {
        let key = EndpointKey::new(protocol, host, port);
        let map = self.connections.lock().unwrap();

        map.values()
            .find(|c| c.endpoint == key)
            .map(|c| c.rtt_ms >= self.config.wan_latency_threshold_ms)
            .unwrap_or(false)
    }

    /// Dispatches keepalive probes and evicts expired idle connections.
    pub fn perform_keepalive_sweep(&self) -> KeepaliveSweepReport {
        self.perform_keepalive_sweep_with_probe(|_, _, _| true)
    }

    /// Dispatches keepalive probes using a custom probe closure.
    pub fn perform_keepalive_sweep_with_probe<F>(&self, mut probe_fn: F) -> KeepaliveSweepReport
    where
        F: FnMut(&str, &str, u16) -> bool,
    {
        let now = Instant::now();
        let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);
        let keepalive_interval = Duration::from_secs(self.config.keepalive_interval_secs);

        let mut report = KeepaliveSweepReport::default();
        let mut map = self.connections.lock().unwrap();

        for conn in map.values_mut() {
            match conn.state {
                ConnectionState::Idle => {
                    let idle_duration = now.saturating_duration_since(conn.last_used_at);

                    if idle_duration > idle_timeout {
                        // Stale connection exceeded idle timeout: recycle
                        conn.state = ConnectionState::Recycled;
                        report.connections_recycled += 1;
                        self.recycled_counter.fetch_add(1, Ordering::Relaxed);
                    } else {
                        // Connection is still eligible: check if keepalive probe is due
                        let probe_elapsed = now.saturating_duration_since(conn.last_probed_at);
                        if probe_elapsed >= keepalive_interval {
                            conn.last_probed_at = now;
                            report.probes_dispatched += 1;
                            self.probes_sent_counter.fetch_add(1, Ordering::Relaxed);

                            let ok = probe_fn(
                                &conn.endpoint.protocol,
                                &conn.endpoint.host,
                                conn.endpoint.port,
                            );
                            if ok {
                                report.probes_successful += 1;
                                self.probes_successful_counter
                                    .fetch_add(1, Ordering::Relaxed);
                            } else {
                                report.probes_failed += 1;
                                // Failed probe marks connection as recycled
                                conn.state = ConnectionState::Recycled;
                                report.connections_recycled += 1;
                                self.recycled_counter.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        if conn.state != ConnectionState::Recycled {
                            report.active_retained += 1;
                        }
                    }
                }
                ConnectionState::Active => {
                    report.active_retained += 1;
                }
                _ => {}
            }
        }

        // Clean up recycled entries from hash map to prevent unbounded memory growth
        map.retain(|_, c| {
            c.state != ConnectionState::Recycled && c.state != ConnectionState::Closed
        });

        report
    }

    /// Records an explicit probe result for telemetry tracking.
    pub fn record_probe_result(&self, success: bool) {
        self.probes_sent_counter.fetch_add(1, Ordering::Relaxed);
        if success {
            self.probes_successful_counter
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Returns a snapshot of connection pool telemetry.
    pub fn telemetry(&self) -> ConnectionPoolTelemetry {
        let map = self.connections.lock().unwrap();
        let mut active = 0;
        let mut idle = 0;

        for conn in map.values() {
            match conn.state {
                ConnectionState::Active => active += 1,
                ConnectionState::Idle => idle += 1,
                _ => {}
            }
        }

        let total_samples = self.total_rtt_samples_count.load(Ordering::Relaxed);
        let total_ms = self.total_rtt_samples_ms.load(Ordering::Relaxed);
        let avg_rtt = if total_samples > 0 {
            total_ms as f64 / total_samples as f64
        } else {
            0.0
        };

        let probes_sent = self.probes_sent_counter.load(Ordering::Relaxed);
        let probes_successful = self.probes_successful_counter.load(Ordering::Relaxed);
        let probes_failed = probes_sent.saturating_sub(probes_successful);

        ConnectionPoolTelemetry {
            active_connections: active,
            idle_connections: idle,
            recycled_count: self.recycled_counter.load(Ordering::Relaxed),
            probes_sent,
            probes_successful,
            probes_failed,
            socket_reuse_count: self.reuse_counter.load(Ordering::Relaxed),
            average_rtt_ms: avg_rtt,
        }
    }
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_pool_keepalive_recycling() {
        let config = PoolConfig {
            max_idle_per_endpoint: 2,
            max_total_connections: 10,
            idle_timeout_secs: 1, // Fast timeout for test
            keepalive_interval_secs: 1,
            wan_latency_threshold_ms: 200,
        };
        let pool = ConnectionPool::with_config(config);

        // 1. Acquire and release connections for an endpoint
        let h1 = pool.acquire("webdav", "nas.remote.org", 443);
        assert_eq!(h1.reuse_count, 0);

        let h2 = pool.acquire("webdav", "nas.remote.org", 443);
        assert_eq!(h2.reuse_count, 0);

        // Record high-latency WAN condition (240ms RTT)
        pool.record_rtt("webdav", "nas.remote.org", 443, Duration::from_millis(240));
        assert!(pool.is_wan_connection("webdav", "nas.remote.org", 443));

        // Release both connections back to pool
        pool.release(h1);
        pool.release(h2);

        let t1 = pool.telemetry();
        assert_eq!(t1.active_connections, 0);
        assert_eq!(t1.idle_connections, 2);

        // 2. Re-acquire: should reuse idle connection
        let h3 = pool.acquire("webdav", "nas.remote.org", 443);
        assert_eq!(h3.reuse_count, 1); // Reused!
        pool.release(h3);

        let t2 = pool.telemetry();
        assert_eq!(t2.socket_reuse_count, 1);

        // 3. Keepalive sweep before timeout
        let report = pool.perform_keepalive_sweep();
        assert!(report.active_retained >= 1);

        // 4. Sleep past idle timeout (1.1s) and sweep: verify recycling
        std::thread::sleep(Duration::from_millis(1100));
        let report_after_timeout = pool.perform_keepalive_sweep();
        assert!(report_after_timeout.connections_recycled >= 1);

        let t3 = pool.telemetry();
        assert!(t3.recycled_count >= 1);
        assert_eq!(t3.idle_connections, 0);
    }

    #[test]
    fn test_wan_threshold_classification() {
        let pool = ConnectionPool::new();
        pool.acquire("nfs", "storage.lan", 2049);
        pool.record_rtt("nfs", "storage.lan", 2049, Duration::from_millis(15));
        assert!(!pool.is_wan_connection("nfs", "storage.lan", 2049));

        pool.acquire("jellyfin", "cloud.media.tv", 8096);
        pool.record_rtt(
            "jellyfin",
            "cloud.media.tv",
            8096,
            Duration::from_millis(275),
        );
        assert!(pool.is_wan_connection("jellyfin", "cloud.media.tv", 8096));
    }
}
