-- SPDX-License-Identifier: MIT
PRAGMA user_version = 1;
CREATE TABLE accounts (
 id TEXT PRIMARY KEY NOT NULL,
 username TEXT UNIQUE NOT NULL,
 role TEXT NOT NULL CHECK(role IN ('admin','member')),
 provisioning_state TEXT NOT NULL CHECK(provisioning_state IN ('provisioning','ready')),
 enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
 password_phc TEXT NOT NULL,
 password_revision INTEGER NOT NULL CHECK(password_revision>0),
 generation INTEGER NOT NULL CHECK(generation>0)
);
CREATE TRIGGER identity_immutable BEFORE UPDATE OF id,username,role ON accounts
 BEGIN SELECT RAISE(ABORT,'immutable identity'); END;
CREATE TABLE credentials (
 hash BLOB PRIMARY KEY NOT NULL CHECK(length(hash)=32),
 kind TEXT NOT NULL CHECK(kind IN ('session','hostkey')),
 account_id TEXT NOT NULL REFERENCES accounts(id),
 generation INTEGER NOT NULL,
 expires_at INTEGER,
 revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)),
 CHECK((kind='session' AND expires_at IS NOT NULL) OR (kind='hostkey' AND expires_at IS NULL))
);
CREATE INDEX credentials_account ON credentials(account_id);
CREATE TABLE capabilities (
 hash BLOB PRIMARY KEY NOT NULL CHECK(length(hash)=32),
 kind TEXT NOT NULL CHECK(kind IN ('setup','invite','reset')),
 issuer TEXT REFERENCES accounts(id),
 issuer_generation INTEGER,
 target TEXT REFERENCES accounts(id),
 target_generation INTEGER,
 expires_at INTEGER,
 state TEXT NOT NULL CHECK(state IN ('available','provisioning','consumed','revoked')),
 reserved_id TEXT UNIQUE REFERENCES accounts(id),
 CHECK((kind='setup' AND issuer IS NULL AND expires_at IS NULL) OR (kind<>'setup' AND issuer IS NOT NULL AND issuer_generation IS NOT NULL AND expires_at IS NOT NULL)),
 CHECK(kind<>'reset' OR (target IS NOT NULL AND target_generation IS NOT NULL))
);
CREATE INDEX capabilities_issuer ON capabilities(issuer);
CREATE INDEX capabilities_target ON capabilities(target);
-- Immutable claim proof: startup can check the capability/account association
-- before resuming it, without hashing or retaining the plaintext password.
CREATE TABLE provisioning_reservations (
 capability BLOB PRIMARY KEY NOT NULL REFERENCES capabilities(hash),
 account_id TEXT UNIQUE NOT NULL REFERENCES accounts(id),
 username TEXT UNIQUE NOT NULL,
 role TEXT NOT NULL CHECK(role IN ('admin','member')),
 credential_proof BLOB NOT NULL CHECK(length(credential_proof)=32)
);
CREATE TRIGGER reservation_immutable BEFORE UPDATE ON provisioning_reservations
 BEGIN SELECT RAISE(ABORT,'immutable reservation'); END;
CREATE TRIGGER reservation_retained BEFORE DELETE ON provisioning_reservations
 BEGIN SELECT RAISE(ABORT,'retained reservation'); END;
CREATE TABLE setup (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 phase TEXT NOT NULL CHECK(phase IN ('incomplete','open','closed')),
 capability BLOB NOT NULL REFERENCES capabilities(hash)
);
CREATE TABLE service_state (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 admission_open INTEGER NOT NULL CHECK(admission_open IN (0,1))
);
INSERT INTO service_state VALUES(1,1);
