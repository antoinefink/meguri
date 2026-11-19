// SPDX-License-Identifier: MIT
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { accessLabel, api, asError, errorMessage } from "./api";
import type { MCPGrant, MCPGrants, MCPRevocation, Me } from "./api";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, CopyValue, Outcome } from "./ui";
import type { OutcomeValue } from "./ui";

function GrantRow({ grant, pending, revoke }: { grant: MCPGrant; pending: boolean; revoke: () => Promise<void> }) {
  const [confirm, setConfirm] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null), cancel = useRef<HTMLButtonElement>(null), restoreFocus = useRef(false);
  useLayoutEffect(() => {
    if (confirm) cancel.current?.focus();
    else if (restoreFocus.current) { trigger.current?.focus(); restoreFocus.current = false; }
  }, [confirm]);
  function close() { if (!pending) { restoreFocus.current = true; setConfirm(false); } }
  return <li className="grant-row" aria-label={`Connection ${grant.client_name}`}>
    <div className="grant-info"><h3>{grant.client_name}</h3><p>{accessLabel[grant.access]}</p>
      <p>Created <time dateTime={grant.created_at}>{new Date(grant.created_at).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" })}</time></p>
    </div>
    <Button ref={trigger} data-grant-action variant="outline" aria-disabled={pending || confirm} onClick={() => { if (!pending && !confirm) setConfirm(true); }}>Revoke<span className="sr-only"> {grant.client_name}</span></Button>
    {confirm && <section className="confirmation" aria-label={`Confirm revoking ${grant.client_name}`} onKeyDown={event => { if (event.key === "Escape") close(); }}>
      <h3>Revoke {grant.client_name}?</h3><p>This removes this connection’s {accessLabel[grant.access].toLowerCase()} permission. The client will need your permission to connect again.</p>
      <div className="actions"><Button ref={cancel} variant="outline" aria-disabled={pending} onClick={close}>Cancel</Button>
        <Button variant="destructive" aria-disabled={pending} onClick={() => { if (!pending) void revoke(); }}>{pending ? "Revoking…" : `Confirm revoke ${grant.client_name}`}</Button></div>
    </section>}
  </li>;
}

// App keys this screen by account ID and visit, so identity changes discard its state.
export function Connection({ me, sessionLoss, refreshIdentity }: { me: Me; sessionLoss: () => void; refreshIdentity: () => void }) {
  const [grants, setGrants] = useState<MCPGrant[] | null>(null), [loading, setLoading] = useState(true), [pending, setPending] = useState(false);
  const [outcome, setOutcome] = useState<OutcomeValue | null>(null);
  const section = useRef<HTMLElement>(null), busy = useRef(false), alive = useRef(true), focusIndex = useRef<number | null>(null);
  useLayoutEffect(() => () => { alive.current = false; }, []);
  useLayoutEffect(() => {
    if (focusIndex.current === null) return;
    const actions = section.current?.querySelectorAll<HTMLButtonElement>("[data-grant-action]");
    const target = actions?.[Math.min(focusIndex.current, actions.length - 1)];
    (target ?? section.current)?.focus(); focusIndex.current = null;
  }, [grants]);
  function accountMismatch() {
    setGrants(null); setPending(false); setLoading(false); focusIndex.current = null;
    setOutcome({ error: true, text: "Your account changed. Refreshing your account identity. Refresh connections to check current access." });
    refreshIdentity();
  }
  async function load(signal?: AbortSignal) {
    if (busy.current) return;
    busy.current = true; setLoading(true); setOutcome(null);
    try {
      const result = await api<MCPGrants>("/api/me/mcp-grants", undefined, signal);
      if (!alive.current || signal?.aborted) return;
      if (result?.account_id !== me.id) { accountMismatch(); return; }
      setGrants(result.grants);
    } catch (caught) {
      if (!alive.current || signal?.aborted) return;
      const error = asError(caught);
      if (error.status === 401 || error.code === "stale_authority") { sessionLoss(); return; }
      setOutcome({ error: true, text: errorMessage(error) });
    } finally { busy.current = false; if (alive.current && !signal?.aborted) setLoading(false); }
  }
  useEffect(() => {
    const controller = new AbortController(); void load(controller.signal);
    return () => controller.abort();
  }, []);
  async function revoke(grant: MCPGrant) {
    if (busy.current) return;
    const requestAccount = me.id;
    busy.current = true; setPending(true); setOutcome({ text: `Revoking ${grant.client_name}…` });
    try {
      const result = await api<MCPRevocation>(`/api/me/mcp-grants/${encodeURIComponent(grant.id)}/revoke`, {});
      // A different rendered account has already unmounted this screen.
      if (!alive.current) return;
      if (result?.account_id !== requestAccount) { accountMismatch(); return; }
      focusIndex.current = Math.max(0, grants?.findIndex(item => item.id === grant.id) ?? 0);
      setGrants(current => current?.filter(item => item.id !== grant.id) ?? null);
      setOutcome({ text: `Revoked ${grant.client_name}.` });
    } catch (caught) {
      if (!alive.current) return;
      const error = asError(caught);
      if (error.status === 401 || error.code === "stale_authority") { sessionLoss(); return; }
      setOutcome({ error: true, text: `${errorMessage(error)} Revocation is not confirmed. Refresh connections to check current access.` });
    } finally { busy.current = false; if (alive.current) setPending(false); }
  }
  return <div className="stack"><header className="page-heading"><p className="eyebrow">Your account</p><h1>Connect your devices</h1><p>Use the same account in your browser and native clients.</p></header>
    <Card><CardHeader><CardTitle level={2}>Connection details</CardTitle><CardDescription>Sign in to a compatible client with your username and account password.</CardDescription></CardHeader><CardContent className="stack"><CopyValue label="Server URL" value={me.server_url} /><CopyValue label="Username" value={me.username} /><p>Replacing your password revokes existing client sign-ins. Sign in again on each device afterward.</p></CardContent></Card>
    <Card ref={section} tabIndex={-1} aria-labelledby="mcp-connections"><CardHeader><CardTitle level={2} id="mcp-connections">MCP connections</CardTitle><CardDescription>Manage clients with access to your synced collection.</CardDescription></CardHeader>
      <CardContent className="stack"><CopyValue label="MCP URL" value={new URL("/mcp", me.server_url).href} />
        <div><Button variant="outline" aria-disabled={loading || pending} onClick={() => void load()}>Refresh connections</Button>
          <Outcome value={outcome} />
          {loading ? <p role="status">Loading MCP connections…</p> : grants === null ? <p>Connections could not be loaded. Use Refresh connections to try explicitly.</p>
            : grants.length === 0 ? <p role="status">No MCP connections</p>
            : <ul className="grant-list">{grants.map(grant => <GrantRow key={grant.id} grant={grant} pending={pending} revoke={() => revoke(grant)} />)}</ul>}
        </div>
      </CardContent>
    </Card>
  </div>;
}
