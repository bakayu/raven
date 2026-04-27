-- migrations/0001_initial.sql

-- Dashboard users
CREATE TABLE users (
    id                    TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    username              TEXT NOT NULL UNIQUE,
    email                 TEXT,
    password_hash         TEXT,                    -- Argon2id; NULL for OIDC-only users
    role                  TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('admin', 'member')),
    failed_login_attempts INTEGER NOT NULL DEFAULT 0,
    auth_locked_until     TEXT,
    last_login_at         TEXT,
    created_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at            TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- External OIDC identities linked to local users
CREATE TABLE user_identities (
    id            TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    user_id       TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider      TEXT NOT NULL,                   -- e.g. google, github
    issuer        TEXT NOT NULL,
    subject       TEXT NOT NULL,                   -- OIDC sub claim
    email         TEXT,
    email_verified INTEGER NOT NULL DEFAULT 0,     -- 0 = false, 1 = true
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    last_login_at TEXT,
    UNIQUE (issuer, subject)
);

-- Refresh token sessions (stored hashed)
CREATE TABLE refresh_tokens (
    id                    TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    user_id               TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash            TEXT NOT NULL UNIQUE,
    issued_at             TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    expires_at            TEXT NOT NULL,
    rotated_from_token_id TEXT REFERENCES refresh_tokens(id) ON DELETE SET NULL,
    revoked_at            TEXT,
    revoked_reason        TEXT,
    created_by_ip         TEXT,
    created_by_user_agent TEXT,
    last_used_at          TEXT
);

-- Agent authentication tokens
CREATE TABLE agent_tokens (
    id           TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    name         TEXT NOT NULL,                    -- human label e.g. "web-server-1"
    token_hash   TEXT NOT NULL UNIQUE,             -- SHA-256 of "rvn_..."
    created_by   TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    last_used_at TEXT,
    revoked_at   TEXT                              -- NULL = active, set = revoked
);

-- Registered agents (upserted on Register RPC)
-- UNIQUE (token_id, hostname) gives the ON CONFLICT upsert a concrete target
CREATE TABLE agents (
    id            TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    token_id      TEXT NOT NULL REFERENCES agent_tokens(id) ON DELETE CASCADE,
    hostname      TEXT NOT NULL,
    ip            TEXT,
    os            TEXT,
    agent_version TEXT,
    log_files     TEXT NOT NULL DEFAULT '[]',      -- JSON array of configured log file paths
    first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    last_seen_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE (token_id, hostname)
);

-- Alert rules
-- created_by is SET NULL on user delete so rules survive user removal
CREATE TABLE alert_rules (
    id               TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    name             TEXT NOT NULL,
    metric           TEXT NOT NULL,                -- e.g. cpu_usage, mem_usage, disk_usage
    operator         TEXT NOT NULL CHECK (operator IN ('>', '<', '>=', '<=')),
    threshold        REAL NOT NULL,
    duration_seconds INTEGER NOT NULL,
    hosts            TEXT NOT NULL DEFAULT '[]',   -- JSON array, empty = all hosts
    channels         TEXT NOT NULL DEFAULT '[]',   -- JSON array of channel IDs
    enabled          INTEGER NOT NULL DEFAULT 1,   -- 0 = false, 1 = true
    created_by       TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Notification channels
-- created_by is SET NULL on user delete so channels survive user removal
CREATE TABLE notification_channels (
    id           TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    name         TEXT NOT NULL,
    channel_type TEXT NOT NULL CHECK (channel_type IN ('discord', 'slack', 'smtp')),
    config       TEXT NOT NULL,                    -- JSON: webhook_url or smtp config
    created_by   TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Alert history (fired/resolved events)
CREATE TABLE alert_events (
    id          TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    rule_id     TEXT NOT NULL REFERENCES alert_rules(id) ON DELETE CASCADE,
    hostname    TEXT NOT NULL,
    status      TEXT NOT NULL CHECK (status IN ('firing', 'resolved')),
    value       REAL,
    fired_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    resolved_at TEXT
);

-- Append-only audit trail
CREATE TABLE audit_log (
    id            TEXT PRIMARY KEY DEFAULT (hex(randomblob(16))),
    actor_user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    action        TEXT NOT NULL,
    entity_type   TEXT NOT NULL,
    entity_id     TEXT,
    metadata      TEXT NOT NULL DEFAULT '{}',      -- JSON
    ip            TEXT,
    user_agent    TEXT,
    request_id    TEXT,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Indexes

-- Token validation on every agent RPC (hottest read path)
CREATE INDEX idx_agent_tokens_token_hash ON agent_tokens (token_hash);
CREATE INDEX idx_agent_tokens_revoked_at ON agent_tokens (revoked_at);

-- Agent lookup by token (used in Register + Heartbeat)
CREATE INDEX idx_agents_token_id ON agents (token_id);

-- Refresh token lookup on every dashboard API call
CREATE INDEX idx_refresh_tokens_token_hash ON refresh_tokens (token_hash);
CREATE INDEX idx_refresh_tokens_user_id    ON refresh_tokens (user_id);
CREATE INDEX idx_refresh_tokens_expires_at ON refresh_tokens (expires_at);

-- OIDC identity lookup during login
CREATE INDEX idx_user_identities_user_id ON user_identities (user_id);

-- Alert rule queries (alert engine loads enabled rules every 30s)
CREATE INDEX idx_alert_rules_enabled ON alert_rules (enabled);

-- Alert event history queries
CREATE INDEX idx_alert_events_rule_id  ON alert_events (rule_id);
CREATE INDEX idx_alert_events_fired_at ON alert_events (fired_at);

-- Audit log queries by actor and time
CREATE INDEX idx_audit_log_actor_user_id ON audit_log (actor_user_id);
CREATE INDEX idx_audit_log_created_at    ON audit_log (created_at);
