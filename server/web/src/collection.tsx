// SPDX-License-Identifier: MIT
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, asError, errorMessage } from "./api";
import type { AccountStorage, CollectionBackup } from "./api";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Outcome } from "./ui";
import type { OutcomeValue } from "./ui";
import { storageValue } from "./storage";

const backupKind: Record<CollectionBackup["kind"], string> = { daily: "Daily", weekly: "Weekly", monthly: "Monthly", before_restore: "Before restore" };
function backupDate(backup: CollectionBackup) {
  const date = new Date(backup.created_at * 1000);
  return { iso: date.toISOString(), day: date.toLocaleDateString("en", { month: "short", day: "numeric", year: "numeric" }),
    time: date.toLocaleTimeString("en", { hour: "numeric", minute: "2-digit", timeZoneName: "short" }) };
}
function backupLabel(backup: CollectionBackup) { const date = backupDate(backup); return `${date.day}, ${date.time}`; }

export function Collection({ sessionLoss }: { sessionLoss: () => void }) {
  const [storage, setStorage] = useState<AccountStorage | null>(null), [loading, setLoading] = useState(true);
  const [outcome, setOutcome] = useState<OutcomeValue | null>(null);
  const [backups, setBackups] = useState<CollectionBackup[] | null>(null), [historyLoading, setHistoryLoading] = useState(true);
  const [historyOutcome, setHistoryOutcome] = useState<OutcomeValue | null>(null), [restoreOutcome, setRestoreOutcome] = useState<OutcomeValue | null>(null);
  const [selected, setSelected] = useState<CollectionBackup | null>(null), [busy, setBusy] = useState(false);
  const alive = useRef(true), authorityLost = useRef(false), submitting = useRef(false), readingStorage = useRef(false), readingHistory = useRef(false);
  const controller = useRef(new AbortController()), cancel = useRef<HTMLButtonElement>(null), trigger = useRef<HTMLButtonElement | null>(null);
  const returnFocus = useRef(false), historyHeading = useRef<HTMLHeadingElement>(null);
  useLayoutEffect(() => {
    if (selected) cancel.current?.focus();
    else if (returnFocus.current) {
      if (trigger.current?.isConnected) trigger.current.focus(); else historyHeading.current?.focus();
      returnFocus.current = false;
    }
  }, [selected]);
  function handleError(caught: unknown, report: (value: OutcomeValue) => void) {
    if (!alive.current || authorityLost.current) return;
    const error = asError(caught);
    if (error.status === 401 || error.code === "stale_authority") {
      authorityLost.current = true; controller.current.abort(); sessionLoss(); return;
    }
    report({ error: true, text: errorMessage(error) });
  }
  async function loadStorage() {
    if (readingStorage.current || authorityLost.current) return;
    readingStorage.current = true; setLoading(true); setOutcome(null);
    try {
      const value = await api<AccountStorage | null>("/api/me/storage", undefined, controller.current.signal);
      if (alive.current && !authorityLost.current) setStorage(value);
    } catch (caught) { handleError(caught, setOutcome); }
    finally { readingStorage.current = false; if (alive.current && !authorityLost.current) setLoading(false); }
  }
  async function loadHistory() {
    if (readingHistory.current || authorityLost.current) return;
    readingHistory.current = true; setHistoryLoading(true); setHistoryOutcome(null);
    try {
      const value = await api<CollectionBackup[]>("/api/me/backups", undefined, controller.current.signal);
      if (alive.current && !authorityLost.current) setBackups(value);
    } catch (caught) { handleError(caught, setHistoryOutcome); }
    finally { readingHistory.current = false; if (alive.current && !authorityLost.current) setHistoryLoading(false); }
  }
  useEffect(() => {
    void loadStorage(); void loadHistory();
    return () => { alive.current = false; controller.current.abort(); };
  }, []);
  function closeConfirmation(focusHistory = false) {
    if (focusHistory) trigger.current = null;
    returnFocus.current = true; setSelected(null);
  }
  async function restore() {
    if (!selected || submitting.current || authorityLost.current || readingHistory.current || readingStorage.current) return;
    submitting.current = true; setBusy(true); setRestoreOutcome(null);
    try {
      await api<null>(`/api/me/backups/${encodeURIComponent(selected.id)}/restore`, {}, controller.current.signal);
      if (!alive.current || authorityLost.current) return;
      closeConfirmation(true);
      setRestoreOutcome({ text: `Restored the server collection and media from ${backupLabel(selected)}. On each device, download the restored server copy before continuing to study or sync.` });
      await Promise.all([loadHistory(), loadStorage()]);
    } catch (caught) { handleError(caught, setRestoreOutcome); }
    finally { submitting.current = false; if (alive.current && !authorityLost.current) setBusy(false); }
  }
  const blocked = busy || loading || historyLoading || !!historyOutcome || !!selected;
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
      <CardDescription>Recover your collection database and media from an earlier point.</CardDescription>
      <p className="muted">Daily backups for seven days, then four weekly and twelve monthly backups.</p>
    </CardHeader><CardContent>
      <Outcome value={restoreOutcome} />
      {selected && <section id="backup-restore-confirmation" className="confirmation backup-confirmation" aria-labelledby="backup-restore-title" aria-describedby="backup-restore-effects" onKeyDown={event => {
        if (event.key === "Escape" && !submitting.current) { event.preventDefault(); closeConfirmation(); }
      }}>
        <h3 id="backup-restore-title">Restore backup from {backupLabel(selected)}?</h3>
        <div id="backup-restore-effects" className="stack">
          <p>This replaces the collection database, including review history and scheduling, and all media on this server with this backup.</p>
          <p>A safety copy of the current server collection and media is saved before restoring.</p>
          <p>After restoring, download the restored server copy on each device before continuing to study or sync.</p>
        </div>
        <div className="actions"><Button ref={cancel} variant="outline" aria-disabled={busy} onClick={() => { if (!submitting.current) closeConfirmation(); }}>Cancel</Button>
          <Button variant="destructive" aria-disabled={busy} onClick={() => void restore()}>{busy ? "Restoring…" : "Restore backup"}</Button></div>
        <p role="status">{busy ? "Restoring the server collection and media…" : ""}</p>
      </section>}
      <Outcome value={historyOutcome} />
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
              if (blocked || submitting.current || authorityLost.current || readingHistory.current || readingStorage.current) return;
              trigger.current = event.currentTarget; setRestoreOutcome(null); setSelected(backup);
            }}>Restore<span className="sr-only"> backup from {backupLabel(backup)}</span></Button>
          </li>;
        })}</ul>
      </>}
    </CardContent></Card>
  </div>;
}
