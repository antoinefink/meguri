// SPDX-License-Identifier: MIT
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, asError, errorMessage } from "./api";
import type { Account, GeneratedLink, Me } from "./api";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, CopyValue, Outcome } from "./ui";
import type { OutcomeValue } from "./ui";
import { storageValue } from "./storage";

function LinkResult({ link }: { link: GeneratedLink }) {
  return <div className="link-result"><CopyValue label="One-time link" value={link.url}>
    <p className="muted">Expires <time dateTime={link.expires_at}>{new Date(link.expires_at).toLocaleString()}</time>. Share this link privately. It grants access to its recipient.</p>
  </CopyValue></div>;
}
function checkLink(link: GeneratedLink, path: "/invite" | "/reset"): GeneratedLink {
  try {
    const url = new URL(link.url);
    if (url.origin !== location.origin || url.pathname !== path || url.search || !/^#token=[A-Za-z0-9_-]{43}$/.test(url.hash) || !Number.isFinite(Date.parse(link.expires_at))) throw new Error();
  } catch { throw new Error("Invalid link result"); }
  return link;
}
function AccountRow({ account, me, sessionLoss, disabled, deleted }: { account: Account; me: Me; sessionLoss: () => void; disabled: (id: string) => void; deleted: (account: Account) => void }) {
  const [confirm, setConfirm] = useState<"disable" | "delete" | null>(null), [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<OutcomeValue | null>(null), [link, setLink] = useState<GeneratedLink | null>(null);
  const trigger = useRef<HTMLButtonElement>(null), deleteTrigger = useRef<HTMLButtonElement>(null), confirmationTrigger = useRef<HTMLButtonElement | null>(null), resetTrigger = useRef<HTMLButtonElement>(null), cancel = useRef<HTMLButtonElement>(null), restoreFocus = useRef(false), submitting = useRef(false), alive = useRef(true);
  useEffect(() => () => { alive.current = false; }, []);
  useLayoutEffect(() => {
    if (confirm) cancel.current?.focus();
    else if (restoreFocus.current) { (confirmationTrigger.current?.isConnected ? confirmationTrigger.current : resetTrigger.current)?.focus(); restoreFocus.current = false; }
  }, [confirm]);
  function closeConfirmation() { restoreFocus.current = true; setConfirm(null); }
  async function perform(action: "reset" | "disable" | "delete") {
    if (submitting.current) return;
    submitting.current = true; setBusy(true); setOutcome(null); setLink(null);
    try {
      const result = await api<GeneratedLink | null>(`/api/admin/accounts/${encodeURIComponent(account.id)}/${action}`, {});
      if (!alive.current) return;
      if (action === "reset") {
        setLink(checkLink(result!, "/reset")); setOutcome({ text: "Reset link created. Earlier reset links for this account are unavailable." });
      } else if (action === "delete") {
        deleted(account);
      } else {
        closeConfirmation(); disabled(account.id); setOutcome({ text: "Account disabled. Its stored data is retained." });
        if (account.id === me.id) sessionLoss();
      }
    } catch (caught) {
      if (!alive.current) return;
      const error = asError(caught);
      if (error.status === 401 || error.code === "stale_authority") { sessionLoss(); return; }
      setOutcome({ error: true, text: errorMessage(error) });
    } finally { submitting.current = false; if (alive.current) setBusy(false); }
  }
  return <li className="account-row" aria-label={`Account ${account.username}`}>
    <div className="account-info"><h2>{account.username}{account.id === me.id ? " (you)" : ""}</h2>
      <p>{account.role === "admin" ? "Administrator" : "Member"} · {account.enabled ? "Enabled" : "Disabled"} · {account.provisioning_state === "ready" ? "Ready" : "Provisioning"}</p>
      {account.storage ? <ul className="account-storage" aria-label="Storage on server">
        <li><span>Collection</span><span>{storageValue(account.storage.collection_bytes)}</span></li>
        <li><span>Media</span><span>{storageValue(account.storage.media_bytes)}</span></li>
        <li><span>Total</span><span>{storageValue(account.storage.total_bytes)}</span></li>
      </ul> : <p>Storage unavailable</p>}
    </div>
    <div className="actions" role="group" aria-label={`Actions for ${account.username}`}>
      <Button ref={resetTrigger} variant="outline" aria-disabled={busy || !!confirm} onClick={() => { if (!confirm) void perform("reset"); }}>Create reset link<span className="sr-only"> for {account.username}</span></Button>
      {account.enabled && <Button ref={trigger} variant="outline" aria-disabled={busy || !!confirm} onClick={() => { if (!submitting.current && !confirm) { confirmationTrigger.current = trigger.current; setOutcome(null); setConfirm("disable"); } }}>Disable<span className="sr-only"> {account.username}</span></Button>}
      {account.role === "member" && <Button ref={deleteTrigger} variant="outline" aria-disabled={busy || !!confirm} onClick={() => { if (!submitting.current && !confirm) { confirmationTrigger.current = deleteTrigger.current; setOutcome(null); setConfirm("delete"); } }}>Delete<span className="sr-only"> {account.username}</span></Button>}
    </div>
    {confirm && <section className="confirmation" aria-label={`Confirm ${confirm === "delete" ? "deleting" : "disabling"} ${account.username}`} onKeyDown={event => { if (event.key === "Escape" && !submitting.current) closeConfirmation(); }}>
      <h3>{confirm === "delete" ? "Delete" : "Disable"} {account.username}?</h3>
      {confirm === "delete" ? <p>This permanently deletes this member’s account, collection, and media from this server.</p> : <p>This revokes all sign-ins for this {account.role === "admin" ? "administrator" : "member"}. Collection and media data are retained. A reset link will not re-enable access. The sole enabled, ready administrator is protected by the server.</p>}
      <div className="actions"><Button ref={cancel} variant="outline" aria-disabled={busy} onClick={() => { if (!submitting.current) closeConfirmation(); }}>Cancel</Button>
        <Button variant="destructive" aria-disabled={busy} onClick={() => void perform(confirm)}>{busy ? (confirm === "delete" ? "Deleting…" : "Disabling…") : `Confirm ${confirm} ${account.username}`}</Button></div>
    </section>}
    <Outcome value={outcome} />{link && <LinkResult link={link} />}
  </li>;
}
export function Accounts({ me, sessionLoss }: { me: Me; sessionLoss: () => void }) {
  const [accounts, setAccounts] = useState<Account[] | null>(null), [busy, setBusy] = useState(false), [loading, setLoading] = useState(true);
  const [outcome, setOutcome] = useState<OutcomeValue | null>(null), [listOutcome, setListOutcome] = useState<OutcomeValue | null>(null);
  const [link, setLink] = useState<GeneratedLink | null>(null);
  const submitting = useRef(false), reading = useRef(false), alive = useRef(true);
  const deletedIds = useRef(new Set<string>()), refresh = useRef<HTMLButtonElement>(null), focusList = useRef(false);
  useLayoutEffect(() => { if (focusList.current) { refresh.current?.focus(); focusList.current = false; } }, [accounts]);
  function deleted(account: Account) {
    deletedIds.current.add(account.id);
    focusList.current = true;
    setAccounts(current => current?.filter(item => item.id !== account.id) ?? null);
    setListOutcome({ text: `Deleted ${account.username}.` });
  }
  async function load(signal?: AbortSignal) {
    if (reading.current) return;
    reading.current = true; setLoading(true); setListOutcome(null);
    try {
      const result = await api<{ accounts: Account[] }>("/api/admin/accounts", undefined, signal);
      if (alive.current && !signal?.aborted) setAccounts(result.accounts.filter(account => !deletedIds.current.has(account.id)));
    } catch (caught) {
      if (!alive.current || signal?.aborted) return;
      const error = asError(caught);
      if (error.status === 401 || error.code === "stale_authority") { sessionLoss(); return; }
      setListOutcome({ error: true, text: errorMessage(error) });
    } finally { reading.current = false; if (alive.current) setLoading(false); }
  }
  useEffect(() => {
    const controller = new AbortController();
    void load(controller.signal);
    return () => { alive.current = false; controller.abort(); };
    // The page mounts once per navigation/session; refresh is explicit.
  }, []);
  async function invite() {
    if (submitting.current) return;
    submitting.current = true; setBusy(true); setOutcome(null); setLink(null);
    try {
      const result = await api<GeneratedLink>("/api/admin/invitations", {});
      if (!alive.current) return;
      setLink(checkLink(result, "/invite")); setOutcome({ text: "Invitation created. Share the one-time link privately." });
    } catch (caught) {
      if (!alive.current) return;
      const error = asError(caught);
      if (error.status === 401 || error.code === "stale_authority") { sessionLoss(); return; }
      setOutcome({ error: true, text: errorMessage(error) });
    } finally { submitting.current = false; if (alive.current) setBusy(false); }
  }
  return <div className="stack">
    <header className="page-heading"><p className="eyebrow">Administration</p><h1>Accounts</h1><p>Invite and manage members, replace access, or delete a member’s server data.</p></header>
    <Card><CardHeader><CardTitle level={2}>Invite a member</CardTitle><CardDescription>Create a one-time invitation. The recipient chooses their own username and password.</CardDescription></CardHeader>
      <CardContent><Button aria-disabled={busy} onClick={() => void invite()}>{busy ? "Creating…" : "Create invitation"}</Button><Outcome value={outcome} />{link && <LinkResult link={link} />}</CardContent>
    </Card>
    <section aria-label="Server accounts"><div className="section-heading"><h2>Server accounts</h2><Button ref={refresh} variant="outline" aria-disabled={loading} onClick={() => void load()}>Refresh accounts</Button></div>
      <Outcome value={listOutcome} /><p role="status">{loading ? "Loading accounts…" : accounts ? `${accounts.length} ${accounts.length === 1 ? "account" : "accounts"}` : "Accounts could not be loaded. Use Refresh accounts to try explicitly."}</p>
      {accounts && <ul className="account-list">{accounts.map(account => <AccountRow key={account.id} account={account} me={me} sessionLoss={sessionLoss} deleted={deleted} disabled={id => setAccounts(current => current?.map(item => item.id === id ? { ...item, enabled: false } : item) ?? null)} />)}</ul>}
    </section>
  </div>;
}
