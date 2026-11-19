-- SPDX-License-Identifier: MIT
-- OAuth secrets are digests, independently namespaced from native credentials.
CREATE TABLE oauth_pending (
 hash BLOB PRIMARY KEY NOT NULL CHECK(length(hash)=32),
 request TEXT NOT NULL,
 expires_at INTEGER NOT NULL,
 session BLOB CHECK(session IS NULL OR length(session)=32),
 account_id TEXT REFERENCES accounts(id) ON DELETE CASCADE,
 generation INTEGER,
 CHECK((session IS NULL AND account_id IS NULL AND generation IS NULL) OR
       (session IS NOT NULL AND account_id IS NOT NULL AND generation>0))
);
CREATE TABLE oauth_grants (
 id TEXT PRIMARY KEY NOT NULL,
 account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
 generation INTEGER NOT NULL CHECK(generation>0),
 client TEXT NOT NULL,
 resource TEXT NOT NULL,
 write_scope INTEGER NOT NULL CHECK(write_scope IN (0,1)),
 created_at INTEGER NOT NULL,
 expires_at INTEGER NOT NULL,
 revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)),
 CHECK(expires_at>created_at AND expires_at<=created_at+2592000)
);
CREATE INDEX oauth_grants_account ON oauth_grants(account_id);
CREATE TABLE oauth_codes (
 hash BLOB PRIMARY KEY NOT NULL CHECK(length(hash)=32),
 account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
 generation INTEGER NOT NULL CHECK(generation>0),
 request TEXT NOT NULL,
 expires_at INTEGER NOT NULL,
 grant_id TEXT REFERENCES oauth_grants(id) ON DELETE CASCADE
);
CREATE TABLE oauth_tokens (
 hash BLOB PRIMARY KEY NOT NULL CHECK(length(hash)=32),
 grant_id TEXT NOT NULL REFERENCES oauth_grants(id) ON DELETE CASCADE,
 kind TEXT NOT NULL CHECK(kind IN ('access','refresh')),
 expires_at INTEGER NOT NULL,
 consumed INTEGER NOT NULL DEFAULT 0 CHECK(consumed IN (0,1))
);
CREATE INDEX oauth_tokens_grant ON oauth_tokens(grant_id);
PRAGMA user_version=4;
