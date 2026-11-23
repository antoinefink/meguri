// SPDX-License-Identifier: MIT
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { accessLabel, api, ApiError, asError, errorMessage } from "./api";
import type { MCPAccess, MCPAuthorization, Me } from "./api";
import { CredentialsForm } from "./forms";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Field, FieldDescription, FieldGroup, FieldLabel, Outcome } from "./ui";
import type { OutcomeValue } from "./ui";

const restartMessage = "Return to your client and start connecting again. This request is unavailable: it may have expired, already been used, or lost its original session. Reloading this page cannot restore it.";

export function Consent({ request, clearRequest, me, onLogin, onSessionLoss, link }: {
  request: string; clearRequest: () => void; me: Me | null; onLogin: (me: Me) => void;
  onSessionLoss: () => void; link: (path: string, label: string) => ReactNode;
}) {
  const prefix = useId();
  const [previewName, setPreviewName] = useState<string | null>(null);
  const [authorization, setAuthorization] = useState<MCPAuthorization | null>(null), [access, setAccess] = useState<MCPAccess>("read");
  const [phase, setPhase] = useState<"opening" | "ready" | "unavailable" | "redirecting">(request ? "opening" : "unavailable");
  const [outcome, setOutcome] = useState<OutcomeValue | null>(request ? null : { error: true, text: restartMessage });
  const [pending, setPending] = useState(false), [readVisit, setReadVisit] = useState(0);
  const submitting = useRef(false), finished = useRef(!request), alive = useRef(true), heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => () => { alive.current = false; }, []);
  useLayoutEffect(() => { if (phase === "ready") heading.current?.focus(); }, [phase]);
  function unavailable(lostSession = false) {
    finished.current = true; clearRequest(); setPreviewName(null); setAuthorization(null); setPhase("unavailable");
    setOutcome({ error: true, text: lostSession ? `Your session is no longer available. ${restartMessage}` : restartMessage });
    if (lostSession) onSessionLoss();
  }
  useEffect(() => {
    if (me || !request || finished.current) return;
    const controller = new AbortController(); setPreviewName(null);
    void api<{ client_name?: unknown }>("/api/mcp/authorization/preview", { request }, controller.signal).then(result => {
      if (controller.signal.aborted) return;
      setPreviewName(typeof result?.client_name === "string" && result.client_name.trim() ? result.client_name : null);
    }).catch(caught => {
      if (controller.signal.aborted) return;
      const error = asError(caught);
      if ([400, 403, 404, 410].includes(error.status) || error.code === "capability_invalid") unavailable();
      // A missing preview is optional context. Sign-in can still resume the binding read.
    });
    return () => controller.abort();
  }, [me?.id, request]);
  useEffect(() => {
    if (!me || !request || finished.current) return;
    const controller = new AbortController(); setAuthorization(null); setPhase("opening"); setOutcome(null);
    void api<MCPAuthorization>("/api/mcp/authorization/read", { request }, controller.signal).then(result => {
      if (controller.signal.aborted) return;
      if (!result || typeof result.username !== "string" || !result.username || typeof result.client_name !== "string" || !result.client_name || typeof result.redirect_host !== "string" || !result.redirect_host
        || !Array.isArray(result.access_options) || !result.access_options.length || result.access_options.some(option => option !== "read" && option !== "read_write")) throw new ApiError("invalid_response");
      setAuthorization(result); setAccess(result.access_options.includes("read") ? "read" : "read_write"); setPhase("ready");
      setOutcome(null);
    }).catch(caught => {
      if (controller.signal.aborted) return;
      const error = asError(caught);
      if (error.status === 401 || error.code === "stale_authority") { unavailable(true); return; }
      if ([400, 403, 404, 410].includes(error.status) || error.code === "capability_invalid") { unavailable(); return; }
      setOutcome({ error: true, text: `${errorMessage(error)} No decision was sent. You can try loading the request again, or return to your client and start again.` });
    });
    return () => controller.abort();
  }, [me?.id, request, readVisit]);
  async function decide(decision: MCPAccess | "deny") {
    if (submitting.current || finished.current || phase !== "ready" || !authorization) return;
    if (decision !== "deny" && !authorization.access_options.includes(decision)) return;
    submitting.current = true; setPending(true); setOutcome({ text: "Submitting decision…" });
    try {
      const result = await api<{ redirect_to: string }>("/api/mcp/authorization/decide", { request, decision });
      if (!alive.current) return;
      const callback = new URL(result.redirect_to);
      if (callback.protocol !== "https:" || callback.hostname !== authorization.redirect_host || callback.username || callback.password) throw new ApiError("invalid_response");
      finished.current = true; clearRequest(); setAuthorization(null); setPhase("redirecting");
      setOutcome({ text: decision === "deny" ? "Access denied. Returning to your client…" : "Access allowed. Returning to your client…" });
      location.assign(callback.href);
    } catch (caught) {
      if (!alive.current) return;
      const error = asError(caught);
      if (error.status === 401 || error.code === "stale_authority" || (error.status === 403 && error.code === "capability_invalid")) { unavailable(true); return; }
      // A lost response can follow a committed decision. Never offer another decision.
      finished.current = true; clearRequest(); setAuthorization(null); setPhase("unavailable");
      setOutcome({ error: true, text: `${errorMessage(error)} The decision could not be confirmed. Check your client before starting a new connection request.` });
    } finally { submitting.current = false; if (alive.current) setPending(false); }
  }
  if (!me && phase !== "unavailable" && phase !== "redirecting") return <CredentialsForm mode="login" token="" clearToken={() => {}} onLogin={onLogin} onSuccess={() => {}} onSessionLoss={() => unavailable(true)} link={link}
    compactLoginCopy={{ description: previewName ? <>Continue connecting <strong>{previewName}</strong> to Meguri.</> : "Sign in to continue connecting your client." }} />;
  return <Card className="auth-card consent-card"><CardHeader><CardTitle ref={heading} tabIndex={-1}>Connect a client</CardTitle>
      {authorization && phase === "ready" && <CardDescription><strong>{authorization.client_name}</strong> wants access to your collection.</CardDescription>}
    </CardHeader>
      <CardContent className="stack"><Outcome value={outcome} />
        {me && phase === "opening" && (outcome ? <Button variant="outline" onClick={() => setReadVisit(value => value + 1)}>Try loading request again</Button> : <p role="status">Loading connection request…</p>)}
        {authorization && phase === "ready" && <form noValidate aria-busy={pending} onSubmit={event => { event.preventDefault(); void decide(access); }}>
          <FieldGroup><dl className="consent-details"><div><dt>Account</dt><dd><strong>{authorization.username}</strong></dd></div><div><dt>Callback hostname</dt><dd>{authorization.redirect_host}</dd></div></dl>
            <fieldset className="consent-choices" disabled={pending}><legend>Collection access</legend>
              {(["read", "read_write"] as const).filter(option => authorization.access_options.includes(option)).map(option => <Field key={option}>
                <FieldLabel className="consent-option" htmlFor={`${prefix}-${option}`}><input id={`${prefix}-${option}`} type="radio" name="access" value={option} checked={access === option} onChange={() => setAccess(option)} aria-labelledby={`${prefix}-${option}-label`} aria-describedby={`${prefix}-${option}-description`} />
                  <span><span id={`${prefix}-${option}-label`}>{accessLabel[option]}</span><span className="muted" id={`${prefix}-${option}-description`}>{option === "read" ? "Find notes, see decks and note types, and download the full collection database." : "Read and download your collection database, add and edit notes, and attach images and audio."}</span></span>
                </FieldLabel>
              </Field>)}
            </fieldset>
            <div className="actions"><Button type="submit" aria-disabled={pending}>{pending ? "Submitting decision…" : "Allow"}</Button><Button variant="outline" aria-disabled={pending} onClick={() => void decide("deny")}>Deny</Button></div>
            <FieldDescription>Different account? Sign out of Meguri, then restart from your client.</FieldDescription>
          </FieldGroup>
        </form>}
        {phase === "unavailable" && link(me ? "/collection" : "/login", me ? "Return to Collection" : "Go to sign in")}
      </CardContent>
    </Card>;
}
