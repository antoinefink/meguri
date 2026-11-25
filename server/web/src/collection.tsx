// SPDX-License-Identifier: MIT
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, ApiError, asError, errorMessage, parseBackupHistory } from "./api";
import type { AccountStorage, BackupRestore, CollectionBackup } from "./api";
import { Button, Card, CardContent, CardHeader, CardTitle, Outcome } from "./ui";
import type { OutcomeValue } from "./ui";
import { storageValue } from "./storage";

const backupKind: Record<CollectionBackup["kind"], string> = { daily: "Daily", weekly: "Weekly", monthly: "Monthly", before_restore: "Before restore" };
function backupDate(backup: CollectionBackup) {
  const date = new Date(backup.created_at * 1000);
  return { iso: date.toISOString(), day: date.toLocaleDateString("en", { month: "short", day: "numeric", year: "numeric" }),
    time: date.toLocaleTimeString("en", { hour: "numeric", minute: "2-digit", timeZoneName: "short" }) };
}
function backupLabel(backup: CollectionBackup) { const date = backupDate(backup); return `${date.day}, ${date.time}`; }

export function Collection({ accountId, sessionLoss }: { accountId: string; sessionLoss: () => void }) {
  const [storage, setStorage] = useState<AccountStorage | null>(null), [loading, setLoading] = useState(true);
  const [outcome, setOutcome] = useState<OutcomeValue | null>(null);
  const [backups, setBackups] = useState<CollectionBackup[] | null>(null), [historyLoading, setHistoryLoading] = useState(true);
  const [historyOutcome, setHistoryOutcome] = useState<OutcomeValue | null>(null), [restoreOutcome, setRestoreOutcome] = useState<OutcomeValue | null>(null);
  const [selected, setSelected] = useState<CollectionBackup | null>(null), [busy, setBusy] = useState(false);
  const [restoreUnknown, setRestoreUnknown] = useState(false);
  const alive = useRef(true), authorityLost = useRef(false), submitting = useRef(false), readingHistory = useRef(false);
  const storageRequest = useRef<AbortController | null>(null);
  const controller = useRef(new AbortController()), cancel = useRef<HTMLButtonElement>(null), trigger = useRef<HTMLButtonElement | null>(null);
  const returnFocus = useRef(false), historyHeading = useRef<HTMLHeadingElement>(null);
  useLayoutEffect(() => {
    if (selected) cancel.current?.focus();
    else if (returnFocus.current) {
      if (document.activeElement?.getAttribute("role") !== "alert") {
        if (trigger.current?.isConnected) trigger.current.focus(); else historyHeading.current?.focus();
      }
      returnFocus.current = false;
    }
  }, [selected]);
  function handleError(caught: unknown, report: (value: OutcomeValue) => void) {
    if (!alive.current || authorityLost.current) return;
    const error = asError(caught);
    if (error.status === 401 || error.code === "stale_authority") {
      authorityLost.current = true; controller.current.abort(); storageRequest.current?.abort(); sessionLoss(); return;
    }
    report({ error: true, text: errorMessage(error) });
  }
  async function loadStorage() {
    if (authorityLost.current) return;
    storageRequest.current?.abort();
    const request = new AbortController(); storageRequest.current = request;
    setLoading(true); setStorage(null); setOutcome(null);
    try {
      const value = await api<AccountStorage | null>("/api/me/storage", undefined, request.signal);
      if (storageRequest.current === request && alive.current && !authorityLost.current) setStorage(value);
    } catch (caught) { if (storageRequest.current === request) handleError(caught, setOutcome); }
    finally { if (storageRequest.current === request && alive.current && !authorityLost.current) setLoading(false); }
  }
  async function loadHistory() {
    if (readingHistory.current || authorityLost.current) return;
    readingHistory.current = true; setHistoryLoading(true); setHistoryOutcome(null);
    if (restoreUnknown) void loadStorage();
    try {
      const value = parseBackupHistory(await api<unknown>("/api/me/backups", undefined, controller.current.signal, accountId), accountId);
      if (alive.current && !authorityLost.current) { setBackups(value); setRestoreUnknown(false); }
    } catch (caught) { handleError(caught, setHistoryOutcome); }
    finally { readingHistory.current = false; if (alive.current && !authorityLost.current) setHistoryLoading(false); }
  }
  useEffect(() => {
    void loadStorage(); void loadHistory();
    return () => { alive.current = false; controller.current.abort(); storageRequest.current?.abort(); };
  }, []);
  function closeConfirmation(focusHistory = false) {
    if (focusHistory) trigger.current = null;
    returnFocus.current = true; setSelected(null);
  }
  async function restore() {
    if (!selected || submitting.current || authorityLost.current || readingHistory.current || restoreUnknown) return;
    submitting.current = true; setBusy(true); setRestoreOutcome(null);
    try {
      const result = await api<BackupRestore>(`/api/me/backups/${encodeURIComponent(selected.id)}/restore`, {}, controller.current.signal, accountId);
      if (!result || typeof result.account_id !== "string" || !result.account_id.length) throw new ApiError("invalid_response");
      if (result.account_id !== accountId) throw new ApiError("stale_authority");
      if (!alive.current || authorityLost.current) return;
      closeConfirmation(true);
      setRestoreOutcome({ text: `Restored the server collection and media from ${backupLabel(selected)}. On each device, download the restored server copy before continuing to study or sync.` });
      void loadStorage(); await loadHistory();
    } catch (caught) {
      handleError(caught, value => {
        const error = asError(caught);
        if (error.status === 0 || error.status >= 500 || error.code === "invalid_response") {
          storageRequest.current?.abort(); storageRequest.current = null;
          setStorage(null); setLoading(false); setOutcome(null);
          setRestoreUnknown(true); closeConfirmation(true);
          setRestoreOutcome({ error: true, text: "The restore result is unknown. Refresh history and review the backups before choosing another restore. No retry was sent automatically." });
        } else setRestoreOutcome(value);
      });
    }
    finally { submitting.current = false; if (alive.current && !authorityLost.current) setBusy(false); }
  }
  function dismissRestoreError() { setRestoreOutcome(null); closeConfirmation(true); historyHeading.current?.focus(); }
  const blocked = busy || restoreUnknown || historyLoading || !!historyOutcome || !!selected;
  return <div className="stack">
    <header className="page-heading"><h1>Collection</h1></header>
    <Card><CardHeader><CardTitle level={2}>Storage on server</CardTitle></CardHeader>
      <CardContent><Outcome value={outcome} />
        {loading ? <p role="status">Loading storage…</p> : outcome ? null : storage ? <dl className="collection-storage">
          <div><dt>Collection database</dt><dd>{storageValue(storage.collection_bytes)}</dd></div>
          <div><dt>Media</dt><dd>{storageValue(storage.media_bytes)}</dd></div>
          <div><dt>Total</dt><dd>{storageValue(storage.total_bytes)}</dd></div>
        </dl> : <p role="status">Storage unavailable</p>}
      </CardContent>
    </Card>
    <Card aria-labelledby="backup-history-title"><CardHeader>
      <div className="backup-heading"><h2 ref={historyHeading} id="backup-history-title" tabIndex={-1}>Backup history</h2>
        <Button variant="outline" aria-disabled={busy || historyLoading || !!selected} onClick={() => {
          if (!submitting.current && !selected) void loadHistory();
        }}>Refresh history</Button></div>
      <p className="muted">Daily backups for seven days, then four weekly and twelve monthly backups.</p>
    </CardHeader><CardContent>
      <div onKeyDown={event => { if (event.key === "Escape" && restoreOutcome?.error) { event.preventDefault(); dismissRestoreError(); } }}>
        <Outcome value={restoreOutcome} />
        {restoreOutcome?.error && <Button variant="outline" onClick={dismissRestoreError}>Dismiss</Button>}
      </div>
      {restoreUnknown && <p>Restore is unavailable until you refresh and review backup history.</p>}
      {selected && <section id="backup-restore-confirmation" className="confirmation backup-confirmation" aria-labelledby="backup-restore-title" aria-describedby="backup-restore-effects" onKeyDown={event => {
        if (event.key === "Escape" && !submitting.current) { event.preventDefault(); closeConfirmation(); }
      }}>
        <h3 id="backup-restore-title">Restore backup from {backupLabel(selected)}?</h3>
        <div id="backup-restore-effects" className="stack">
          <p>This replaces your collection on the server, including its review history, scheduling and media.</p>
          <p>A safety copy of the current server collection and media is saved before restoring.</p>
          <p>After restoring, download the restored server copy on each device before continuing to study or sync.</p>
        </div>
        <div className="actions"><Button ref={cancel} variant="outline" aria-disabled={busy} onClick={() => { if (!submitting.current) closeConfirmation(); }}>Cancel</Button>
          <Button variant="destructive" aria-disabled={busy} onClick={() => void restore()}>{busy ? "Restoring…" : "Restore backup"}</Button></div>
        <p role="status">{busy ? "Restoring the server collection and media…" : ""}</p>
      </section>}
      <div onKeyDown={event => { if (event.key === "Escape" && historyOutcome) { event.preventDefault(); historyHeading.current?.focus(); } }}>
        <Outcome value={historyOutcome} />
      </div>
      <p role="status">{historyLoading ? "Loading backup history…" : backups && !historyOutcome ? `${backups.length} ${backups.length === 1 ? "backup" : "backups"} available.` : ""}</p>
      {!historyLoading && !historyOutcome && backups?.length === 0 && <p className="backup-empty">No backups yet. Backups are created automatically each day while the server is running.</p>}
      {!!backups?.length && <>
        <div className="backup-columns" aria-hidden="true"><span>Created</span><span>Type</span><span>Size</span><span /></div>
        <ul className="backup-list" aria-label="Collection backups">{backups.map(backup => {
          const date = backupDate(backup);
          return <li key={backup.id} className="backup-row">
            <time dateTime={date.iso}>{date.day}<span className="muted">{date.time}</span></time>
            <span className="backup-kind"><span className="sr-only">Type: </span>{backupKind[backup.kind]}</span>
            <span className="backup-size"><span className="sr-only">Size: </span>{storageValue(backup.size_bytes)}</span>
            <Button variant="outline" aria-disabled={blocked} aria-expanded={selected?.id === backup.id} aria-controls={selected?.id === backup.id ? "backup-restore-confirmation" : undefined} onClick={event => {
              if (blocked || submitting.current || authorityLost.current || readingHistory.current) return;
              trigger.current = event.currentTarget; setRestoreOutcome(null); setSelected(backup);
            }}>Restore<span className="sr-only"> backup from {backupLabel(backup)}</span></Button>
          </li>;
        })}</ul>
      </>}
    </CardContent></Card>
  </div>;
}
