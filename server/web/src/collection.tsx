// SPDX-License-Identifier: MIT
import { useEffect, useState } from "react";
import { api, asError, errorMessage } from "./api";
import type { AccountStorage } from "./api";
import { Card, CardContent, CardHeader, CardTitle, Outcome } from "./ui";
import type { OutcomeValue } from "./ui";
import { storageValue } from "./storage";

export function Collection({ sessionLoss }: { sessionLoss: () => void }) {
  const [storage, setStorage] = useState<AccountStorage | null>(null), [loading, setLoading] = useState(true);
  const [outcome, setOutcome] = useState<OutcomeValue | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    void api<AccountStorage | null>("/api/me/storage", undefined, controller.signal).then(value => {
      if (!controller.signal.aborted) setStorage(value);
    }).catch(caught => {
      if (controller.signal.aborted) return;
      const error = asError(caught);
      if (error.status === 401 || error.code === "stale_authority") { sessionLoss(); return; }
      setOutcome({ error: true, text: errorMessage(error) });
    }).finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, []);
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
  </div>;
}
