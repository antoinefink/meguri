// SPDX-License-Identifier: MIT
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
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
type Action = "reset" | "reset-data" | "disable" | "delete";
function AccountRow({ account, me, sessionLoss, updated, deleted }: { account: Account; me: Me; sessionLoss: () => void; updated: (account: Account) => void; deleted: (account: Account) => void }) {
  const [confirm, setConfirm] = useState<Exclude<Action, "reset"> | null>(null), [busy, setBusy] = useState(false), [open, setOpen] = useState(false);
  const [outcome, setOutcome] = useState<OutcomeValue | null>(null), [link, setLink] = useState<GeneratedLink | null>(null);
  const actionsId = useId(), disclosure = useRef<HTMLDivElement>(null), trigger = useRef<HTMLButtonElement>(null), cancel = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false), submitting = useRef(false), alive = useRef(true);
  useEffect(() => () => { alive.current = false; }, []);
  useLayoutEffect(() => {
    if (confirm) cancel.current?.focus();
    else if (restoreFocus.current) { trigger.current?.focus(); restoreFocus.current = false; }
  }, [confirm]);
  useEffect(() => {
    if (!open) return;
    function outside(event: PointerEvent) {
      if (event.target instanceof Node && !disclosure.current?.contains(event.target)) {
        // Return focus if dismissal would remove the focused button; normal
        // pointer focus can then proceed to the outside control being clicked.
        if (disclosure.current?.contains(document.activeElement)) trigger.current?.focus();
        setOpen(false);
      }
    }
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  function closeConfirmation() { restoreFocus.current = true; setConfirm(null); }
  function choose(action: Action) {
    if (submitting.current || confirm) return;
    setOpen(false); trigger.current?.focus();
    if (action === "reset") void perform(action);
    else { setOutcome(null); setConfirm(action); }
  }
  async function perform(action: Action) {
    if (submitting.current) return;
    submitting.current = true; setBusy(true); setOutcome(null); setLink(null);
    try {
      const result = await api<GeneratedLink | Account | null>(`/api/admin/accounts/${encodeURIComponent(account.id)}/${action}`, {});
      if (!alive.current) return;
      if (action === "reset") {
        setLink(checkLink(result as GeneratedLink, "/reset")); setOutcome({ text: "Reset link created. Earlier reset links for this account are unavailable." });
      } else if (action === "reset-data") {
        const target = result as Account;
        if (!target || target.id !== account.id) throw new Error("Invalid reset result");
        updated(target); closeConfirmation();
        setOutcome({ text: `Reset server data for ${account.username}. Device copies and account sign-in are preserved.` });
      } else if (action === "delete") {
        deleted(account);
      } else {
        closeConfirmation(); updated({ ...account, enabled: false }); setOutcome({ text: "Account disabled. Its stored data is retained." });
        if (account.id === me.id) sessionLoss();
      }
    } catch (caught) {
      if (!alive.current) return;
      const error = asError(caught);
      if (error.status === 401 || error.code === "stale_authority") { sessionLoss(); return; }
      setOutcome({ error: true, text: errorMessage(error) });
    } finally { submitting.current = false; if (alive.current) setBusy(false); }
  }
  const actionName = confirm === "reset-data" ? "reset data for" : confirm;
  return <li className="account-row" aria-label={`Account ${account.username}`}>
    <div className="account-summary"><div className="account-info"><h3>{account.username}{account.id === me.id ? " (you)" : ""}</h3>
      <p>{account.role === "admin" ? "Administrator" : "Member"} · {account.enabled ? "Enabled" : "Disabled"} · {account.provisioning_state === "ready" ? "Ready" : "Provisioning"}</p>
      {account.storage ? <ul className="account-storage" aria-label="Storage on server">
        <li><span>Collection</span><span>{storageValue(account.storage.collection_bytes)}</span></li>
        <li><span>Media</span><span>{storageValue(account.storage.media_bytes)}</span></li>
        <li><span>Total</span><span>{storageValue(account.storage.total_bytes)}</span></li>
      </ul> : <p>Storage unavailable</p>}
    </div>
    <div className="account-actions" ref={disclosure} onBlur={event => {
      if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false);
    }} onKeyDown={event => {
      if (event.key === "Escape" && open) { event.preventDefault(); event.stopPropagation(); setOpen(false); trigger.current?.focus(); }
      if (event.key === "ArrowDown" || event.key === "ArrowUp" || event.key === "Home" || event.key === "End") {
        const buttons = Array.from(disclosure.current?.querySelectorAll<HTMLButtonElement>(".account-actions-panel button") ?? []);
        if (open && buttons.length) {
          event.preventDefault();
          const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
          buttons[event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : index < 0 ? (event.key === "ArrowUp" ? buttons.length - 1 : 0) : (index + (event.key === "ArrowUp" ? -1 : 1) + buttons.length) % buttons.length]?.focus();
        }
      }
    }}>
      <Button ref={trigger} variant="outline" aria-expanded={open} aria-controls={actionsId} aria-disabled={busy || !!confirm} onClick={() => { if (!submitting.current && !confirm) setOpen(value => !value); }}>Actions<span aria-hidden="true"> ▾</span><span className="sr-only"> for {account.username}</span></Button>
      <div id={actionsId} hidden={!open} className="account-actions-panel" role="group" aria-label={`Actions for ${account.username}`}>
        <Button variant="outline" onClick={() => choose("reset")}>Create reset link<span className="sr-only"> for {account.username}</span></Button>
        <div className="account-destructive" role="group" aria-label="Destructive actions">
          {account.provisioning_state === "ready" && <Button variant="outline" onClick={() => choose("reset-data")}>Reset data<span className="sr-only"> for {account.username}</span></Button>}
          {account.enabled && <Button variant="outline" onClick={() => choose("disable")}>Disable<span className="sr-only"> {account.username}</span></Button>}
          {account.role === "member" && <Button variant="outline" onClick={() => choose("delete")}>Delete<span className="sr-only"> {account.username}</span></Button>}
        </div>
      </div>
    </div></div>
    {confirm && <section className="confirmation" aria-label={`Confirm ${confirm === "reset-data" ? "resetting data for" : confirm === "delete" ? "deleting" : "disabling"} ${account.username}`} onKeyDown={event => { if (event.key === "Escape" && !submitting.current) closeConfirmation(); }}>
      <h3>{confirm === "reset-data" ? "Reset data for" : confirm === "delete" ? "Delete" : "Disable"} {account.username}?</h3>
      {confirm === "reset-data" ? <p>This permanently clears {account.username}’s collection and all media from this server. The account and device copies are kept.</p> : confirm === "delete" ? <p>This permanently deletes this member’s account, collection, and media from this server.</p> : <p>This revokes all sign-ins for this {account.role === "admin" ? "administrator" : "member"}. Collection and media data are retained. A reset link will not re-enable access. The sole enabled, ready administrator is protected by the server.</p>}
      <div className="actions"><Button ref={cancel} variant="outline" aria-disabled={busy} onClick={() => { if (!submitting.current) closeConfirmation(); }}>Cancel</Button>
        <Button variant="destructive" aria-disabled={busy} onClick={() => void perform(confirm)}>{busy ? (confirm === "reset-data" ? "Resetting…" : confirm === "delete" ? "Deleting…" : "Disabling…") : `Confirm ${actionName} ${account.username}`}</Button></div>
    </section>}
    <Outcome value={outcome} />{link && <LinkResult link={link} />}
  </li>;
}
export function Accounts({ me, sessionLoss }: { me: Me; sessionLoss: () => void }) {
  const [accounts, setAccounts] = useState<Account[] | null>(null), [busy, setBusy] = useState(false), [loading, setLoading] = useState(true);
  const [outcome, setOutcome] = useState<OutcomeValue | null>(null), [listOutcome, setListOutcome] = useState<OutcomeValue | null>(null);
  const [link, setLink] = useState<GeneratedLink | null>(null);
  const submitting = useRef(false), reading = useRef(false), alive = useRef(true);
  const deletedIds = useRef(new Set<string>()), listHeading = useRef<HTMLHeadingElement>(null), focusList = useRef(false);
  useLayoutEffect(() => { if (focusList.current) { listHeading.current?.focus(); focusList.current = false; } }, [accounts]);
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
    // Read once per navigation/session; mutations update their own rows.
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
    <header className="page-heading"><p className="eyebrow">Administration</p><h1>Accounts</h1><p>Invite and manage accounts, replace access, or reset server data.</p></header>
    <Card><CardHeader><CardTitle level={2}>Invite a member</CardTitle><CardDescription>Create a one-time invitation. The recipient chooses their own username and password.</CardDescription></CardHeader>
      <CardContent><Button aria-disabled={busy} onClick={() => void invite()}>{busy ? "Creating…" : "Create invitation"}</Button><Outcome value={outcome} />{link && <LinkResult link={link} />}</CardContent>
    </Card>
    <section aria-label="Server accounts" aria-busy={loading}><div className="section-heading"><h2 ref={listHeading} tabIndex={-1}>Server accounts <span role="status">{accounts && <><span className="account-count" aria-hidden="true">{accounts.length}</span><span className="sr-only">{`${accounts.length} ${accounts.length === 1 ? "account" : "accounts"}`}</span></>}</span></h2></div>
      <Outcome value={listOutcome} /><p role="status">{loading ? "Loading accounts…" : accounts ? "" : "Accounts could not be loaded. Reload the page to try again."}</p>
      {accounts && <ul className="account-list">{accounts.map(account => <AccountRow key={account.id} account={account} me={me} sessionLoss={sessionLoss} deleted={deleted} updated={target => setAccounts(current => current?.map(item => item.id === target.id ? target : item) ?? null)} />)}</ul>}
    </section>
  </div>;
}
