// SPDX-License-Identifier: MIT
// M9: native form navigation lets the browser stream the attachment to disk.
import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import type { Me } from "./api";
import { CredentialsForm } from "./forms";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Outcome } from "./ui";
import type { OutcomeValue } from "./ui";

const newLink = "Return to your chat to request a new download link.";
export function CollectionDownload({ ticket, me, onLogin, onSessionLoss, onSignOut, signingOut, link }: {
  ticket: string; me: Me | null; onLogin: (me: Me) => void; onSessionLoss: () => void;
  onSignOut: () => void; signingOut: boolean; link: (path: string, label: string) => ReactNode;
}) {
  // Native navigation POSTs can send Origin: null under no-referrer. The
  // fragment is already gone; this route alone permits a clean same-origin
  // referrer so the existing exact-Origin check also protects the native form.
  useEffect(() => {
    const policy = document.querySelector<HTMLMetaElement>('meta[name="referrer"]');
    if (!policy) return;
    const previous = policy.content; policy.content = "same-origin";
    return () => { policy.content = previous; };
  }, []);
  const submitted = useRef(false);
  const [requested, setRequested] = useState(false), [outcome, setOutcome] = useState<OutcomeValue | null>(null);
  if (!me && ticket) return <CredentialsForm mode="login" token="" clearToken={() => {}} onLogin={identity => {
    submitted.current = false; setRequested(false); setOutcome(null); onLogin(identity);
  }} onSuccess={() => {}} onSessionLoss={onSessionLoss} link={link}
    compactLoginCopy={{ description: "Sign in to the same Meguri account connected to your chat to download your collection." }} />;
  return <Card className="auth-card"><CardHeader><CardTitle>Download collection</CardTitle>
    <CardDescription>Save your synced collection as collection.anki2. Media files are separate.</CardDescription>
  </CardHeader><CardContent className="stack">
    <Outcome value={!ticket ? { error: true, text: newLink } : outcome} />
    {me && <div role="group" aria-label="Signed-in account" className="stack"><p>Signed in as {me.username}.</p>
      <Button variant="outline" disabled={signingOut} onClick={() => {
        submitted.current = false; setRequested(false); setOutcome(null); onSignOut();
      }}>{signingOut ? "Signing out…" : `Sign out ${me.username}`}</Button>
    </div>}
    {me && ticket && <form method="post" action="/api/mcp/collection-download" encType="application/x-www-form-urlencoded" target="_blank" rel="noopener"
      onSubmit={event => {
        if (submitted.current || signingOut) { event.preventDefault(); return; }
        submitted.current = true; setRequested(true);
        setOutcome({ text: "Download requested. Check your browser’s downloads or the new tab. If it failed, try again here or switch accounts. For an expired or used link, request a new link in your chat." });
      }}>
      <input type="hidden" name="ticket" value={ticket} />
      <Button type="submit" disabled={requested || signingOut}>Download collection</Button>
    </form>}
    {me && ticket && requested && <Button variant="outline" disabled={signingOut} onClick={() => {
      submitted.current = false; setRequested(false); setOutcome(null);
    }}>Try again</Button>}
  </CardContent></Card>;
}
