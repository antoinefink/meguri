// SPDX-License-Identifier: MIT
// M8: one ephemeral, signed-in file handoff using the shared account primitives.
import { useEffect, useId, useRef, useState } from "react";
import type { ReactNode } from "react";
import { asError, uploadMedia } from "./api";
import type { Me } from "./api";
import { CredentialsForm } from "./forms";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Field, FieldDescription, FieldGroup, FieldLabel, Input, Outcome } from "./ui";
import type { OutcomeValue } from "./ui";

const returnToChat = "Return to your chat to request a new upload link. Reloading this page cannot restore it.";
export function MediaUpload({ uploadId, clearUpload, me, onLogin, onSessionLoss, onSignOut, signingOut, link }: {
  uploadId: string; clearUpload: () => void; me: Me | null; onLogin: (me: Me) => void;
  onSessionLoss: () => void; onSignOut: () => void; signingOut: boolean; link: (path: string, label: string) => ReactNode;
}) {
  const prefix = useId(), alive = useRef(true), submitting = useRef(false), finished = useRef(false);
  const controller = useRef<AbortController | null>(null), account = useRef(me?.id);
  const [file, setFile] = useState<File | null>(null), [pending, setPending] = useState(false), [done, setDone] = useState(false);
  const [refused, setRefused] = useState(false);
  const [outcome, setOutcome] = useState<OutcomeValue | null>(() => uploadId ? null : { error: true, text: returnToChat });
  useEffect(() => () => { alive.current = false; controller.current?.abort(); }, []);
  useEffect(() => {
    if (account.current !== me?.id && !finished.current) {
      controller.current?.abort(); submitting.current = false; setFile(null); setPending(false);
      setRefused(false); setOutcome(null);
    }
    account.current = me?.id;
  }, [me?.id]);
  async function submit() {
    if (submitting.current || finished.current || !uploadId || !file || !me) return;
    if (!file.size || file.size > 8 * 1024 * 1024) {
      setOutcome({ error: true, text: "Choose a nonempty PNG, JPEG or MP3 file up to 8 MiB." }); return;
    }
    submitting.current = true; setPending(true); setOutcome({ text: "Uploading file…" });
    const request = new AbortController(); controller.current = request;
    let recoverable = false;
    try {
      const result = await uploadMedia(uploadId, file, request.signal);
      if (!alive.current || request.signal.aborted) return;
      if (!result?.uploaded) throw new Error("invalid_response");
      setDone(true); setOutcome({ text: "File uploaded. Return to your chat to attach it to your note before the link expires." });
    } catch (caught) {
      if (!alive.current || request.signal.aborted) return;
      const error = asError(caught);
      // A generic capability refusal can be the wrong browser account. The
      // server has not consumed this ID; retain it only in page memory so the
      // member can switch accounts. Expired/used links will still be refused.
      recoverable = error.code === "capability_invalid" && (error.status === 403 || error.status === 410);
      if (recoverable) {
        setRefused(true);
        setOutcome({ error: true, text: `This link is unavailable for the current account. Sign out and sign in to the Meguri account connected to your chat to try again. If the link has expired or was already used, ${returnToChat}` });
        return;
      }
      if (error.status === 401) onSessionLoss();
      setOutcome({ error: true, text: error.code === "attachment_invalid" || error.status === 413
        ? "The file was not accepted. Use a PNG, JPEG or MP3 file up to 8 MiB and request a new link in your chat."
        : `The upload could not be confirmed. ${returnToChat}` });
    } finally {
      if (!request.signal.aborted) {
        finished.current = !recoverable; submitting.current = false;
        if (alive.current) { if (!recoverable) clearUpload(); setFile(null); setPending(false); }
      }
    }
  }
  if (!me && uploadId && !finished.current) return <CredentialsForm mode="login" token="" clearToken={() => {}} onLogin={onLogin} onSuccess={() => {}}
    onSessionLoss={() => { finished.current = true; clearUpload(); onSessionLoss(); setOutcome({ error: true, text: returnToChat }); }} link={link}
    compactLoginCopy={{ description: "Sign in to the same Meguri account connected to your chat to upload a file." }} />;
  return <Card className="auth-card"><CardHeader><CardTitle>Attach a file</CardTitle>
    {!done && uploadId && <CardDescription>Choose one PNG, JPEG or MP3 file up to 8 MiB.</CardDescription>}
  </CardHeader><CardContent className="stack"><Outcome value={outcome} />
    {me && <div role="group" aria-label="Signed-in account" className="stack">
      <p>Signed in as {me.username}.</p>
      <Button variant="outline" disabled={pending || signingOut} onClick={onSignOut}>{signingOut ? "Signing out…" : `Sign out ${me.username}`}</Button>
    </div>}
    {me && uploadId && !finished.current && !refused && <form onSubmit={event => { event.preventDefault(); void submit(); }} aria-busy={pending}>
      <FieldGroup><Field><FieldLabel htmlFor={`${prefix}-file`}>File</FieldLabel>
        <Input id={`${prefix}-file`} type="file" accept="image/png,image/jpeg,audio/mpeg,.png,.jpg,.jpeg,.mp3" disabled={pending}
          aria-describedby={`${prefix}-acceptance`} onChange={event => { setFile(event.target.files?.[0] ?? null); setOutcome(null); }} />
        <FieldDescription id={`${prefix}-acceptance`}>After uploading, return to your chat to finish attaching the file.</FieldDescription>
      </Field><Button type="submit" disabled={!file || pending}>{pending ? "Uploading…" : "Upload file"}</Button></FieldGroup>
    </form>}
  </CardContent></Card>;
}
